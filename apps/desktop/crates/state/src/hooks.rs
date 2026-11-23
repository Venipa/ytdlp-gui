//! Engine services the views need but `state` must not link (the engine is an optional
//! dependency of the binary): the binary registers them as a global, views read them.
use std::sync::Arc;

use domain::JobSpec;
use gpui::Global;

/// Blocking: the path yt-dlp would write for a sample video under the spec's folder and template.
pub type PreviewFn = Arc<dyn Fn(&JobSpec) -> Result<String, String> + Send + Sync>;
/// Blocking: reads a browser's cookies (yt-dlp key: firefox, chrome, safari, ...) as cookies.txt text.
pub type BrowserCookiesFn = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

/// Blocking: the version of the yt-dlp the engine runs, when the engine is in this build.
pub type VersionFn = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Optional engine services; every field is `None` in builds without the engine.
#[derive(Clone, Default)]
pub struct Hooks {
    pub preview: Option<PreviewFn>,
    pub browser_cookies: Option<BrowserCookiesFn>,
    pub yt_dlp_version: Option<VersionFn>,
}

impl Global for Hooks {}
