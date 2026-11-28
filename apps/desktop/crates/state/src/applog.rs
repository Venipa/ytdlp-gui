//! The app's `log` backend: every line from ytdlpd's own crates (info and up; warnings and
//! errors from libraries) goes to stderr, to `<data>/logs/ytdlpd.log` and to the Logs screen.
//! `YTDLPD_LOG=debug` adds debug lines.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use log::{Level, LevelFilter, Log, Metadata, Record};

/// The workspace crates whose info lines are kept; other targets keep warnings and errors.
const OWN: &[&str] = &[
    "ytdlpd", "state", "engine", "tools", "storage", "domain", "views", "bridge", "cli", "embed",
    "router", "input", "i18n", "icons", "ui",
];
/// The log file is moved to `ytdlpd.old.log` when it passes this size at start.
const ROTATE_AT: u64 = 5 * 1024 * 1024;

/// Target of per-job engine lines: written to stderr and the file, but the Logs screen gets them
/// from the downloads model (with the job id), so they are not forwarded here.
pub const JOB_TARGET: &str = "downloads::job";

/// One line for the Logs screen: level word and message.
pub type Line = (String, String);

struct AppLog {
    least: Level,
    file: Mutex<Option<File>>,
    screen: UnboundedSender<Line>,
}

static LOGGER: OnceLock<&'static AppLog> = OnceLock::new();
static SCREEN: Mutex<Option<UnboundedReceiver<Line>>> = Mutex::new(None);

fn own(target: &str) -> bool {
    OWN.contains(&target.split("::").next().unwrap_or(target))
}

impl Log for AppLog {
    fn enabled(&self, meta: &Metadata) -> bool {
        match own(meta.target()) {
            true => meta.level() <= self.least,
            false => meta.level() <= Level::Warn,
        }
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = record.level().as_str().to_lowercase();
        let message = record.args().to_string();
        let line = format!(
            "{} {:5} {}: {message}",
            jiff::Timestamp::now().strftime("%Y-%m-%dT%H:%M:%S%.3fZ"),
            record.level(),
            record.target()
        );
        eprintln!("{line}");
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = writeln!(file, "{line}");
        }
        if record.target() == JOB_TARGET {
            return;
        }
        // messages already carry their subsystem ("fixes: ..."); library lines get their target
        let message = match own(record.target()) {
            true => message,
            false => format!("{}: {message}", record.target()),
        };
        let _ = self.screen.unbounded_send((level, message));
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.flush();
        }
    }
}

/// Installs the logger; call once, first thing in `main`. Lines wait in memory until the Logs
/// screen takes them ([`take_screen`]) and the file is opened ([`attach_file`]).
pub fn install() {
    let least = match std::env::var("YTDLPD_LOG").as_deref() {
        Ok("debug") | Ok("trace") => Level::Debug,
        _ => Level::Info,
    };
    let (screen, rx) = unbounded();
    *SCREEN.lock().unwrap() = Some(rx);
    let logger: &'static AppLog = Box::leak(Box::new(AppLog {
        least,
        file: Mutex::new(None),
        screen,
    }));
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map_or_else(String::new, |l| l.to_string());
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        log::error!("ytdlpd: panic at {at}: {msg}");
        previous(info);
    }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Debug);
        LOGGER.set(logger).ok();
    }
}

/// Appends lines to `<dir>/ytdlpd.log` from now on, moving a large one aside first.
pub fn attach_file(dir: &Path) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let path = dir.join("ytdlpd.log");
    let opened = std::fs::create_dir_all(dir).and_then(|_| {
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > ROTATE_AT) {
            std::fs::rename(&path, dir.join("ytdlpd.old.log"))?;
        }
        OpenOptions::new().create(true).append(true).open(&path)
    });
    match opened {
        Ok(file) => *logger.file.lock().unwrap() = Some(file),
        Err(error) => log::warn!(
            "state: cannot open the log file {}: {error}",
            path.display()
        ),
    }
}

/// The lines for the Logs screen, once; `None` when the logger is not installed or taken.
pub fn take_screen() -> Option<UnboundedReceiver<Line>> {
    SCREEN.lock().ok()?.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_own_info_and_library_warnings() {
        let (screen, _rx) = unbounded();
        let log = AppLog {
            least: Level::Info,
            file: Mutex::new(None),
            screen,
        };
        let meta = |level, target| Metadata::builder().level(level).target(target).build();
        assert!(log.enabled(&meta(Level::Info, "state::fixes")));
        assert!(!log.enabled(&meta(Level::Debug, "engine::runtime")));
        assert!(!log.enabled(&meta(Level::Info, "gpui::window")));
        assert!(log.enabled(&meta(Level::Warn, "gpui::window")));
    }

    #[test]
    fn job_lines_skip_the_screen() {
        let (screen, mut rx) = unbounded();
        let log = AppLog {
            least: Level::Info,
            file: Mutex::new(None),
            screen,
        };
        let line = |target| {
            log.log(
                &Record::builder()
                    .level(Level::Warn)
                    .target(target)
                    .args(format_args!("x"))
                    .build(),
            )
        };
        line(JOB_TARGET);
        assert!(rx.try_recv().is_err());
        line("downloads");
        assert!(rx.try_recv().is_ok());
    }
}
