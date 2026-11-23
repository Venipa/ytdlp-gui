//! The A/C contract: what the app calls on the engine and tools crates. Object-safe, no gpui.
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::model::*;
use crate::settings::CookiesSettings;

/// Opaque handle to a running worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobHandle(pub u64);

/// Probe tuning that is not part of a download.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProbeOpts {
    pub timeout_s: Option<u32>,
    /// `true` expands playlists flat (entries only); `false` probes a single item.
    pub flat_playlist: bool,
    pub cookies: Option<CookiesSettings>,
    pub proxy: Option<String>,
    /// `(runtime key, path)`: YouTube needs a JavaScript runtime to unlock its formats; without
    /// one a lookup sees only images and fails.
    #[serde(default)]
    pub js_runtime: Option<(String, String)>,
}

/// Worker events (mirror of the IPC lines in architecture section 4). Progress is latest-wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobEvent {
    Ready,
    Progress {
        done: u64,
        total: Option<u64>,
        speed: Option<f64>,
        eta: Option<f64>,
        frag: Option<(u32, u32)>,
    },
    /// `downloading`, `merging` or `postprocess:<name>`.
    Phase(String),
    /// Final path candidate; the last one wins.
    Path(String),
    /// Partials and intermediates created, needed for cleanup.
    Files(Vec<String>),
    Log {
        level: String,
        message: String,
    },
    /// Success. `info_path` points at a temp file for large (playlist) info dicts.
    Finished {
        info_json: Option<String>,
        info_path: Option<String>,
    },
    Failed(EngineError),
}

/// Engine-facing backend. Pause and cancel are both `kill`; the caller decides on partial cleanup.
/// The stream ends after `Finished`/`Failed` or when the worker dies.
pub trait Backend: Send + Sync {
    /// Extracts metadata without downloading.
    fn probe(
        &self,
        url: String,
        opts: ProbeOpts,
    ) -> BoxFuture<'static, Result<ProbeResult, EngineError>>;
    /// Starts a worker for `spec`.
    fn start(
        &self,
        id: DownloadId,
        spec: JobSpec,
    ) -> BoxFuture<'static, Result<(JobHandle, BoxStream<'static, JobEvent>), EngineError>>;
    /// Kills the worker's process tree.
    fn kill(&self, handle: JobHandle) -> BoxFuture<'static, ()>;
    /// Points new workers (and the first probe) at an updated yt-dlp site dir; `None` goes back to
    /// the bundled one. Running jobs keep what they loaded.
    fn set_site(&self, _site: Option<std::path::PathBuf>) {}
}

/// Progress/outcome of `ToolsApi::ensure`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolEvent {
    Progress { done: u64, total: Option<u64> },
    Done(ToolStatus),
    Failed(EngineError),
}

/// A verified yt-dlp site dir to switch to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteUpdate {
    pub version: String,
    /// The new site dir.
    pub site: String,
    /// The site that was active before, kept for a way back.
    pub previous: Option<String>,
}

/// Tools-facing backend.
pub trait ToolsApi: Send + Sync {
    /// Detects every tool (settings path, PATH, well-known dirs, managed install).
    fn status(&self) -> BoxFuture<'static, Vec<ToolStatus>>;
    /// Downloads/installs `tool` if missing, streaming progress; the stream ends with `Done` or `Failed`.
    fn ensure(&self, tool: ToolKind) -> BoxStream<'static, ToolEvent>;
    /// Downloads and installs `tool` again even when one is detected (Tools page "Re-download").
    /// Defaults to `ensure` for implementations that cannot tell the difference.
    fn reinstall(&self, tool: ToolKind) -> BoxStream<'static, ToolEvent> {
        self.ensure(tool)
    }
    /// The newest version of `tool` newer than `current`, `None` when `current` is the latest or
    /// the tool has no update channel.
    fn check_update(
        &self,
        _tool: ToolKind,
        _current: Option<String>,
    ) -> BoxFuture<'static, Result<Option<String>, EngineError>> {
        Box::pin(async { Ok(None) })
    }
    /// Downloads the newest `tool` (yt-dlp with the yt-dlp-ejs it pins) into a new site dir, runs the
    /// self-test on it and, only when that passes, reports it as ready to switch to. `active` is the
    /// site in use now, kept as the way back. A failure leaves everything as it was.
    fn install_update(
        &self,
        _tool: ToolKind,
        _active: Option<String>,
    ) -> BoxFuture<'static, Result<SiteUpdate, EngineError>> {
        Box::pin(async { Err(EngineError::new(ErrorCode::Unknown, "no update channel")) })
    }
    /// Points `tool` at a user binary; rejects executables that are not that tool.
    fn set_custom_path(
        &self,
        tool: ToolKind,
        path: String,
    ) -> BoxFuture<'static, Result<ToolStatus, EngineError>>;
}
