//! Putting a verified update in place (ported from maple `updates/install.rs`). Every install
//! waits for this process to exit before it touches the running copy, then optionally starts the
//! new one: Windows runs the Inno installer silently, macOS swaps the `.app` taken off the dmg,
//! Linux swaps the AppImage.
use std::path::{Path, PathBuf};
#[cfg(any(windows, unix))]
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context as _, Result};

/// How this copy was installed, which decides whether it may replace itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Install {
    /// Installed by our Inno setup (its uninstaller sits beside the exe).
    #[cfg(windows)]
    Setup(PathBuf),
    /// An app bundle in a folder this user can write.
    #[cfg(target_os = "macos")]
    Bundle(PathBuf),
    /// An AppImage in a folder this user can write.
    #[cfg(all(unix, not(target_os = "macos")))]
    AppImage(PathBuf),
    /// A dev build, a portable copy or a package manager's: it only links to the release.
    Managed,
}

impl Install {
    /// Works out how this copy was installed, once per run.
    pub(super) fn current() -> &'static Self {
        static CURRENT: OnceLock<Install> = OnceLock::new();
        CURRENT.get_or_init(detect)
    }

    pub(super) fn updates_itself(&self) -> bool {
        *self != Self::Managed
    }

    /// Readies a verified download beside what it replaces (macOS: copies the `.app` off the
    /// dmg; Linux: an executable copy beside the AppImage). Blocking.
    pub(super) fn prepare(&self, download: &Path) -> Result<PathBuf> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Bundle(bundle) => unpack_bundle(download, &beside(bundle, ".ytdlpd.app.update")),
            #[cfg(all(unix, not(target_os = "macos")))]
            Self::AppImage(image) => {
                stage_image(download, &beside(image, ".ytdlpd-update.AppImage"))
            }
            _ => Ok(download.to_path_buf()),
        }
    }

    /// Schedules the prepared update to go in once this process exits, then the new copy to
    /// start when `relaunch`. The caller quits right after.
    pub(super) fn launch(&self, prepared: &Path, relaunch: bool) -> Result<()> {
        match self {
            #[cfg(windows)]
            Self::Setup(exe) => setup(prepared, exe, relaunch),
            #[cfg(target_os = "macos")]
            Self::Bundle(bundle) => after_exit(
                r#"rm -rf "$2.old"; mv "$2" "$2.old" && { mv "$1" "$2" || mv "$2.old" "$2"; }; rm -rf "$2.old"; [ "$3" = 1 ] && exec /usr/bin/open "$2""#,
                prepared,
                bundle,
                relaunch,
            ),
            #[cfg(all(unix, not(target_os = "macos")))]
            Self::AppImage(image) => after_exit(
                r#"mv -f "$1" "$2"; [ "$3" = 1 ] && exec env -u APPDIR -u APPIMAGE -u ARGV0 -u OWD "$2""#,
                prepared,
                image,
                relaunch,
            ),
            Self::Managed => {
                let _ = (prepared, relaunch);
                anyhow::bail!("this copy of ytdlpd does not update itself")
            }
        }
    }
}

#[cfg(any(target_os = "macos", all(unix, not(target_os = "macos"))))]
fn beside(path: &Path, name: &str) -> PathBuf {
    path.with_file_name(name)
}

#[cfg(windows)]
fn detect() -> Install {
    let Ok(exe) = std::env::current_exe() else {
        return Install::Managed;
    };
    let installed = exe
        .parent()
        .is_some_and(|d| d.join("unins000.exe").exists());
    // the fixture server (YTDLPD_UPDATES_API) lets a dev build walk the real install path
    match installed || std::env::var_os(super::API_ENV).is_some() {
        true => Install::Setup(exe),
        false => Install::Managed,
    }
}

/// A bundle counts when it is not on a disk image, translocated, in the Nix store or unwritable.
#[cfg(target_os = "macos")]
fn detect() -> Install {
    let bundle = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.parent()?.to_path_buf()))
        .filter(|b| b.extension().is_some_and(|e| e == "app"));
    let Some(bundle) = bundle else {
        return Install::Managed;
    };
    let foreign = bundle.starts_with("/Volumes")
        || bundle.starts_with("/nix/store")
        || bundle.to_string_lossy().contains("/AppTranslocation/");
    match !foreign && bundle.parent().is_some_and(writable) {
        true => Install::Bundle(bundle),
        false => Install::Managed,
    }
}

/// The AppImage runtime names the image it started from in `APPIMAGE`.
#[cfg(all(unix, not(target_os = "macos")))]
fn detect() -> Install {
    match std::env::var_os("APPIMAGE").map(PathBuf::from) {
        Some(image) if image.is_file() && image.parent().is_some_and(writable) => {
            Install::AppImage(image)
        }
        _ => Install::Managed,
    }
}

#[cfg(not(any(windows, unix)))]
fn detect() -> Install {
    Install::Managed
}

#[cfg(unix)]
fn writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".ytdlpd-write-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    std::fs::remove_file(&probe).ok();
    ok
}

/// Quotes for a PowerShell single-quoted string.
#[cfg(windows)]
fn ps_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// A hidden PowerShell waits for this process, runs the installer silently for this user into
/// the same folder, then starts ytdlpd again (the old copy too when the installer failed, so a
/// relaunch never leaves nothing running).
#[cfg(windows)]
fn setup(installer: &Path, exe: &Path, relaunch: bool) -> Result<()> {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let dir = exe.parent().context("the exe has no folder")?;
    let dir_arg = format!("/DIR=\"{}\"", dir.display()).replace('\'', "''");
    let mut script = format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; \
         $p = Start-Process -FilePath {installer} -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/CURRENTUSER','{dir_arg}' -Wait -PassThru; ",
        pid = std::process::id(),
        installer = ps_quote(installer),
    );
    if relaunch {
        script.push_str(&format!("Start-Process -FilePath {}", ps_quote(exe)));
    }
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
        ])
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("cannot start the installer")?;
    Ok(())
}

/// Runs `script` (`$1` prepared, `$2` target, `$3` relaunch) from a shell that first waits for
/// this process to exit, so the new copy never runs beside the old one.
#[cfg(unix)]
fn after_exit(script: &str, prepared: &Path, target: &Path, relaunch: bool) -> Result<()> {
    let wait = r#"pid=$4; while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done; "#;
    Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("{wait}{script}"))
        .arg("sh")
        .arg(prepared)
        .arg(target)
        .arg(if relaunch { "1" } else { "0" })
        .arg(std::process::id().to_string())
        .spawn()
        .context("cannot schedule the update")?;
    Ok(())
}

/// Copies `ytdlpd.app` off the dmg into `staged` beside the running bundle, then ejects.
#[cfg(target_os = "macos")]
fn unpack_bundle(image: &Path, staged: &Path) -> Result<PathBuf> {
    let mount = tempfile::tempdir().context("cannot make a mount point")?;
    run(Command::new("/usr/bin/hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(mount.path())
        .arg(image))
    .context("cannot open the disk image")?;
    std::fs::remove_dir_all(staged).ok();
    let copied = run(Command::new("/usr/bin/ditto")
        .arg(mount.path().join("ytdlpd.app"))
        .arg(staged))
    .context("cannot copy the new version off the disk image");
    if let Err(error) = run(Command::new("/usr/bin/hdiutil")
        .args(["detach", "-quiet"])
        .arg(mount.path()))
    {
        log::warn!("updates: cannot eject the disk image: {error:#}");
    }
    copied?;
    Ok(staged.to_path_buf())
}

#[cfg(target_os = "macos")]
fn run(command: &mut Command) -> Result<()> {
    let output = command.output().context("cannot start the command")?;
    anyhow::ensure!(
        output.status.success(),
        "{} exited with {}: {}",
        command.get_program().to_string_lossy(),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

/// An executable copy beside the old image, so the later rename stays on one filesystem.
#[cfg(all(unix, not(target_os = "macos")))]
fn stage_image(download: &Path, staged: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::copy(download, staged).context("cannot copy the new AppImage beside the old")?;
    std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755))
        .context("cannot make the new AppImage executable")?;
    Ok(staged.to_path_buf())
}
