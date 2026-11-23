//! `Tools`: status of every tool and installs with progress, retrying transient failures.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use domain::{EngineError, ErrorCode, ToolEvent, ToolKind, ToolState, ToolStatus, ToolsApi};
use futures::StreamExt as _;
use gpui::{Context, EventEmitter, Task};

/// Attempts per `ensure` (a connection reset was seen live on the first try).
const TRIES: u32 = 3;
/// Base wait between tries (multiplied by the try number).
const RETRY_DELAY: Duration = Duration::from_secs(1);

/// What the Tools page is told after an action it started.
#[derive(Clone, Debug)]
pub enum ToolsEvent {
    /// A check for a newer version ended: the newer version, or `None` when this is the latest.
    UpdateChecked {
        kind: ToolKind,
        latest: Result<Option<String>, EngineError>,
    },
    /// A tool was downloaded again.
    Reinstalled(ToolKind),
}

/// Tool statuses for the Welcome and Tools screens.
pub struct Tools {
    api: Arc<dyn ToolsApi>,
    status: Vec<ToolStatus>,
    detected: bool,
    jobs: HashMap<ToolKind, Task<()>>,
    detect: Option<Task<()>>,
    retry_delay: Duration,
}

impl EventEmitter<ToolsEvent> for Tools {}

impl Tools {
    /// Creates the entity state; call `refresh` to detect.
    pub fn new(api: Arc<dyn ToolsApi>) -> Self {
        Self {
            api,
            status: vec![],
            detected: false,
            jobs: HashMap::new(),
            detect: None,
            retry_delay: RETRY_DELAY,
        }
    }

    /// Overrides the retry wait (tests).
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    /// All known tools.
    pub fn all(&self) -> &[ToolStatus] {
        &self.status
    }

    /// One tool.
    pub fn get(&self, kind: ToolKind) -> Option<&ToolStatus> {
        self.status.iter().find(|s| s.kind == kind)
    }

    /// True after the first detection finished.
    pub fn detected(&self) -> bool {
        self.detected
    }

    /// True when every listed tool is ready.
    pub fn all_ready(&self) -> bool {
        self.detected && self.status.iter().all(|s| s.state == ToolState::Ready)
    }

    /// Re-detects every tool.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let status = self.api.status();
        self.detect = Some(cx.spawn(async move |this, cx| {
            let found = status.await;
            this.update(cx, |this, cx| {
                for s in found {
                    // keep a running install's progress
                    if !this.jobs.contains_key(&s.kind) {
                        this.set(s);
                    }
                }
                this.detected = true;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Installs every tool that is missing or failed.
    pub fn ensure_missing(&mut self, cx: &mut Context<Self>) {
        let todo: Vec<ToolKind> = self
            .status
            .iter()
            .filter(|s| matches!(s.state, ToolState::Missing | ToolState::Failed(_)))
            .map(|s| s.kind)
            .collect();
        for kind in todo {
            self.ensure(kind, cx);
        }
    }

    /// Installs `kind` if missing, with progress; transient failures retry up to three times.
    pub fn ensure(&mut self, kind: ToolKind, cx: &mut Context<Self>) {
        self.run(kind, false, cx)
    }

    /// Downloads `kind` again although one is installed.
    pub fn reinstall(&mut self, kind: ToolKind, cx: &mut Context<Self>) {
        self.run(kind, true, cx)
    }

    /// Asks whether a newer `kind` exists (`current` is what is installed); the answer comes as
    /// [`ToolsEvent::UpdateChecked`].
    pub fn check_update(
        &mut self,
        kind: ToolKind,
        current: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let check = self.api.check_update(kind, current);
        cx.spawn(async move |this, cx| {
            let latest = check.await;
            this.update(cx, |_, cx| {
                cx.emit(ToolsEvent::UpdateChecked { kind, latest })
            })
            .ok();
        })
        .detach();
    }

    fn run(&mut self, kind: ToolKind, force: bool, cx: &mut Context<Self>) {
        if self.jobs.contains_key(&kind) {
            return;
        }
        self.set_state(
            kind,
            ToolState::Downloading {
                done: 0,
                total: None,
            },
        );
        let (api, delay) = (self.api.clone(), self.retry_delay);
        let task = cx.spawn(async move |this, cx| {
            let mut outcome = ToolState::Failed(EngineError::new(
                ErrorCode::Unknown,
                "the install ended early",
            ));
            for attempt in 1..=TRIES {
                let mut events = match force {
                    true => api.reinstall(kind),
                    false => api.ensure(kind),
                };
                let mut failure = None;
                let mut finished = None;
                while let Some(event) = events.next().await {
                    match event {
                        ToolEvent::Progress { done, total } => {
                            this.update(cx, |this, cx| {
                                this.set_state(kind, ToolState::Downloading { done, total });
                                cx.notify();
                            })
                            .ok();
                        }
                        ToolEvent::Done(status) => finished = Some(status),
                        ToolEvent::Failed(error) => failure = Some(error),
                    }
                }
                if let Some(status) = finished {
                    this.update(cx, |this, cx| {
                        this.set(status);
                        this.jobs.remove(&kind);
                        if force {
                            cx.emit(ToolsEvent::Reinstalled(kind));
                        }
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                let Some(error) = failure else { break };
                let again = error.retryable && attempt < TRIES;
                log::warn!("tools: installing {kind} failed (try {attempt}/{TRIES}): {error}");
                outcome = ToolState::Failed(error);
                if !again {
                    break;
                }
                cx.background_executor().timer(delay * attempt).await;
            }
            this.update(cx, |this, cx| {
                this.set_state(kind, outcome);
                this.jobs.remove(&kind);
                cx.notify();
            })
            .ok();
        });
        self.jobs.insert(kind, task);
        cx.notify();
    }

    /// Points a tool at a user binary (rejected when it is not that tool).
    pub fn use_custom(&mut self, kind: ToolKind, path: String, cx: &mut Context<Self>) {
        let call = self.api.set_custom_path(kind, path);
        self.jobs.insert(
            kind,
            cx.spawn(async move |this, cx| {
                let result = call.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(status) => this.set(status),
                        Err(error) => this.set_state(kind, ToolState::Failed(error)),
                    }
                    this.jobs.remove(&kind);
                    cx.notify();
                })
                .ok();
            }),
        );
    }

    fn set(&mut self, status: ToolStatus) {
        match self.status.iter_mut().find(|s| s.kind == status.kind) {
            Some(slot) => *slot = status,
            None => self.status.push(status),
        }
    }

    fn set_state(&mut self, kind: ToolKind, state: ToolState) {
        match self.status.iter_mut().find(|s| s.kind == kind) {
            Some(slot) => slot.state = state,
            None => self.status.push(ToolStatus {
                kind,
                state,
                source: None,
                path: None,
                version: None,
            }),
        }
    }
}
