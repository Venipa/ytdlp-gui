//! `ToolsApi` over the `tools` crate. Its network code needs tokio, so every call hops onto the Io runtime.
use std::path::PathBuf;
use std::sync::Arc;

use domain::{
    EngineError, ErrorCode, ToolEvent, ToolKind, ToolSource, ToolState, ToolStatus, ToolsApi,
};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::{FutureExt as _, StreamExt as _};
use tokio::runtime::Handle;

/// Real tool detection and installation (ffmpeg, ffprobe, deno).
pub struct RealTools {
    handle: Handle,
    dirs: Arc<::tools::Dirs>,
}

fn tool_of(kind: ToolKind) -> Option<::tools::Tool> {
    match kind {
        ToolKind::Ffmpeg => Some(::tools::Tool::Ffmpeg),
        ToolKind::Ffprobe => Some(::tools::Tool::Ffprobe),
        ToolKind::Deno => Some(::tools::Tool::Deno),
        _ => None,
    }
}

fn kind_of(tool: ::tools::Tool) -> ToolKind {
    match tool {
        ::tools::Tool::Ffmpeg => ToolKind::Ffmpeg,
        ::tools::Tool::Ffprobe => ToolKind::Ffprobe,
        ::tools::Tool::Deno => ToolKind::Deno,
    }
}

fn status_of(s: ::tools::ToolStatus) -> ToolStatus {
    ToolStatus {
        kind: kind_of(s.tool),
        state: ToolState::Ready,
        source: Some(match s.source {
            ::tools::Source::Managed => ToolSource::Managed,
            ::tools::Source::Path => ToolSource::Path,
            ::tools::Source::Custom => ToolSource::Custom,
        }),
        path: Some(s.path.to_string_lossy().into_owned()),
        version: Some(s.version),
    }
}

fn missing(kind: ToolKind) -> ToolStatus {
    ToolStatus {
        kind,
        state: ToolState::Missing,
        source: None,
        path: None,
        version: None,
    }
}

/// Transient network trouble retries; a checksum problem fails closed and never retries.
fn classify(error: &anyhow::Error) -> EngineError {
    let text = format!("{error:#}");
    let code = if text.to_lowercase().contains("checksum") {
        ErrorCode::Unknown
    } else {
        ErrorCode::Network
    };
    EngineError::new(code, text)
}

impl RealTools {
    /// `handle` is the Io runtime.
    pub fn new(handle: Handle, dirs: ::tools::Dirs) -> Self {
        Self {
            handle,
            dirs: Arc::new(dirs),
        }
    }
}

impl ToolsApi for RealTools {
    fn status(&self) -> BoxFuture<'static, Vec<ToolStatus>> {
        let dirs = self.dirs.clone();
        let job = self.handle.spawn(async move {
            let mut out = vec![];
            for kind in [ToolKind::Ffmpeg, ToolKind::Ffprobe, ToolKind::Deno] {
                let tool = tool_of(kind).expect("managed kind");
                out.push(match ::tools::detect(tool, &dirs, None).await {
                    Some(s) => status_of(s),
                    None => missing(kind),
                });
            }
            out
        });
        async move { job.await.unwrap_or_default() }.boxed()
    }

    fn ensure(&self, kind: ToolKind) -> BoxStream<'static, ToolEvent> {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let dirs = self.dirs.clone();
        self.handle.spawn(async move {
            let Some(tool) = tool_of(kind) else {
                tx.unbounded_send(ToolEvent::Failed(EngineError::new(
                    ErrorCode::Unknown,
                    format!("{kind} is not managed"),
                )))
                .ok();
                return;
            };
            let progress_tx = tx.clone();
            let progress = move |done: u64, total: Option<u64>| {
                progress_tx
                    .unbounded_send(ToolEvent::Progress { done, total })
                    .ok();
            };
            let event = match ::tools::ensure(tool, &dirs, None, &progress).await {
                Ok(s) => ToolEvent::Done(status_of(s)),
                Err(error) => ToolEvent::Failed(classify(&error)),
            };
            tx.unbounded_send(event).ok();
        });
        rx.boxed()
    }

    fn set_custom_path(
        &self,
        kind: ToolKind,
        path: String,
    ) -> BoxFuture<'static, Result<ToolStatus, EngineError>> {
        let dirs = self.dirs.clone();
        let job = self.handle.spawn(async move {
            let tool = tool_of(kind).ok_or_else(|| {
                EngineError::new(ErrorCode::Unknown, format!("{kind} is not managed"))
            })?;
            let path = PathBuf::from(path);
            match ::tools::detect(tool, &dirs, Some(&path)).await {
                Some(s) if s.source == ::tools::Source::Custom => Ok(status_of(s)),
                _ => Err(EngineError::new(
                    ErrorCode::Unknown,
                    format!("{} is not {kind}", path.display()),
                )),
            }
        });
        async move {
            job.await
                .unwrap_or_else(|e| Err(EngineError::new(ErrorCode::Unknown, e.to_string())))
        }
        .boxed()
    }
}
