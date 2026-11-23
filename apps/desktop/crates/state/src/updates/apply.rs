//! The Windows update card's work, run by a copy of the exe outside the install folder: waits for
//! the app to exit, runs the Inno installer and reports its progress.
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, GetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// The flag (followed by a JSON [`Plan`]) that starts the update card instead of the app.
pub const FLAG: &str = "--apply-update";
/// Where the copy runs from, beside the downloads.
pub(super) const RUN_DIR: &str = "run";
const EXE: &str = "ytdlpd.exe";
/// How long the app may take to exit.
const EXIT_WAIT: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_millis(100);

/// What the card installs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// The verified setup file.
    pub installer: PathBuf,
    /// The install folder.
    pub dir: PathBuf,
    /// The app to wait for.
    pub pid: u32,
    /// The version that was running.
    pub from: String,
    /// The version the installer brings.
    pub to: String,
    /// Opens the app again afterwards.
    pub relaunch: bool,
}

/// Why the update did not go in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The Windows permission prompt was declined.
    Declined,
    /// The app did not exit.
    StillRunning,
    /// The installer did not start.
    Start,
    /// The installer ended with this code.
    Exit(u32),
}

impl Plan {
    /// The plan on this process's command line, when it was started as the update card.
    pub fn from_args(mut args: impl Iterator<Item = String>) -> Option<Self> {
        args.find(|a| a == FLAG)?;
        serde_json::from_str(&args.next()?).ok()
    }

    /// Copies the running exe and the DLLs it loads beside the download and starts the card there,
    /// so nothing in the install folder is in use while Setup replaces it.
    pub(super) fn start(&self, exe: &Path) -> Result<()> {
        let run = self.folder().join(RUN_DIR);
        std::fs::create_dir_all(&run).context("cannot make the update folder")?;
        for entry in std::fs::read_dir(&self.dir).context("cannot list the install folder")? {
            let path = entry.context("cannot list the install folder")?.path();
            if let (Some(name), Some("dll")) =
                (path.file_name(), path.extension().and_then(OsStr::to_str))
            {
                std::fs::copy(&path, run.join(name)).context("cannot copy a library")?;
            }
        }
        std::fs::copy(exe, run.join(EXE)).context("cannot copy the app")?;
        std::process::Command::new(run.join(EXE))
            .arg(FLAG)
            .arg(serde_json::to_string(self)?)
            .spawn()
            .context("cannot start the update window")?;
        Ok(())
    }

    /// Setup's log of the last try.
    pub fn log(&self) -> PathBuf {
        self.folder().join("setup.log")
    }

    fn progress(&self) -> PathBuf {
        self.folder().join("setup.progress")
    }

    fn folder(&self) -> &Path {
        self.installer.parent().unwrap_or(&self.dir)
    }

    /// Opens the installed app.
    pub fn open_app(&self) -> Result<()> {
        std::process::Command::new(self.dir.join(EXE))
            .spawn()
            .context("cannot open ytdlpd")?;
        Ok(())
    }

    /// A folder this user cannot write (an all-users install in Program Files) needs `/ALLUSERS`
    /// and admin rights; otherwise Setup keeps the mode of the earlier install.
    fn setup_args(&self, writable: bool) -> String {
        format!(
            "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART {}/DIR=\"{}\" /LOG=\"{}\" /PROGRESS=\"{}\"",
            if writable { "" } else { "/ALLUSERS " },
            self.dir.display(),
            self.log().display(),
            self.progress().display(),
        )
    }

    /// Installs, calling `percent` as Setup reports progress. Blocking.
    pub fn run(&self, mut percent: impl FnMut(u8)) -> Result<(), Failure> {
        wait_exit(self.pid)?;
        std::fs::remove_file(self.progress()).ok();
        std::fs::remove_file(self.log()).ok();
        let writable = super::install::writable(&self.dir);
        let verb = wide(if writable { "open" } else { "runas" });
        let file = wide(&self.installer);
        let args = wide(self.setup_args(writable));
        // SAFETY: the struct is plain data; every pointer outlives the call.
        let process = unsafe {
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
            info.lpVerb = verb.as_ptr();
            info.lpFile = file.as_ptr();
            info.lpParameters = args.as_ptr();
            info.nShow = SW_SHOWNORMAL;
            if ShellExecuteExW(&mut info) == 0 {
                let error = GetLastError();
                log::warn!("updates: cannot start the installer: error {error}");
                return Err(match error {
                    ERROR_CANCELLED => Failure::Declined,
                    _ => Failure::Start,
                });
            }
            info.hProcess
        };
        if process.is_null() {
            return Err(Failure::Start);
        }
        let mut code = u32::MAX;
        // SAFETY: `process` is the handle ShellExecuteExW gave us, closed once.
        unsafe {
            while WaitForSingleObject(process, TICK.as_millis() as u32) == WAIT_TIMEOUT {
                let text = std::fs::read_to_string(self.progress()).unwrap_or_default();
                if let Some(p) = last_percent(&text) {
                    percent(p);
                }
            }
            GetExitCodeProcess(process, &mut code);
            CloseHandle(process);
        }
        match code {
            0 => Ok(()),
            code => Err(Failure::Exit(code)),
        }
    }
}

/// The newest whole line; Setup may be halfway through writing the next.
fn last_percent(text: &str) -> Option<u8> {
    let whole = &text[..text.rfind('\n')?];
    let last: u8 = whole.lines().last()?.trim().parse().ok()?;
    Some(last.min(100))
}

fn wait_exit(pid: u32) -> Result<(), Failure> {
    // SAFETY: a null handle (already gone) is checked; the handle is closed once.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return Ok(());
        }
        let ended = WaitForSingleObject(process, EXIT_WAIT.as_millis() as u32) == WAIT_OBJECT_0;
        CloseHandle(process);
        ended.then_some(()).ok_or(Failure::StillRunning)
    }
}

fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan {
            installer: r"C:\c\updates\s.exe".into(),
            dir: r"C:\Program Files\ytdlpd".into(),
            pid: 1,
            from: "1.0.5".into(),
            to: "1.0.6".into(),
            relaunch: true,
        }
    }

    #[test]
    fn asks_for_all_users_only_where_it_cannot_write() {
        let rest = r#"/DIR="C:\Program Files\ytdlpd" /LOG="C:\c\updates\setup.log" /PROGRESS="C:\c\updates\setup.progress""#;
        let base = "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART";
        assert_eq!(plan().setup_args(true), format!("{base} {rest}"));
        assert_eq!(plan().setup_args(false), format!("{base} /ALLUSERS {rest}"));
    }

    #[test]
    fn plan_survives_the_command_line() {
        let json = serde_json::to_string(&plan()).unwrap();
        let args = ["ytdlpd".to_string(), FLAG.to_string(), json];
        assert_eq!(Plan::from_args(args.into_iter()), Some(plan()));
        assert_eq!(Plan::from_args(["ytdlpd".to_string()].into_iter()), None);
    }

    #[test]
    fn progress_reads_whole_lines() {
        assert_eq!(last_percent(""), None);
        assert_eq!(last_percent("1"), None);
        assert_eq!(last_percent("61\n6"), Some(61));
        assert_eq!(last_percent("61\r\n62\r\n"), Some(62));
        assert_eq!(last_percent("x\n"), None);
    }
}
