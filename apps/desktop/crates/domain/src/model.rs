use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::settings::{CookiesSettings, SponsorBlockSettings, SubtitleSettings};

/// Row id of a download; also the call number shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadId(pub i64);

impl fmt::Display for DownloadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Declares a string-backed enum with `as_str`, `FromStr`, `Display`, serde as that string.
macro_rules! str_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $s)] $var),+ }
        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// Stable wire / database string.
            pub fn as_str(self) -> &'static str { match self { $($name::$var => $s),+ } }
        }
        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, String> {
                match s { $($s => Ok($name::$var),)+ _ => Err(format!("unknown {}: {s}", stringify!($name))) }
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) }
        }
    };
}

str_enum! {
    /// Job lifecycle state (see `state::transition`).
    DownloadStatus {
        Queued = "queued", Probing = "probing", Downloading = "downloading", Processing = "processing",
        Paused = "paused", Completed = "completed", Failed = "failed", Cancelled = "cancelled",
    }
}

impl DownloadStatus {
    /// A worker is (or should be) running for this status.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Probing | Self::Downloading | Self::Processing)
    }
    /// No further transitions except Retry.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

str_enum! {
    /// Media kind of a job.
    MediaKind { Video = "video", Audio = "audio" }
}

str_enum! {
    /// Stable error classification (architecture section 6).
    ErrorCode {
        Private = "private", Geo = "geo", Login = "login", AgeRestricted = "age_restricted",
        Unavailable = "unavailable", UnsupportedUrl = "unsupported_url", FormatUnavailable = "format_unavailable",
        RateLimited = "rate_limited", Network = "network", Ssl = "ssl", Ffmpeg = "ffmpeg",
        JsRuntimeMissing = "js_runtime_missing", DiskFull = "disk_full", Permission = "permission",
        CookiesLocked = "cookies_locked", Cancelled = "cancelled", Unknown = "unknown",
    }
}

impl ErrorCode {
    /// Only transient failures retry automatically.
    pub fn retryable(self) -> bool {
        matches!(self, Self::Network | Self::RateLimited)
    }

    /// Fluent message id for the user-facing copy: `err-` plus the code with `_` as `-`
    /// (`age_restricted` is `err-age-restricted`).
    pub fn message_key(self) -> String {
        format!("err-{}", self.as_str().replace('_', "-"))
    }
}

/// Classified engine failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    /// Seconds the server asked to wait (`Retry-After` of a 429).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

impl EngineError {
    /// Builds an error with `retryable` derived from the code.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code.retryable(),
            retry_after: None,
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for EngineError {}

str_enum! {
    /// Video height cap for a preset; `Best` has no cap.
    VideoQuality { Best = "best", P2160 = "2160", P1440 = "1440", P1080 = "1080", P720 = "720", P480 = "480" }
}

str_enum! {
    /// Audio target; `Best` keeps the native stream.
    AudioFormat { Best = "best", Mp3 = "mp3", M4a = "m4a", Opus = "opus" }
}

/// What the user picked in the preset menu. String form: `video:1080`, `audio:mp3`, `custom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Preset {
    Video(VideoQuality),
    Audio(AudioFormat),
    /// Explicit format string chosen in "Choose formats".
    Custom,
}

impl Preset {
    /// The default preset.
    pub const DEFAULT: Preset = Preset::Video(VideoQuality::Best);
    /// Kind a job with this preset stores in `downloads.kind` (custom is video).
    pub fn kind(self) -> MediaKind {
        match self {
            Preset::Audio(_) => MediaKind::Audio,
            _ => MediaKind::Video,
        }
    }
    /// The ten selectable presets (video best/2160/1440/1080/720/480, audio best/mp3/m4a/opus).
    pub fn all() -> Vec<Preset> {
        VideoQuality::ALL
            .iter()
            .map(|q| Preset::Video(*q))
            .chain(AudioFormat::ALL.iter().map(|a| Preset::Audio(*a)))
            .collect()
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Preset::Video(q) => write!(f, "video:{q}"),
            Preset::Audio(a) => write!(f, "audio:{a}"),
            Preset::Custom => f.write_str("custom"),
        }
    }
}
impl FromStr for Preset {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.split_once(':') {
            None if s == "custom" => Ok(Preset::Custom),
            Some(("video", q)) => q.parse().map(Preset::Video),
            Some(("audio", a)) => a.parse().map(Preset::Audio),
            _ => Err(format!("unknown preset: {s}")),
        }
    }
}
impl From<Preset> for String {
    fn from(p: Preset) -> String {
        p.to_string()
    }
}
impl TryFrom<String> for Preset {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

/// Everything a worker needs to run one download; stored verbatim as `downloads.options_json`
/// so Retry reproduces it. Extra args stay a `Vec<String>` (never split on spaces).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSpec {
    pub url: String,
    pub preset: Preset,
    /// yt-dlp format string (for custom: the exact string from Choose formats).
    pub format_spec: String,
    pub output_dir: String,
    pub outtmpl: String,
    pub playlist_index: Option<u32>,
    pub embed_thumbnail: bool,
    pub embed_metadata: bool,
    pub embed_chapters: bool,
    pub subtitles: SubtitleSettings,
    pub sponsorblock: SponsorBlockSettings,
    pub cookies: CookiesSettings,
    pub proxy: Option<String>,
    pub rate_limit: Option<String>,
    pub retries: Option<u32>,
    pub ffmpeg_location: Option<String>,
    /// `(runtime key: deno|node|bun|quickjs, absolute path)`.
    pub js_runtime: Option<(String, String)>,
    pub extra_args: Vec<String>,
    /// The ffmpeg fix run on the finished file (site rule or default); older specs have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<crate::Fix>,
}

/// List row; never carries `options_json` or `info_json`. `title_lc`/`uploader_lc` are precomputed for filtering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadSummary {
    pub id: DownloadId,
    pub url: String,
    pub domain: String,
    pub extractor: Option<String>,
    pub video_id: Option<String>,
    pub title: Option<String>,
    pub uploader: Option<String>,
    pub title_lc: String,
    pub uploader_lc: String,
    pub duration_s: Option<f64>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_path: Option<String>,
    pub kind: MediaKind,
    pub preset: String,
    pub format_label: Option<String>,
    pub status: DownloadStatus,
    pub status_reason: Option<String>,
    pub phase: Option<String>,
    pub progress: f64,
    pub bytes_done: Option<i64>,
    pub bytes_total: Option<i64>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub file_missing: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attempts: u32,
    pub next_attempt_at: Option<i64>,
    pub playlist_id: Option<i64>,
    pub playlist_index: Option<i64>,
    pub position: f64,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub updated_at: i64,
}

impl DownloadSummary {
    /// A fresh queued row (what an enqueue creates before the engine touches it).
    pub fn new(id: DownloadId, url: &str, preset: Preset, now: i64) -> Self {
        Self {
            id,
            url: url.to_string(),
            domain: crate::domain_of(url),
            extractor: None,
            video_id: None,
            title: None,
            uploader: None,
            title_lc: String::new(),
            uploader_lc: String::new(),
            duration_s: None,
            thumbnail_url: None,
            thumbnail_path: None,
            kind: preset.kind(),
            preset: preset.to_string(),
            format_label: None,
            status: DownloadStatus::Queued,
            status_reason: None,
            phase: None,
            progress: 0.0,
            bytes_done: None,
            bytes_total: None,
            file_path: None,
            file_size: None,
            file_missing: false,
            error_code: None,
            error_message: None,
            attempts: 0,
            next_attempt_at: None,
            playlist_id: None,
            playlist_index: None,
            position: id.0 as f64,
            created_at: now,
            started_at: None,
            finished_at: None,
            updated_at: now,
        }
    }

    /// Recomputes the lowercase search fields after `title`/`uploader` change.
    pub fn refresh_lc(&mut self) {
        self.title_lc = self.title.as_deref().unwrap_or_default().to_lowercase();
        self.uploader_lc = self.uploader.as_deref().unwrap_or_default().to_lowercase();
    }
}

/// One selectable stream from a probe.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FormatInfo {
    pub id: String,
    pub ext: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub abr: Option<f64>,
    pub tbr: Option<f64>,
    pub filesize: Option<u64>,
    pub note: Option<String>,
    pub protocol: Option<String>,
}

/// One entry of a probed playlist (flat; formats are probed lazily per entry).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub index: u32,
    pub id: Option<String>,
    pub url: String,
    pub title: Option<String>,
    pub duration_s: Option<f64>,
    pub thumbnail_url: Option<String>,
}

/// Typed result of `extract_info(download=False)`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProbeResult {
    pub url: String,
    pub webpage_url: Option<String>,
    pub extractor: Option<String>,
    pub id: Option<String>,
    pub title: Option<String>,
    pub uploader: Option<String>,
    pub duration_s: Option<f64>,
    pub thumbnail_url: Option<String>,
    /// Empty for playlists.
    pub formats: Vec<FormatInfo>,
    /// `Some` when the URL is a playlist; `title`/`id` then describe the playlist.
    pub entries: Option<Vec<PlaylistEntry>>,
    /// Trimmed info dict (no heatmap/thumbnails, with formats) as stored in `download_meta`.
    pub info_json: Option<String>,
}

impl ProbeResult {
    /// True when the URL expanded to a playlist.
    pub fn is_playlist(&self) -> bool {
        self.entries.is_some()
    }
}

str_enum! {
    /// Managed external tools.
    ToolKind { Ffmpeg = "ffmpeg", Ffprobe = "ffprobe", Deno = "deno", YtDlp = "yt-dlp", YtDlpEjs = "yt-dlp-ejs", Python = "python" }
}

str_enum! {
    /// Where a resolved tool came from (`tools.source`).
    ToolSource { Managed = "managed", Path = "path", Custom = "custom" }
}

/// State of one tool as the Tools page shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolState {
    Checking,
    Missing,
    Ready,
    Downloading { done: u64, total: Option<u64> },
    Failed(EngineError),
}

/// Status of one tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolStatus {
    pub kind: ToolKind,
    pub state: ToolState,
    pub source: Option<ToolSource>,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_round_trip() {
        assert_eq!(Preset::all().len(), 10);
        for p in Preset::all().into_iter().chain([Preset::Custom]) {
            assert_eq!(p.to_string().parse::<Preset>().unwrap(), p);
            let j = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<Preset>(&j).unwrap(), p);
        }
        assert!("video:999".parse::<Preset>().is_err());
        assert_eq!(Preset::Audio(AudioFormat::Mp3).kind(), MediaKind::Audio);
    }

    #[test]
    fn enum_strings_match_db_checks() {
        for s in DownloadStatus::ALL {
            assert_eq!(s.as_str().parse::<DownloadStatus>().unwrap(), *s);
        }
        assert!(ErrorCode::Network.retryable() && !ErrorCode::Login.retryable());
        assert_eq!(ErrorCode::AgeRestricted.message_key(), "err-age-restricted");
        assert_eq!(ErrorCode::Geo.message_key(), "err-geo");
    }
}
