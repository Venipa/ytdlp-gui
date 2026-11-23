//! Engine services the views need but `state` must not link (the engine is an optional
//! dependency of the binary): the binary registers them as a global, views read them.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use domain::JobSpec;
use gpui::Global;

/// Blocking: the path yt-dlp would write for a sample video under the spec's folder and template.
pub type PreviewFn = Arc<dyn Fn(&JobSpec) -> Result<String, String> + Send + Sync>;
/// Blocking: stores Netscape cookie text under the data dir (owner-only) and returns the file.
pub type StoreCookiesFn = Arc<dyn Fn(&Path, &str) -> Result<PathBuf, String> + Send + Sync>;

/// Optional engine services; every field is `None` in builds without the engine.
#[derive(Clone, Default)]
pub struct Hooks {
    pub preview: Option<PreviewFn>,
    pub store_cookies: Option<StoreCookiesFn>,
}

impl Global for Hooks {}
