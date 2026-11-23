//! OS notifications behind a small trait so tests and demo scenes can fake them.
use std::cell::RefCell;

/// Shows a notification.
pub trait Notifier {
    /// Show `title` and `body`; failures are logged, never returned (a missing notification daemon is normal).
    fn notify(&self, title: &str, body: &str);
}

/// Real notifications through notify-rust.
pub struct Desktop;

impl Notifier for Desktop {
    fn notify(&self, title: &str, body: &str) {
        let (title, body) = (title.to_owned(), body.to_owned());
        // `show` can block on the platform service, so keep it off the UI thread
        std::thread::spawn(move || {
            // Unset, mac-notification-sys asks AppleScript for an app named "use_default", which
            // macOS answers with a "Where is use_default?" file picker. Only the first call counts.
            #[cfg(target_os = "macos")]
            let _ = notify_rust::set_application(if cfg!(debug_assertions) {
                "com.apple.Terminal" // a dev binary is not a registered bundle
            } else {
                "dev.ytdlpd.app" // CFBundleIdentifier in scripts/package-macos.sh
            });
            if let Err(e) = notify_rust::Notification::new()
                .appname("ytdlpd")
                .summary(&title)
                .body(&body)
                .show()
            {
                log::warn!("platform: cannot show a notification: {e}");
            }
        });
    }
}

/// Records notifications instead of showing them.
#[derive(Default)]
pub struct Recorder(pub RefCell<Vec<(String, String)>>);

impl Notifier for Recorder {
    fn notify(&self, title: &str, body: &str) {
        self.0.borrow_mut().push((title.into(), body.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_records() {
        let r = Recorder::default();
        let n: &dyn Notifier = &r;
        n.notify("Done", "file.mp4");
        assert_eq!(
            r.0.borrow().as_slice(),
            [("Done".to_string(), "file.mp4".to_string())]
        );
    }
}
