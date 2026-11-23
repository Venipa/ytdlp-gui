//! The few operating-system calls the Downloads page makes: reveal a file, open it, read the free
//! space of a drive.

use std::path::{Path, PathBuf};

/// Opens the file manager with `path` selected (its folder when the file is gone).
pub fn show_in_folder(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        let target = if path.exists() {
            format!("/select,\"{}\"", path.display())
        } else {
            format!("\"{}\"", path.parent().unwrap_or(path).display())
        };
        // explorer exits 1 even when it worked, so the status is not checked
        std::process::Command::new("explorer")
            .raw_arg(target)
            .spawn()
            .ok();
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .ok();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // shortcut: opens the folder without selecting the file, upgrade with the FileManager1 D-Bus call
        open::that_detached(path.parent().unwrap_or(path)).ok();
    }
}

/// Opens `path` with its default application.
pub fn open_path(path: &Path) {
    if let Err(error) = open::that_detached(path) {
        log::warn!("views: cannot open {}: {error}", path.display());
    }
}

/// The nearest folder at or above `dir` that exists.
fn existing(dir: &Path) -> PathBuf {
    dir.ancestors()
        .find(|p| p.exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Free bytes on the drive holding `dir`, and that drive's name (`D:` on Windows, the folder
/// elsewhere).
pub fn free_space(dir: &Path) -> Option<(u64, String)> {
    let at = existing(dir);
    let free = free_bytes(&at)?;
    let drive = at
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    Some((free, drive))
}

#[cfg(windows)]
fn free_bytes(at: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = at.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0u64;
    // SAFETY: `wide` is NUL terminated and outlives the call; the out pointers are valid or null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available)
}

#[cfg(unix)]
fn free_bytes(at: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt as _;
    let path = std::ffi::CString::new(at.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a valid C string and `stat` is written by a successful call.
    let ok = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if ok != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so the struct is initialised.
    let stat = unsafe { stat.assume_init() };
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_of_the_temp_dir_is_known() {
        let (free, drive) = free_space(&std::env::temp_dir().join("missing-child")).unwrap();
        assert!(free > 0);
        assert!(!drive.is_empty());
    }
}
