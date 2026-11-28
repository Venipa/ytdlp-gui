//! Cookie set files under `<data>/cookies/sets/<id>.txt`, and the private per-run file a download
//! or probe gets with only the cookies for its link (removed when the run ends).
use std::io::Write as _;
use std::path::{Path, PathBuf};

use domain::cookies::{SetText, select};
use domain::{CookieSet, CookieSource, CookiesMode, CookiesSettings};

/// Which set gave which cookie domains to a run.
pub type Used = Vec<(String, Vec<String>)>;

#[derive(Clone, Debug)]
pub struct CookieFiles {
    dir: PathBuf,
}

/// Removes its run file when dropped (the download or probe task ended or was dropped).
pub struct RunFile(PathBuf);

impl RunFile {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for RunFile {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

impl CookieFiles {
    pub fn new(data: &Path) -> Self {
        Self {
            dir: data.join("cookies"),
        }
    }

    fn set_path(&self, id: &str) -> PathBuf {
        self.dir.join("sets").join(format!("{id}.txt"))
    }

    /// The set's text, `None` when it has none yet (a browser set never read).
    pub fn read(&self, id: &str) -> Option<String> {
        std::fs::read_to_string(self.set_path(id)).ok()
    }

    pub fn write(&self, id: &str, text: &str) -> std::io::Result<()> {
        write_private(&self.set_path(id), text)
    }

    pub fn remove(&self, id: &str) {
        std::fs::remove_file(self.set_path(id)).ok();
    }

    /// A fresh set id (its file name).
    pub fn new_id(&self, now_ms: i64) -> String {
        (0..)
            .map(|n| format!("{now_ms:x}{n}"))
            .find(|id| !self.set_path(id).exists())
            .expect("an unused id")
    }

    /// Writes the cookies switched-on sets have for `url` to a private file named after `run`.
    /// Blocking (reads every set); `None` when no set has cookies for the link.
    pub fn for_run(&self, sets: &[CookieSet], url: &str, run: &str) -> Option<(RunFile, Used)> {
        let texts: Vec<(&CookieSet, String)> = sets
            .iter()
            .filter(|s| s.on)
            .filter_map(|s| Some((s, self.read(&s.id)?)))
            .collect();
        let views: Vec<SetText> = texts
            .iter()
            .map(|(s, text)| SetText {
                name: &s.name,
                off_domains: &s.off_domains,
                text,
            })
            .collect();
        let picked = select(&views, url)?;
        let path = self.dir.join("run").join(format!("{run}.txt"));
        match write_private(&path, &picked.text) {
            Ok(()) => Some((RunFile(path), picked.used)),
            Err(error) => {
                log::warn!("cookies: cannot write {}: {error}", path.display());
                None
            }
        }
    }

    /// Deletes set files no set refers to (a crash between writing a set and saving settings).
    pub fn prune(&self, sets: &[CookieSet]) {
        let Ok(rd) = std::fs::read_dir(self.dir.join("sets")) else {
            return;
        };
        let mut removed = 0;
        for entry in rd.flatten() {
            let path = entry.path();
            let known = path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|id| sets.iter().any(|set| set.id == id));
            if !known {
                match std::fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    Err(e) => log::warn!("cookies: cannot remove an unused cookie set: {e}"),
                }
            }
        }
        if removed > 0 {
            log::info!("cookies: removed {removed} unused cookie sets");
        }
    }

    /// Removes run files a killed app left behind.
    pub fn sweep_runs(&self) {
        std::fs::remove_dir_all(self.dir.join("run")).ok();
    }

    /// Turns the old single-source setting (a file, or a browser) into the first set.
    pub fn migrate(&self, c: &mut CookiesSettings, now_ms: i64) -> bool {
        if !c.sets.is_empty() || c.mode == CookiesMode::None {
            return false;
        }
        let id = self.new_id(now_ms);
        let source = match (c.mode, &c.file, &c.browser) {
            (CookiesMode::File, Some(file), _) => {
                let text = match std::fs::read_to_string(file) {
                    Ok(text) => text,
                    Err(e) => {
                        log::warn!("cookies: cannot migrate the cookies file: {e}");
                        return false;
                    }
                };
                if let Err(e) = self.write(&id, &text) {
                    log::warn!(
                        "cookies: cannot migrate the cookies file, the set was not written: {e}"
                    );
                    return false;
                }
                log::info!("cookies: migrated the cookies file into a set");
                CookieSource::File
            }
            // read on the Cookies page with Refresh
            (CookiesMode::Browser, _, Some(b)) => CookieSource::Browser(b.clone()),
            _ => return false,
        };
        let name = match &source {
            CookieSource::Browser(b) => browser_name(b),
            _ => "cookies.txt".into(),
        };
        c.sets.push(CookieSet {
            id,
            name,
            source,
            updated_ms: now_ms,
            on: true,
            off_domains: vec![],
        });
        c.mode = CookiesMode::None;
        c.file = None;
        true
    }
}

/// Display name of a yt-dlp browser key.
pub fn browser_name(key: &str) -> String {
    match key {
        "firefox" => "Firefox",
        "chrome" => "Chrome",
        "edge" => "Edge",
        "safari" => "Safari",
        "brave" => "Brave",
        "chromium" => "Chromium",
        "opera" => "Opera",
        "vivaldi" => "Vivaldi",
        other => other,
    }
    .to_string()
}

/// Writes `text` readable by the owner only: mode 0600 on unix, an owner-only ACL on Windows.
pub fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)?.write_all(text.as_bytes())?;
    #[cfg(windows)]
    restrict_acl(path)?;
    Ok(())
}

#[cfg(windows)]
fn restrict_acl(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let user = std::env::var("USERNAME").map_err(std::io::Error::other)?;
    let st = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(format!("{user}:(R,W,D)"))
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()?;
    if !st.status.success() {
        return Err(std::io::Error::other(
            "cannot restrict the cookie file permissions",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_file_holds_only_the_site_and_goes_away() {
        let dir = tempfile::tempdir().unwrap();
        let files = CookieFiles::new(dir.path());
        let text = "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t1900000000\tSID\ta\nvimeo.com\tFALSE\t/\tTRUE\t1900000000\tv\tb\n";
        files.write("s1", text).unwrap();
        let set = CookieSet {
            id: "s1".into(),
            name: "Main".into(),
            on: true,
            ..Default::default()
        };
        let (run, used) = files
            .for_run(std::slice::from_ref(&set), "https://vimeo.com/1", "7")
            .unwrap();
        let written = std::fs::read_to_string(run.path()).unwrap();
        assert!(written.contains("\tv\t") && !written.contains("SID"));
        assert_eq!(
            used,
            vec![("Main".to_string(), vec!["vimeo.com".to_string()])]
        );
        #[cfg(unix)]
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(
                &std::fs::metadata(run.path()).unwrap().permissions()
            ) & 0o777,
            0o600
        );
        let path = run.path().to_path_buf();
        drop(run);
        assert!(!path.exists());
        let off = CookieSet { on: false, ..set };
        assert!(files.for_run(&[off], "https://vimeo.com/1", "8").is_none());
    }

    #[test]
    fn prune_keeps_only_files_of_known_sets() {
        let dir = tempfile::tempdir().unwrap();
        let files = CookieFiles::new(dir.path());
        files.write("kept", "x").unwrap();
        files.write("orphan", "x").unwrap();
        let kept = CookieSet {
            id: "kept".into(),
            ..Default::default()
        };
        files.prune(&[kept]);
        assert!(files.read("kept").is_some() && files.read("orphan").is_none());
    }

    #[test]
    fn old_file_setting_becomes_a_set() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.txt");
        std::fs::write(&old, "a.com\tFALSE\t/\tFALSE\t0\tk\tv\n").unwrap();
        let files = CookieFiles::new(dir.path());
        let mut c = CookiesSettings {
            mode: CookiesMode::File,
            file: Some(old.to_string_lossy().into()),
            ..Default::default()
        };
        assert!(files.migrate(&mut c, 1));
        assert_eq!(c.sets.len(), 1);
        assert!(c.sets[0].on && c.mode == CookiesMode::None);
        assert!(files.read(&c.sets[0].id).unwrap().contains("a.com"));
        assert!(!files.migrate(&mut c, 2));
    }
}
