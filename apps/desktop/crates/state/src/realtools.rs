//! `ToolsApi` over the `tools` crate. Its network code needs tokio, so every call hops onto the Io runtime.
use std::path::PathBuf;
use std::sync::Arc;

use domain::{
    EngineError, ErrorCode, SiteUpdate, ToolEvent, ToolKind, ToolSource, ToolState, ToolStatus,
    ToolsApi,
};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::{FutureExt as _, StreamExt as _};
use tokio::runtime::Handle;

/// Real tool detection and installation (ffmpeg, ffprobe, deno; aria2c is only detected).
pub struct RealTools {
    handle: Handle,
    dirs: Arc<::tools::Dirs>,
}

fn tool_of(kind: ToolKind) -> Option<::tools::Tool> {
    match kind {
        ToolKind::Ffmpeg => Some(::tools::Tool::Ffmpeg),
        ToolKind::Ffprobe => Some(::tools::Tool::Ffprobe),
        ToolKind::Deno => Some(::tools::Tool::Deno),
        ToolKind::Aria2c => Some(::tools::Tool::Aria2c),
        _ => None,
    }
}

fn kind_of(tool: ::tools::Tool) -> ToolKind {
    match tool {
        ::tools::Tool::Ffmpeg => ToolKind::Ffmpeg,
        ::tools::Tool::Ffprobe => ToolKind::Ffprobe,
        ::tools::Tool::Deno => ToolKind::Deno,
        ::tools::Tool::Aria2c => ToolKind::Aria2c,
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

/// Runs `ytdlpd --self-test --site <dir>` in a throwaway process: the new yt-dlp must import from `site`.
// shortcut: no timeout on the child, add one if a hang is ever seen.
fn verify_site(site: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let out = std::process::Command::new(std::env::current_exe().context("cannot find the app")?)
        .arg("--self-test")
        .arg("--site")
        .arg(site)
        .output()
        .context("cannot start the self-test")?;
    anyhow::ensure!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(())
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
            ::tools::pair_ffmpeg(&dirs);
            let mut out = vec![];
            for kind in [
                ToolKind::Ffmpeg,
                ToolKind::Ffprobe,
                ToolKind::Deno,
                ToolKind::Aria2c,
            ] {
                let tool = tool_of(kind).expect("managed kind");
                out.push(match ::tools::detect(tool, &dirs, None).await {
                    Some(s) => {
                        let s = status_of(s);
                        log::info!(
                            "tools: {kind} {} ({})",
                            s.version.as_deref().unwrap_or("?"),
                            s.source.map_or("?", |x| x.as_str())
                        );
                        s
                    }
                    None => {
                        log::info!("tools: {kind} missing");
                        missing(kind)
                    }
                });
            }
            out
        });
        async move {
            job.await.unwrap_or_else(|e| {
                log::error!("tools: detection task failed: {e}");
                vec![]
            })
        }
        .boxed()
    }

    fn ensure(&self, kind: ToolKind) -> BoxStream<'static, ToolEvent> {
        self.install(kind, false)
    }

    fn reinstall(&self, kind: ToolKind) -> BoxStream<'static, ToolEvent> {
        self.install(kind, true)
    }

    fn check_update(
        &self,
        kind: ToolKind,
        current: Option<String>,
    ) -> BoxFuture<'static, Result<Option<String>, EngineError>> {
        let dirs = self.dirs.clone();
        let job = self.handle.spawn(async move {
            match kind {
                ToolKind::YtDlp => ::tools::check_ytdlp_update(&dirs, current.as_deref())
                    .await
                    .inspect(|update| match update {
                        Some(u) => log::info!("tools: yt-dlp {} is available", u.ytdlp.version),
                        None => log::info!("tools: yt-dlp is up to date"),
                    })
                    .inspect_err(|e| log::warn!("tools: cannot check for a yt-dlp update: {e:#}"))
                    .map(|update| update.map(|u| u.ytdlp.version))
                    .map_err(|e| classify(&e)),
                _ => Ok(None),
            }
        });
        async move {
            job.await.unwrap_or_else(|e| {
                log::error!("tools: task failed: {e}");
                Err(EngineError::new(ErrorCode::Unknown, e.to_string()))
            })
        }
        .boxed()
    }

    fn install_update(
        &self,
        kind: ToolKind,
        active: Option<String>,
    ) -> BoxFuture<'static, Result<SiteUpdate, EngineError>> {
        let dirs = self.dirs.clone();
        let job = self.handle.spawn(async move {
            if kind != ToolKind::YtDlp {
                return Err(EngineError::new(
                    ErrorCode::Unknown,
                    format!("{kind} has no update channel"),
                ));
            }
            let update = ::tools::check_ytdlp_update(&dirs, None)
                .await
                .map_err(|e| classify(&e))?
                .ok_or_else(|| EngineError::new(ErrorCode::Unknown, "no update found"))?;
            let version = update.ytdlp.version.clone();
            let active = active.map(PathBuf::from);
            let installed =
                ::tools::install_ytdlp_pair(&dirs, &update, active.as_deref(), verify_site)
                    .await
                    .inspect_err(|e| log::warn!("tools: yt-dlp {version} install failed: {e:#}"))
                    .map_err(|e| classify(&e))?;
            log::info!("tools: installed yt-dlp {version}");
            Ok(SiteUpdate {
                version,
                site: installed.active.to_string_lossy().into_owned(),
                previous: installed.previous.map(|p| p.to_string_lossy().into_owned()),
            })
        });
        async move {
            job.await.unwrap_or_else(|e| {
                log::error!("tools: task failed: {e}");
                Err(EngineError::new(ErrorCode::Unknown, e.to_string()))
            })
        }
        .boxed()
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
                _ => {
                    log::warn!(
                        "tools: custom {kind} rejected: {} is not {kind}",
                        path.file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                    );
                    Err(EngineError::new(
                        ErrorCode::Unknown,
                        format!("{} is not {kind}", path.display()),
                    ))
                }
            }
        });
        async move {
            job.await.unwrap_or_else(|e| {
                log::error!("tools: task failed: {e}");
                Err(EngineError::new(ErrorCode::Unknown, e.to_string()))
            })
        }
        .boxed()
    }
}

impl RealTools {
    fn install(&self, kind: ToolKind, force: bool) -> BoxStream<'static, ToolEvent> {
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
            let done = match force {
                true => ::tools::reinstall(tool, &dirs, &progress).await,
                false => ::tools::ensure(tool, &dirs, None, &progress).await,
            };
            let event = match done {
                Ok(s) => {
                    log::info!("tools: installed {kind} {}", s.version);
                    ToolEvent::Done(status_of(s))
                }
                Err(error) => {
                    log::warn!("tools: cannot install {kind}: {error:#}");
                    ToolEvent::Failed(classify(&error))
                }
            };
            tx.unbounded_send(event).ok();
        });
        rx.boxed()
    }
}
