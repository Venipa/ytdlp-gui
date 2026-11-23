//! `domain::Backend` over the real engine: probes in-process, one worker process per download.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use domain::{
    Backend, DownloadId, EngineError, ErrorCode, JobEvent, JobHandle, JobSpec, ProbeOpts,
    ProbeResult,
};
use futures::{
    FutureExt, StreamExt,
    channel::{mpsc, oneshot},
    future::BoxFuture,
    stream::BoxStream,
};

use crate::{
    host::{WorkerConfig, WorkerHandle, spawn_worker_with},
    ipc::Event,
    probe, runtime,
    spec::{Env, probe_opts},
};

/// Default probe timeout (the UI uses 45 s).
const PROBE_TIMEOUT_S: u32 = 45;

/// Where things are on this machine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// The app exe, re-run with `--engine-worker`.
    pub worker_exe: PathBuf,
    /// Python home; `None` = exe-relative, then the dev runtime.
    pub python_home: Option<PathBuf>,
    /// Runtime-updated yt-dlp + ejs dir.
    pub site_updates: Option<PathBuf>,
    /// Cache dir and fallback JS runtime for every job and probe.
    pub env: Env,
}

/// The real backend.
pub struct EngineBackend {
    cfg: Arc<EngineConfig>,
    /// The yt-dlp site new workers start with; starts as `cfg.site_updates`, moved by `set_site`.
    site: Arc<std::sync::RwLock<Option<PathBuf>>>,
    next: AtomicU64,
    jobs: Arc<Mutex<HashMap<u64, WorkerHandle>>>,
}

impl EngineBackend {
    /// Build a backend; nothing starts until the first probe or job.
    pub fn new(cfg: EngineConfig) -> Self {
        Self {
            site: Arc::new(std::sync::RwLock::new(cfg.site_updates.clone())),
            cfg: Arc::new(cfg),
            next: AtomicU64::new(1),
            jobs: Default::default(),
        }
    }
}

/// Wire event to contract events (`Result` becomes `Path` + `Finished`).
pub fn map_event(ev: Event) -> Vec<JobEvent> {
    match ev {
        Event::Ready => vec![JobEvent::Ready],
        Event::Progress {
            done,
            total,
            speed,
            eta,
            frag,
        } => vec![JobEvent::Progress {
            done,
            total,
            speed,
            eta: eta.map(|e| e as f64),
            frag,
        }],
        Event::Phase { name } => vec![JobEvent::Phase(name)],
        Event::Path { path } => vec![JobEvent::Path(path)],
        Event::Files { paths } => vec![JobEvent::Files(paths)],
        Event::Log { level, msg } => vec![JobEvent::Log {
            level,
            message: msg,
        }],
        Event::Result { path, info, .. } => path
            .map(JobEvent::Path)
            .into_iter()
            .chain([JobEvent::Finished {
                info_json: Some(info.to_string()),
                info_path: None,
            }])
            .collect(),
        Event::Error {
            code,
            message,
            retryable,
            retry_after,
        } => vec![JobEvent::Failed(EngineError {
            code,
            message,
            retryable,
            retry_after,
        })],
    }
}

impl Backend for EngineBackend {
    fn probe(
        &self,
        url: String,
        opts: ProbeOpts,
    ) -> BoxFuture<'static, Result<ProbeResult, EngineError>> {
        let cfg = self.cfg.clone();
        let site = self.site.read().map(|s| s.clone()).unwrap_or_default();
        async move {
            let (tx, rx) = oneshot::channel();
            std::thread::spawn(move || {
                let r = runtime::init(runtime::InitOptions {
                    home: cfg.python_home.clone(),
                    site_updates: site,
                    sink: runtime::log_sink,
                })
                .map_err(|e| EngineError::new(ErrorCode::Unknown, e.to_string()))
                .and_then(|_| {
                    let t = Duration::from_secs(opts.timeout_s.unwrap_or(PROBE_TIMEOUT_S).into());
                    probe::probe(&url, &probe_opts(&opts, &cfg.env), t)
                });
                let _ = tx.send(r);
            });
            rx.await
                .unwrap_or_else(|_| Err(EngineError::new(ErrorCode::Unknown, "probe thread died")))
        }
        .boxed()
    }

    fn start(
        &self,
        id: DownloadId,
        spec: JobSpec,
    ) -> BoxFuture<'static, Result<(JobHandle, BoxStream<'static, JobEvent>), EngineError>> {
        let (cfg, jobs) = (self.cfg.clone(), self.jobs.clone());
        let site = self.site.read().map(|s| s.clone()).unwrap_or_default();
        let handle = self.next.fetch_add(1, Ordering::Relaxed);
        async move {
            let wc = WorkerConfig {
                python_home: cfg.python_home.clone(),
                site_updates: site,
                env: vec![],
            };
            let mut w = spawn_worker_with(&cfg.worker_exe, &spec, &cfg.env, &wc).map_err(|e| {
                EngineError::new(ErrorCode::Unknown, format!("cannot start worker: {e}"))
            })?;
            log::info!(
                "engine: job {} worker pid {} (handle {handle})",
                id.0,
                w.pid()
            );
            let events = w.take_events();
            jobs.lock().unwrap().insert(handle, w);
            let (tx, rx) = mpsc::unbounded();
            std::thread::spawn(move || {
                let mut terminal = false;
                for ev in events {
                    terminal |= ev.is_terminal();
                    for e in map_event(ev) {
                        if tx.unbounded_send(e).is_err() {
                            return;
                        }
                    }
                }
                // Pipes closed. If kill() did not take the worker, it ended on its own: reap it, and
                // report a death without a terminal event.
                let w = jobs.lock().unwrap().remove(&handle);
                if let Some(mut w) = w {
                    let status = w.wait();
                    if !terminal {
                        let code = status.ok().and_then(|s| s.code());
                        let _ = tx.unbounded_send(JobEvent::Failed(EngineError::new(
                            ErrorCode::Unknown,
                            format!("worker exited unexpectedly (code {code:?})"),
                        )));
                    }
                }
            });
            Ok((JobHandle(handle), rx.boxed()))
        }
        .boxed()
    }

    fn set_site(&self, site: Option<PathBuf>) {
        runtime::set_site(site.clone());
        if let Ok(mut current) = self.site.write() {
            *current = site;
        }
    }

    fn kill(&self, handle: JobHandle) -> BoxFuture<'static, ()> {
        let jobs = self.jobs.clone();
        async move {
            let w = jobs.lock().unwrap().remove(&handle.0);
            if let Some(mut w) = w {
                w.kill();
            }
        }
        .boxed()
    }
}
