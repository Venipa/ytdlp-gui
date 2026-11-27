//! settings.json (architecture section 12). serde only; the gpui hot-reload watcher lives in `state`.
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Subtitle options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleSettings {
    pub enabled: bool,
    pub langs: Vec<String>,
    pub embed: bool,
}
impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            langs: vec!["en".into()],
            embed: false,
        }
    }
}

/// `off`, `mark` or `remove`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SponsorBlockMode {
    #[default]
    Off,
    Mark,
    Remove,
}

/// SponsorBlock options.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SponsorBlockSettings {
    pub mode: SponsorBlockMode,
    pub categories: Vec<String>,
}

/// `none`, `file` or `browser`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CookiesMode {
    #[default]
    None,
    File,
    Browser,
}

/// Cookie source (the cookie file itself is copied to `data/cookies/`, never stored here).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CookiesSettings {
    pub mode: CookiesMode,
    pub file: Option<String>,
    pub browser: Option<String>,
    /// The cookie manager's sets, highest priority first. When any is on, each download gets only
    /// the cookies for its site ([`crate::cookies::select`]) and `mode` is not used.
    pub sets: Vec<CookieSet>,
}

/// Where a cookie set came from.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CookieSource {
    #[default]
    File,
    Paste,
    /// A copy read from this browser (yt-dlp's name: firefox, chrome, safari, ...).
    Browser(String),
}

/// One cookie set; its text lives in `<data>/cookies/sets/<id>.txt` (owner-only).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CookieSet {
    pub id: String,
    pub name: String,
    pub source: CookieSource,
    /// When the text was last imported, pasted, edited or read (unix ms).
    pub updated_ms: i64,
    pub on: bool,
    /// Cookie domains switched off in this set.
    pub off_domains: Vec<String>,
}

/// User-chosen tool binaries.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPaths {
    pub ffmpeg: Option<String>,
    pub ffprobe: Option<String>,
    pub deno: Option<String>,
    pub aria2c: Option<String>,
}

/// Which GitHub releases the app updates to: `stable` (latest release) or `beta` (pre-releases too).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

/// `automatic` downloads a found update in the background; `manual` asks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMode {
    #[default]
    Automatic,
    Manual,
}

/// Remembered window geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub w: f32,
    pub h: f32,
    pub x: Option<f32>,
    pub y: Option<f32>,
}

/// Contents of settings.json. Unknown keys are ignored, missing keys take defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `system`, `dark`, `light` or a bundled theme file id such as `catppuccin/mocha`.
    pub theme: String,
    pub language: String,
    /// `solid`, `translucent` or `blur`.
    pub background: String,
    /// How see-through a non-solid window is, 0 (opaque) to 1 (clear).
    pub transparency: f32,
    /// Whether menus, dialogs and floating controls are drawn as frosted glass.
    pub blur_ui: bool,
    pub compact_mode: bool,
    /// Whether download rows show a thumbnail (ledger rows) instead of the compact style.
    pub show_thumbnails: bool,
    pub font_size: f32,
    pub output_dir: String,
    pub outtmpl: String,
    pub playlist_outtmpl: String,
    pub concurrent_global: u32,
    pub concurrent_per_domain: u32,
    pub max_attempts: u32,
    pub resume_on_start: bool,
    pub last_preset: String,
    pub embed_thumbnail: bool,
    pub embed_metadata: bool,
    pub embed_chapters: bool,
    pub subtitles: SubtitleSettings,
    pub sponsorblock: SponsorBlockSettings,
    pub proxy: Option<String>,
    pub rate_limit: Option<String>,
    pub cookies: CookiesSettings,
    pub tool_paths: ToolPaths,
    /// Hand downloads to aria2c (when one is found) instead of yt-dlp's own downloader.
    pub aria2c: bool,
    /// Whether the custom argument lines below reach downloads.
    pub custom_args: bool,
    /// Custom yt-dlp arguments, one option (with its value) per line, as on the command line.
    pub ytdlp_args: Vec<String>,
    /// Custom aria2c arguments, one per line; used only while aria2c is on.
    pub aria2c_args: Vec<String>,
    /// `gpl` or `lgpl`.
    pub ffmpeg_flavor: String,
    pub ytdlp_auto_check: bool,
    pub clipboard_watch: bool,
    pub clipboard_auto_add: bool,
    /// Site rules, first match wins (Settings › Site rules). `clipboard_auto_add`, `last_preset` and
    /// `default_fix` are the "Everything else" rule.
    pub site_rules: Vec<crate::SiteRule>,
    /// The fix run on downloads no rule matches.
    pub default_fix: Option<crate::Fix>,
    /// Video encoder for fixes that convert: `auto` (hardware when ffmpeg has it), `hardware`, `software`.
    pub encoder: String,
    pub notifications: bool,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub window: Option<WindowState>,
    pub detail_panel_width: f32,
    pub update_channel: UpdateChannel,
    pub update_mode: UpdateMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            language: "en-US".into(),
            background: "solid".into(),
            transparency: 0.15,
            blur_ui: false,
            compact_mode: false,
            show_thumbnails: false,
            font_size: 12.0,
            output_dir: dirs::video_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_default()
                .join("ytdlpd")
                .to_string_lossy()
                .into_owned(),
            outtmpl: "%(title).180B [%(id)s].%(ext)s".into(),
            playlist_outtmpl:
                "%(playlist_title).80B/%(playlist_index)03d %(title).160B [%(id)s].%(ext)s".into(),
            concurrent_global: 3,
            concurrent_per_domain: 2,
            max_attempts: 5,
            resume_on_start: false,
            last_preset: "video:best".into(),
            embed_thumbnail: false,
            embed_metadata: true,
            embed_chapters: true,
            subtitles: Default::default(),
            sponsorblock: Default::default(),
            proxy: None,
            rate_limit: None,
            cookies: Default::default(),
            tool_paths: Default::default(),
            aria2c: false,
            custom_args: false,
            ytdlp_args: vec![],
            aria2c_args: vec![],
            ffmpeg_flavor: "gpl".into(),
            ytdlp_auto_check: true,
            clipboard_watch: true,
            clipboard_auto_add: false,
            site_rules: vec![],
            default_fix: None,
            encoder: "auto".into(),
            notifications: true,
            close_to_tray: false,
            start_minimized: false,
            window: None,
            detail_panel_width: 360.0,
            update_channel: UpdateChannel::Stable,
            update_mode: UpdateMode::Automatic,
        }
    }
}

/// Outcome of a load: the settings to use plus the parse error that made us keep the old ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    pub error: Option<String>,
}

impl Settings {
    /// Loads `path`. Missing file gives defaults; invalid JSON keeps `last_good` and reports the error.
    pub fn load(path: &Path, last_good: &Settings) -> Loaded {
        match std::fs::read_to_string(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded {
                settings: Settings::default(),
                error: None,
            },
            Err(e) => Loaded {
                settings: last_good.clone(),
                error: Some(format!("cannot read {}: {e}", path.display())),
            },
            Ok(text) => match serde_json::from_str(&text) {
                Ok(settings) => Loaded {
                    settings,
                    error: None,
                },
                Err(e) => Loaded {
                    settings: last_good.clone(),
                    error: Some(format!("invalid settings.json: {e}")),
                },
            },
        }
    }

    /// Atomic save: write a sibling temp file, then rename over the target.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_round_trip() {
        let d = Settings::default();
        assert_eq!(
            (d.concurrent_global, d.concurrent_per_domain, d.max_attempts),
            (3, 2, 5)
        );
        assert!(d.output_dir.ends_with("ytdlpd") && d.embed_metadata && !d.embed_thumbnail);
        let dir = std::env::temp_dir().join(format!("ytdlpd-settings-{}", std::process::id()));
        let p = dir.join("settings.json");
        let mut s = Settings {
            theme: "light".into(),
            proxy: Some("http://p:1".into()),
            ..d.clone()
        };
        s.window = Some(WindowState {
            w: 1.0,
            h: 2.0,
            x: None,
            y: Some(3.0),
        });
        s.sponsorblock.mode = SponsorBlockMode::Remove;
        s.save(&p).unwrap();
        assert!(!p.with_extension("json.tmp").exists());
        assert_eq!(
            Settings::load(&p, &d),
            Loaded {
                settings: s.clone(),
                error: None
            }
        );

        // invalid JSON keeps the last good value; partial JSON fills defaults
        std::fs::write(&p, "{ nope").unwrap();
        let l = Settings::load(&p, &s);
        assert_eq!(l.settings, s);
        assert!(l.error.is_some());
        std::fs::write(&p, r#"{"theme":"x","unknown":1}"#).unwrap();
        let l = Settings::load(&p, &s);
        assert_eq!(
            (l.settings.theme.as_str(), l.settings.font_size, l.error),
            ("x", 12.0, None)
        );
        assert_eq!(Settings::load(&dir.join("missing.json"), &s).settings, d);
        std::fs::remove_dir_all(dir).ok();
    }
}
