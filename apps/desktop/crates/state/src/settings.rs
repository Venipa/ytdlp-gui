//! `AppSettings`: settings.json with hot reload (notify) and debounced save.
use std::path::PathBuf;
use std::time::Duration;

use domain::{JobSpec, Preset, Settings};
use futures::StreamExt as _;
use gpui::{Context, EventEmitter, Task};
use notify::{RecursiveMode, Watcher as _};

/// Quiet period before a change is written.
const SAVE_DELAY: Duration = Duration::from_millis(500);
/// Quiet period before an external edit is reloaded (editors write in bursts).
const RELOAD_DELAY: Duration = Duration::from_millis(200);

/// Emitted after the settings value changed (own edit or external reload).
pub struct SettingsChanged;

/// The settings value plus its file.
pub struct AppSettings {
    settings: Settings,
    path: PathBuf,
    /// The parse error of the last reload; the last good value stays active.
    error: Option<String>,
    save: Option<Task<()>>,
    watch: Option<Task<()>>,
    _watcher: Option<notify::RecommendedWatcher>,
}

impl EventEmitter<SettingsChanged> for AppSettings {}

impl AppSettings {
    /// Loads `path` (defaults when missing) and starts watching its directory.
    pub fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let loaded = Settings::load(&path, &Settings::default());
        let mut this = Self {
            settings: loaded.settings,
            error: loaded.error,
            path,
            save: None,
            watch: None,
            _watcher: None,
        };
        this.start_watching(cx);
        this
    }

    /// In-memory settings without a watcher (tests).
    pub fn detached(path: PathBuf, settings: Settings) -> Self {
        Self {
            settings,
            path,
            error: None,
            save: None,
            watch: None,
            _watcher: None,
        }
    }

    fn start_watching(&mut self, cx: &mut Context<Self>) {
        let (watcher, mut rx) = match watch_file(&self.path) {
            Ok(w) => w,
            Err(error) => {
                log::warn!("settings: not watching {}: {error:#}", self.path.display());
                return;
            }
        };
        self._watcher = Some(watcher);
        self.watch = Some(cx.spawn(async move |this, cx| {
            while rx.next().await.is_some() {
                cx.background_executor().timer(RELOAD_DELAY).await;
                while rx.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    /// The current value.
    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// Why the last reload was rejected, if it was.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Re-reads the file. Invalid JSON keeps the last good value and records the error.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let loaded = Settings::load(&self.path, &self.settings);
        self.error = loaded.error;
        if self.error.is_some() {
            log::warn!("settings: {}", self.error.as_deref().unwrap_or_default());
            cx.notify();
        } else if loaded.settings != self.settings {
            self.settings = loaded.settings;
            cx.emit(SettingsChanged);
            cx.notify();
        }
    }

    /// Applies an edit, notifies, and schedules a debounced save.
    pub fn update(&mut self, edit: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let before = self.settings.clone();
        edit(&mut self.settings);
        if self.settings == before {
            return;
        }
        cx.emit(SettingsChanged);
        cx.notify();
        self.schedule_save(cx);
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            this.update(cx, |this, _| this.save_now()).ok();
        }));
    }

    /// Writes immediately (atomic), e.g. at shutdown.
    pub fn save_now(&mut self) {
        self.save = None;
        if let Err(error) = self.settings.save(&self.path) {
            log::error!("settings: cannot save {}: {error:#}", self.path.display());
        }
    }

    /// Builds the spec for a new download from the current settings.
    pub fn job_spec(&self, url: &str, preset: Preset, format_spec: &str) -> JobSpec {
        let s = &self.settings;
        JobSpec {
            url: url.to_string(),
            preset,
            format_spec: format_spec.to_string(),
            output_dir: s.output_dir.clone(),
            outtmpl: s.outtmpl.clone(),
            playlist_index: None,
            embed_thumbnail: s.embed_thumbnail,
            embed_metadata: s.embed_metadata,
            embed_chapters: s.embed_chapters,
            subtitles: s.subtitles.clone(),
            sponsorblock: s.sponsorblock.clone(),
            cookies: s.cookies.clone(),
            proxy: s.proxy.clone(),
            rate_limit: s.rate_limit.clone(),
            retries: None,
            ffmpeg_location: s.tool_paths.ffmpeg.clone(),
            js_runtime: s.tool_paths.deno.clone().map(|p| ("deno".to_string(), p)),
            extra_args: vec![],
        }
    }
}

/// Watches the directory of `path` and signals on every event that touches the file itself.
fn watch_file(
    path: &std::path::Path,
) -> anyhow::Result<(
    notify::RecommendedWatcher,
    futures::channel::mpsc::UnboundedReceiver<()>,
)> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let (tx, rx) = futures::channel::mpsc::unbounded::<()>();
    let name = path.file_name().map(|n| n.to_os_string());
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && event
                .paths
                .iter()
                .any(|p| p.file_name().map(|n| n.to_os_string()) == name)
        {
            tx.unbounded_send(()).ok();
        }
    })?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    Ok((watcher, rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_signals_an_edit_within_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let (_watcher, mut rx) = watch_file(&path).unwrap();
        std::fs::write(&path, "{}").unwrap();
        let hit = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(10));
            rx.try_recv().is_ok()
        });
        assert!(hit, "no event for settings.json within 1 s");
    }
}
