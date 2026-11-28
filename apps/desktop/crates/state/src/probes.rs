//! `Probes`: link previews, up to three at once, results keyed by URL.
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use domain::{Backend, EngineError, ErrorCode, ProbeOpts, ProbeResult};
use futures::FutureExt as _;
use gpui::{Context, Task};

use crate::cookies::CookieFiles;

/// Parallel probes.
const PARALLEL: usize = 3;
/// A stuck probe is abandoned (dropped, not killed) after this long.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(45);

/// Where a URL's probe stands.
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeState {
    /// Waiting for a free slot or running.
    Loading,
    Ready(Arc<ProbeResult>),
    Failed(EngineError),
}

/// Probe cache and queue.
pub struct Probes {
    backend: Arc<dyn Backend>,
    states: HashMap<String, ProbeState>,
    waiting: VecDeque<(String, ProbeOpts)>,
    running: HashMap<String, Task<()>>,
    /// Probes that hit the timeout (logged; the interpreter is not told).
    abandoned: usize,
    timeout: Duration,
    /// Set by [`Probes::with_cookies`]: the sets in `opts.cookies` are matched per link.
    cookie_files: Option<CookieFiles>,
    runs: u64,
}

impl Probes {
    /// Creates the entity state.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            states: HashMap::new(),
            waiting: VecDeque::new(),
            running: HashMap::new(),
            abandoned: 0,
            timeout: PROBE_TIMEOUT,
            cookie_files: None,
            runs: 0,
        }
    }

    /// Probes with only the cookies the sets in `ProbeOpts::cookies` have for each link.
    pub fn with_cookies(mut self, files: CookieFiles) -> Self {
        self.cookie_files = Some(files);
        self
    }

    /// Overrides the timeout (tests).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// State for `url`.
    pub fn get(&self, url: &str) -> Option<&ProbeState> {
        self.states.get(url)
    }

    /// Probes abandoned after a timeout so far.
    pub fn abandoned(&self) -> usize {
        self.abandoned
    }

    /// Starts (or re-starts after a failure) a probe; a ready or loading URL is left alone.
    pub fn request(&mut self, url: &str, opts: ProbeOpts, cx: &mut Context<Self>) {
        if matches!(
            self.states.get(url),
            Some(ProbeState::Loading | ProbeState::Ready(_))
        ) {
            return;
        }
        self.states.insert(url.to_string(), ProbeState::Loading);
        self.waiting.push_back((url.to_string(), opts));
        self.pump(cx);
        cx.notify();
    }

    /// Forgets a URL; a running probe is abandoned.
    pub fn remove(&mut self, url: &str, cx: &mut Context<Self>) {
        self.states.remove(url);
        self.waiting.retain(|(u, _)| u != url);
        if self.running.remove(url).is_some() {
            self.pump(cx);
        }
        cx.notify();
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        while self.running.len() < PARALLEL {
            let Some((url, mut opts)) = self.waiting.pop_front() else {
                break;
            };
            let backend = self.backend.clone();
            let files = self.cookie_files.clone();
            self.runs += 1;
            let run = format!("probe-{}", self.runs);
            let timeout = self.timeout;
            let (key, link) = (url.clone(), url.clone());
            let task = cx.spawn(async move |this, cx| {
                // held until the probe ends: dropping it removes the cookie file
                let _run_file = match (files, opts.cookies.take()) {
                    (Some(files), Some(c)) => {
                        let link = link.clone();
                        let picked = cx
                            .background_executor()
                            .spawn(async move { files.for_run(&c.sets, &link, &run) })
                            .await;
                        picked.map(|(file, _)| {
                            opts.cookies = Some(domain::CookiesSettings {
                                mode: domain::CookiesMode::File,
                                file: Some(file.path().to_string_lossy().into_owned()),
                                ..Default::default()
                            });
                            file
                        })
                    }
                    _ => None,
                };
                let probe = backend.probe(link, opts);
                let timer = cx.background_executor().timer(timeout);
                let outcome = futures::select! {
                    result = probe.fuse() => Some(result),
                    _ = futures::FutureExt::fuse(timer) => None,
                };
                this.update(cx, |this, cx| this.finished(&key, outcome, cx))
                    .ok();
            });
            self.running.insert(url, task);
        }
    }

    fn finished(
        &mut self,
        url: &str,
        outcome: Option<Result<ProbeResult, EngineError>>,
        cx: &mut Context<Self>,
    ) {
        if self.running.remove(url).is_none() {
            return; // removed meanwhile
        }
        let state = match outcome {
            Some(Ok(result)) => ProbeState::Ready(Arc::new(result)),
            Some(Err(error)) => {
                log::warn!("probes: {url}: {} {}", error.code, error.message);
                ProbeState::Failed(error)
            }
            None => {
                self.abandoned += 1;
                log::warn!("probes: abandoned a probe after {:?}: {url}", self.timeout);
                ProbeState::Failed(EngineError::new(
                    ErrorCode::Network,
                    "the link preview timed out",
                ))
            }
        };
        self.states.insert(url.to_string(), state);
        self.pump(cx);
        cx.notify();
    }
}
