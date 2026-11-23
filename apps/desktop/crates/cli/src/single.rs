//! Single instance: the first launch owns a local socket; later launches forward a request and exit.
use std::io::{self, BufReader, ErrorKind};
use std::path::Path;
use std::thread;
use std::time::Duration;

use interprocess::local_socket::traits::{ListenerExt as _, Stream as _};
use interprocess::local_socket::{ListenerOptions, Name, Stream};

use crate::wire::{self, Reply, Request};

/// How long the running app waits for a peer to speak.
const PATIENCE: Duration = Duration::from_secs(5);
/// How long a second launch waits for the answer before giving up (the exit budget is 2 s).
const ANSWER: Duration = Duration::from_millis(1500);

/// Outcome of [`claim`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instance {
    /// This process owns the socket and serves requests.
    First,
    /// Another instance took the request; this process should exit.
    Running,
    /// No socket could be used; run as a normal instance.
    Failed,
}

/// Socket name for `app` (`app_dir_name()`); a `--data-dir` override gets its own instance so a
/// test or screenshot run never forwards into the developer's running app.
pub fn socket_name(app: &str, data_dir: Option<&Path>) -> String {
    match data_dir {
        None => format!("{app}.sock"),
        Some(d) => {
            // FNV-1a: stable across builds, unlike DefaultHasher
            let h = d
                .to_string_lossy()
                .bytes()
                .fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
                });
            format!("{app}-{h:016x}.sock")
        }
    }
}

/// Try to become the first instance. When another one runs, hand `request` to it. `on_request`
/// runs on a socket thread for every request a later launch forwards.
pub fn claim(
    socket: &str,
    request: &Request,
    on_request: impl Fn(Request) + Send + Sync + 'static,
) -> Instance {
    let name = match name(socket) {
        Ok(n) => n,
        Err(e) => {
            log::warn!("single: cannot name the instance socket: {e:#}");
            return Instance::Failed;
        }
    };
    let listener = match options(name.clone()).create_sync() {
        Ok(l) => l,
        Err(e) => {
            // Windows reports an occupied pipe with an error other than AddrInUse
            if hand_over(name.clone(), request) {
                return Instance::Running;
            }
            if e.kind() != ErrorKind::AddrInUse {
                log::error!("single: cannot own the instance socket: {e:#}");
                return Instance::Failed;
            }
            log::warn!("single: socket is occupied but unreachable; attempting recovery");
            match options(name.clone())
                .try_overwrite(true)
                .max_spin_time(Duration::ZERO)
                .create_sync()
            {
                Ok(l) => l,
                Err(e) => {
                    log::error!("single: cannot recover instance socket: {e:#}");
                    return Instance::Failed;
                }
            }
        }
    };
    let on_request = std::sync::Arc::new(on_request);
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let on_request = on_request.clone();
            thread::spawn(move || serve(conn, &*on_request));
        }
    });
    Instance::First
}

fn serve(conn: Stream, on_request: &dyn Fn(Request)) {
    if let Err(e) = conn.set_recv_timeout(Some(PATIENCE)) {
        log::warn!("single: cannot bound the read: {e:#}");
    }
    let reply = match wire::receive::<Request>(&mut BufReader::new(&conn)) {
        Ok(r) => {
            on_request(r);
            Reply::Done
        }
        Err(e) => {
            log::warn!("single: cannot read a request: {e}");
            Reply::Failed(e.to_string())
        }
    };
    if let Err(e) = wire::send(&mut &conn, &reply) {
        log::warn!("single: cannot answer: {e}");
    }
}

fn hand_over(name: Name<'_>, request: &Request) -> bool {
    let conn = match Stream::connect(name) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("single: cannot reach the running instance: {e:#}");
            return false;
        }
    };
    // Not every platform's local socket supports a read timeout (Windows pipes do not), so the
    // exchange runs on its own thread and the wait for it is bounded here.
    let request = request.clone();
    let (done, answered) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let ok = wire::send(&mut &conn, &request).is_ok()
            && matches!(
                wire::receive::<Reply>(&mut BufReader::new(&conn)),
                Ok(Reply::Done)
            );
        done.send(ok).ok();
    });
    answered.recv_timeout(ANSWER).unwrap_or(false)
}

/// The socket as a file in a folder only this user can enter.
#[cfg(unix)]
fn name(socket: &str) -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericFilePath, ToFsName as _};
    private_dir()?.join(socket).to_fs_name::<GenericFilePath>()
}

/// The socket as a named pipe.
#[cfg(windows)]
fn name(socket: &str) -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName as _};
    socket.to_owned().to_ns_name::<GenericNamespaced>()
}

/// The user's runtime folder (readable by this user alone); the cache folder without one.
#[cfg(all(unix, not(target_os = "macos")))]
fn private_dir() -> io::Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt as _;
    use std::path::PathBuf;

    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let dir = match (runtime, std::env::var_os("FLATPAK_ID")) {
        // a Flatpak sandbox gets a fresh runtime dir per launch, so use the app's own subfolder
        (Some(r), Some(app)) => r.join("app").join(app),
        (Some(r), None) => return Ok(r),
        (None, _) => dirs::cache_dir()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "no runtime or cache folder"))?
            .join("ytdlpd"),
    };
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    Ok(dir)
}

/// The per-user temporary folder macOS keeps under `/var/folders`; asked of the system, not `TMPDIR`.
#[cfg(target_os = "macos")]
fn private_dir() -> io::Result<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: confstr writes at most `buffer.len()` bytes into the buffer it is given.
    let length = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if length == 0 || length > buffer.len() {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(length - 1); // the length counts the NUL
    Ok(OsString::from_vec(buffer).into())
}

fn options(name: Name<'_>) -> ListenerOptions<'_> {
    let options = ListenerOptions::new().name(name);
    #[cfg(windows)]
    let options = match reachable() {
        Some(d) => {
            use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
            options.security_descriptor(d)
        }
        None => options,
    };
    options
}

/// Pipe access for this user's account alone, elevated or not: an elevated process owns its
/// objects as Administrators, so the default descriptor would lock out a later unelevated launch.
#[cfg(windows)]
fn reachable() -> Option<interprocess::os::windows::security_descriptor::SecurityDescriptor> {
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    let described = user_sid()
        .and_then(|sid| {
            widestring::U16CString::from_str(format!("D:(A;;GA;;;{sid})"))
                .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
        })
        .and_then(|d| SecurityDescriptor::deserialize(&d));
    match described {
        Ok(d) => Some(d),
        Err(e) => {
            log::warn!("single: cannot describe the socket's access: {e:#}");
            None
        }
    }
}

/// The SID of the account this process runs as, in `S-1-5-...` text form.
#[cfg(windows)]
fn user_sid() -> io::Result<String> {
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo handle of the current process needs no closing, and `token` is a live out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut length = 0u32;
    // SAFETY: a null buffer of length zero only asks for the size the answer needs.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length) };
    // u64 cells keep the buffer aligned for the pointer at the start of TOKEN_USER
    let mut buffer = vec![0u64; (length as usize).div_ceil(size_of::<u64>())];
    // SAFETY: the buffer holds at least `length` bytes, and the token is open until closed below.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    };
    let error = io::Error::last_os_error();
    // SAFETY: the token was opened above and is not used again.
    unsafe { CloseHandle(token) };
    if read == 0 {
        return Err(error);
    }
    // SAFETY: GetTokenInformation filled the buffer with a TOKEN_USER whose SID points into it.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text = ptr::null_mut();
    // SAFETY: `sid` is valid while `buffer` lives, and `text` is a live out pointer.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ConvertSidToStringSidW returned a NUL-terminated string that LocalFree releases.
    let sid = unsafe { widestring::U16CStr::from_ptr_str(text) }.to_string_lossy();
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn socket_names_differ_per_data_dir() {
        let a = socket_name("ytdlpd", None);
        assert_eq!(a, "ytdlpd.sock");
        assert_ne!(
            socket_name("ytdlpd", Some(Path::new("/a"))),
            socket_name("ytdlpd", Some(Path::new("/b")))
        );
        assert_eq!(
            socket_name("x", Some(Path::new("/a"))),
            socket_name("x", Some(Path::new("/a")))
        );
    }

    #[test]
    fn second_instance_forwards_and_returns_fast() {
        let sock = format!("ytdlpd-test-{}.sock", std::process::id());
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let first = claim(&sock, &Request::Show, move |r| {
            drop(tx.lock().unwrap().send(r))
        });
        assert_eq!(first, Instance::First);
        let t = Instant::now();
        let req = Request::Add {
            urls: vec!["https://a.b/".into()],
        };
        assert_eq!(claim(&sock, &req, |_| {}), Instance::Running);
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), req);
    }
}
