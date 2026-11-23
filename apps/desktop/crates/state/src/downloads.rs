//! `Downloads`: the queue. Rows come from storage; lifecycle goes through `domain::transition`;
//! the engine is reached only through `domain::Backend`.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use domain::{
    Backend, Clock, DownloadId, DownloadStatus, DownloadSummary, Effects, EngineError, ErrorCode,
    JobEvent, JobHandle, JobSpec, ProbeResult, StateEvent, XorShift, next_attempt_at, should_retry,
    transition,
};
use futures::StreamExt as _;
use gpui::{Context, Entity, EventEmitter, Task};
use storage::NewDownload;

use crate::scheduler::{Limits, Scheduler};
use crate::settings::{AppSettings, SettingsChanged};
use crate::{Logs, Store};

/// Redraw tick while jobs run.
const TICK: Duration = Duration::from_millis(250);
/// Progress is persisted at most this often (phase changes are written at once).
const PROGRESS_WRITE_MS: i64 = 5000;

/// What the views may react to besides `cx.notify()`.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadsEvent {
    /// Rows were read from storage.
    Loaded,
    Completed(DownloadId),
    /// Failed for good (no more automatic retries).
    Failed(DownloadId, EngineError),
}

/// Live numbers that are not persisted.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Live {
    pub speed: Option<f64>,
    pub eta: Option<f64>,
    pub frag: Option<(u32, u32)>,
}

struct Job {
    handle: Option<JobHandle>,
    path: Option<String>,
    last_write: i64,
    /// The runner; dropping it stops event handling.
    _task: Task<()>,
}

/// The download queue entity.
pub struct Downloads {
    rows: Vec<DownloadSummary>,
    loaded: bool,
    load_error: Option<String>,
    jobs: HashMap<DownloadId, Job>,
    specs: HashMap<DownloadId, JobSpec>,
    /// Partial files per download, kept through pause, deleted on cancel.
    partials: HashMap<DownloadId, Vec<String>>,
    live: HashMap<DownloadId, Live>,
    backend: Arc<dyn Backend>,
    store: Store,
    clock: Arc<dyn Clock>,
    logs: Entity<Logs>,
    limits: Limits,
    max_attempts: u32,
    resume_on_start: bool,
    scheduler: Scheduler,
    rng: XorShift,
    dirty: bool,
    tick: Option<Task<()>>,
    load_task: Option<Task<()>>,
}

impl EventEmitter<DownloadsEvent> for Downloads {}

fn limits_from(s: &domain::Settings) -> Limits {
    Limits {
        global: s.concurrent_global.max(1) as usize,
        per_domain: s.concurrent_per_domain.max(1) as usize,
        ..Limits::default()
    }
}

impl Downloads {
    /// Builds the entity state; call `load` next. Limits follow `settings`.
    pub fn new(
        backend: Arc<dyn Backend>,
        store: Store,
        clock: Arc<dyn Clock>,
        logs: Entity<Logs>,
        settings: &Entity<AppSettings>,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = settings.read(cx).get().clone();
        cx.subscribe(settings, |this, settings, _: &SettingsChanged, cx| {
            let s = settings.read(cx).get().clone();
            this.apply_settings(&s, cx);
        })
        .detach();
        let seed = clock.now_ms() as u64;
        Self {
            rows: vec![],
            loaded: false,
            load_error: None,
            jobs: HashMap::new(),
            specs: HashMap::new(),
            partials: HashMap::new(),
            live: HashMap::new(),
            backend,
            store,
            clock,
            logs,
            limits: limits_from(&current),
            max_attempts: current.max_attempts,
            resume_on_start: current.resume_on_start,
            scheduler: Scheduler::default(),
            rng: XorShift(seed ^ 0x9E37_79B9_7F4A_7C15),
            dirty: false,
            tick: None,
            load_task: None,
        }
    }

    /// Re-reads limits and the retry cap (also called on settings changes).
    pub fn apply_settings(&mut self, s: &domain::Settings, cx: &mut Context<Self>) {
        self.limits = limits_from(s);
        self.max_attempts = s.max_attempts;
        self.resume_on_start = s.resume_on_start;
        self.pump(cx);
    }

    // ---- reading ----

    /// Rows, newest first.
    pub fn rows(&self) -> &[DownloadSummary] {
        &self.rows
    }

    /// One row.
    pub fn get(&self, id: DownloadId) -> Option<&DownloadSummary> {
        self.rows.iter().find(|r| r.id == id)
    }

    /// True once storage answered.
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// Why loading failed (database unusable).
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Speed, ETA and fragment counters of a running job.
    pub fn live(&self, id: DownloadId) -> Option<&Live> {
        self.live.get(&id)
    }

    /// Workers currently attached (tests: leaks show up here).
    pub fn running_jobs(&self) -> usize {
        self.jobs.len()
    }

    /// Queued rows waiting for a slot or their backoff.
    pub fn pending_len(&self) -> usize {
        self.count(|s| s == DownloadStatus::Queued)
    }

    /// Number of rows per status (chip counts).
    pub fn count(&self, pred: impl Fn(DownloadStatus) -> bool) -> usize {
        self.rows.iter().filter(|r| pred(r.status)).count()
    }

    /// Time source (the views show "retrying in N s" from `next_attempt_at - now`).
    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// True while a 429 penalty holds `domain` to fewer slots.
    pub fn penalized(&self, domain: &str) -> bool {
        self.scheduler.penalized(domain, self.clock.now_ms())
    }

    // ---- loading and enqueue ----

    /// Recovers interrupted rows (active becomes paused, reason `interrupted`) and reads all rows.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        let read = self.store.read(|db| async move {
            db.recover_interrupted().await?;
            db.list_summaries().await
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            this.update(cx, |this, cx| this.loaded_rows(result, cx))
                .ok();
        }));
    }

    fn loaded_rows(&mut self, result: Result<Vec<DownloadSummary>>, cx: &mut Context<Self>) {
        match result {
            Ok(rows) => {
                self.rows = rows;
                self.loaded = true;
                self.load_error = None;
                if self.resume_on_start {
                    let ids: Vec<_> = self
                        .rows
                        .iter()
                        .filter(|r| {
                            r.status == DownloadStatus::Paused
                                && r.status_reason.as_deref() == Some("interrupted")
                        })
                        .map(|r| r.id)
                        .collect();
                    for id in ids {
                        self.apply(id, StateEvent::Resume, cx);
                    }
                }
                cx.emit(DownloadsEvent::Loaded);
                self.pump(cx);
            }
            Err(error) => {
                log::error!("downloads: cannot load: {error:#}");
                self.load_error = Some(format!("{error:#}"));
            }
        }
        cx.notify();
    }

    /// Adds a queued download. `probe` fills title, uploader and the stored info (re-opens Choose formats).
    /// The task resolves with the new id once the row is stored.
    pub fn enqueue(
        &mut self,
        spec: JobSpec,
        probe: Option<&ProbeResult>,
        format_label: Option<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<DownloadId>> {
        let mut new = match NewDownload::from_spec(&spec) {
            Ok(new) => new,
            Err(error) => return cx.spawn(async move |_, _| Err(error)),
        };
        new.format_label = format_label;
        if let Some(p) = probe {
            new.webpage_url = p.webpage_url.clone();
            new.extractor = p.extractor.clone();
            new.video_id = p.id.clone();
            new.title = p.title.clone();
            new.uploader = p.uploader.clone();
            new.duration_s = p.duration_s;
            new.thumbnail_url = p.thumbnail_url.clone();
            new.info_json = p.info_json.clone();
        }
        let stored = new.clone();
        let read = self
            .store
            .read(move |db| async move { db.insert_download(&stored).await });
        cx.spawn(async move |this, cx| {
            let id = read.await?;
            this.update(cx, |this, cx| this.inserted(id, &new, spec, cx))?;
            Ok(id)
        })
    }

    fn inserted(
        &mut self,
        id: DownloadId,
        new: &NewDownload,
        spec: JobSpec,
        cx: &mut Context<Self>,
    ) {
        let now = self.clock.now_ms();
        let position = self.rows.iter().map(|r| r.position).fold(0.0, f64::max) + 1.0;
        let preset = spec.preset;
        let mut row = DownloadSummary::new(id, &new.url, preset, now);
        row.position = position;
        row.domain = if new.domain.is_empty() {
            row.domain
        } else {
            new.domain.clone()
        };
        row.extractor = new.extractor.clone();
        row.video_id = new.video_id.clone();
        row.title = new.title.clone();
        row.uploader = new.uploader.clone();
        row.duration_s = new.duration_s;
        row.thumbnail_url = new.thumbnail_url.clone();
        row.format_label = new.format_label.clone();
        row.playlist_id = new.playlist_id;
        row.playlist_index = new.playlist_index;
        row.refresh_lc();
        self.rows.insert(0, row);
        self.specs.insert(id, spec);
        cx.notify();
        self.pump(cx);
    }

    // ---- user actions ----

    /// Pause: kills the worker, keeps partials; a queued row leaves the queue.
    pub fn pause(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.apply(id, StateEvent::Pause, cx);
        self.pump(cx);
    }

    /// Resume a paused row: back to the queue (same row, `continuedl`).
    pub fn resume(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.apply(id, StateEvent::Resume, cx);
        self.pump(cx);
    }

    /// Cancel: a queued row is cancelled immediately and never reaches the engine; an active one is
    /// killed and its known partials deleted.
    pub fn cancel(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.apply(id, StateEvent::Cancel, cx);
        self.pump(cx);
    }

    /// Manual retry of a failed or cancelled row: same row, attempts keep counting, backoff gate reset.
    pub fn retry(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.apply(id, StateEvent::Retry, cx);
        self.pump(cx);
    }

    /// Removes a row that is not active; returns false (and does nothing) for an active row.
    pub fn delete(&mut self, id: DownloadId, cx: &mut Context<Self>) -> bool {
        match self.get(id) {
            Some(r) if !r.status.is_active() => {}
            _ => return false,
        }
        self.rows.retain(|r| r.id != id);
        self.specs.remove(&id);
        self.partials.remove(&id);
        self.live.remove(&id);
        self.store
            .write(move |db| async move { db.delete_download(id).await.map(|_| ()) });
        cx.notify();
        self.pump(cx);
        true
    }

    /// Pauses every active and queued row (shutdown); resolves once the database is flushed and closed.
    pub fn shutdown(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let ids: Vec<_> = self
            .rows
            .iter()
            .filter(|r| r.status.is_active() || r.status == DownloadStatus::Queued)
            .map(|r| r.id)
            .collect();
        for id in ids {
            self.apply(id, StateEvent::Pause, cx);
        }
        self.tick = None;
        let close = self.store.close();
        cx.spawn(async move |_, _| close.await)
    }

    // ---- lifecycle core ----

    /// Applies a state event to a row and performs its effects. `None` when illegal or unknown.
    fn apply(
        &mut self,
        id: DownloadId,
        event: StateEvent,
        cx: &mut Context<Self>,
    ) -> Option<Effects> {
        let now = self.clock.now_ms();
        let row = self.rows.iter_mut().find(|r| r.id == id)?;
        let (to, fx) = match transition(row.status, event) {
            Ok(t) => t,
            Err(error) => {
                log::warn!("downloads: {id}: {error}");
                return None;
            }
        };
        let was = row.status;
        row.status = to;
        row.updated_at = now;
        row.status_reason = (event == StateEvent::Interrupted).then(|| "interrupted".to_string());
        if to == DownloadStatus::Probing {
            row.started_at.get_or_insert(now);
            row.error_code = None;
            row.error_message = None;
            row.phase = None;
        }
        if to.is_terminal() {
            row.finished_at = Some(now);
            row.phase = None;
        } else {
            row.finished_at = None;
        }
        if fx.attempts_inc {
            row.attempts += 1;
        }
        if fx.clear_backoff {
            row.next_attempt_at = None;
            row.error_code = None;
            row.error_message = None;
        }
        if to == DownloadStatus::Queued && was == DownloadStatus::Paused {
            row.next_attempt_at = None;
        }
        let (attempts, next) = (row.attempts, row.next_attempt_at);
        let (reason, phase) = (row.status_reason.clone(), row.phase.clone());
        let clear_error = row.error_code.is_none();

        if fx.kill_worker {
            self.kill(id, cx);
        }
        if fx.delete_partials {
            self.delete_partials(id);
        }
        if to.is_terminal() || to == DownloadStatus::Paused || to == DownloadStatus::Queued {
            self.live.remove(&id);
        }
        if to.is_terminal() {
            self.specs.remove(&id);
        }
        self.store.write(move |db| async move {
            db.set_status(id, to, reason.as_deref()).await?;
            db.set_phase(id, phase.as_deref()).await?;
            db.set_retry(id, attempts, next).await?;
            if clear_error {
                db.set_error(id, None).await?;
            }
            Ok(())
        });
        cx.notify();
        Some(fx)
    }

    fn kill(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        if let Some(job) = self.jobs.remove(&id)
            && let Some(handle) = job.handle
        {
            let kill = self.backend.kill(handle);
            cx.spawn(async move |_, _| kill.await).detach();
        }
    }

    fn delete_partials(&mut self, id: DownloadId) {
        let files = self.partials.remove(&id).unwrap_or_default();
        if !files.is_empty() {
            // shortcut: sync delete of a few files on the gpui thread, move to Io if cleanup gets heavy.
            for f in files {
                std::fs::remove_file(&f).ok();
            }
        }
    }

    /// Starts every queued row the scheduler admits and keeps the tick alive.
    pub fn pump(&mut self, cx: &mut Context<Self>) {
        if !self.loaded {
            return;
        }
        let now = self.clock.now_ms();
        for id in self.scheduler.pick(&self.rows, now, &self.limits) {
            if self.apply(id, StateEvent::Start, cx).is_some() {
                self.spawn_runner(id, cx);
            }
        }
        self.ensure_tick(cx);
    }

    fn spawn_runner(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let cached = self.specs.get(&id).cloned();
        let detail = cached.is_none().then(|| {
            self.store.read(move |db| async move {
                let d = db
                    .get_detail(id)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("row {id} is gone"))?;
                Ok(serde_json::from_str::<JobSpec>(&d.options_json)?)
            })
        });
        let task = cx.spawn(async move |this, cx| {
            let spec = match (cached, detail) {
                (Some(spec), _) => Ok(spec),
                (None, Some(read)) => read.await,
                (None, None) => unreachable!("either a cached spec or a read"),
            };
            let spec = match spec {
                Ok(spec) => spec,
                Err(error) => {
                    let error = EngineError::new(
                        ErrorCode::Unknown,
                        format!("cannot read the job: {error:#}"),
                    );
                    this.update(cx, |this, cx| this.job_failed(id, error, cx))
                        .ok();
                    return;
                }
            };
            match backend.start(id, spec).await {
                Err(error) => {
                    this.update(cx, |this, cx| this.job_failed(id, error, cx))
                        .ok();
                }
                Ok((handle, mut events)) => {
                    let attached = this.update(cx, |this, _| match this.jobs.get_mut(&id) {
                        Some(job) => {
                            job.handle = Some(handle);
                            true
                        }
                        None => false,
                    });
                    if !matches!(attached, Ok(true)) {
                        // cancelled while starting: make sure the worker dies
                        backend.kill(handle).await;
                        return;
                    }
                    while let Some(event) = events.next().await {
                        if this
                            .update(cx, |this, cx| this.on_event(id, event, cx))
                            .is_err()
                        {
                            return;
                        }
                    }
                    this.update(cx, |this, cx| this.stream_ended(id, cx)).ok();
                }
            }
        });
        self.jobs.insert(
            id,
            Job {
                handle: None,
                path: None,
                last_write: self.clock.now_ms(),
                _task: task,
            },
        );
    }

    fn on_event(&mut self, id: DownloadId, event: JobEvent, cx: &mut Context<Self>) {
        if !self.jobs.contains_key(&id) {
            return; // late event of a killed job
        }
        let now = self.clock.now_ms();
        match event {
            JobEvent::Ready => {}
            JobEvent::Progress {
                done,
                total,
                speed,
                eta,
                frag,
            } => {
                if self
                    .get(id)
                    .is_some_and(|r| r.status == DownloadStatus::Probing)
                {
                    self.apply(id, StateEvent::PhaseDownloading, cx);
                }
                self.live.insert(id, Live { speed, eta, frag });
                let Some(row) = self.rows.iter_mut().find(|r| r.id == id) else {
                    return;
                };
                row.bytes_done = Some(done as i64);
                row.bytes_total = total.map(|t| t as i64);
                if let Some(t) = total.filter(|t| *t > 0) {
                    row.progress = (done as f64 / t as f64).clamp(0.0, 1.0);
                }
                self.dirty = true;
                let (progress, done, total) = (row.progress, row.bytes_done, row.bytes_total);
                if let Some(job) = self.jobs.get_mut(&id)
                    && now - job.last_write >= PROGRESS_WRITE_MS
                {
                    job.last_write = now;
                    self.store.write(move |db| async move {
                        db.set_progress(id, progress, done, total).await
                    });
                }
            }
            JobEvent::Phase(name) => {
                let (ev, phase) = if name == "downloading" {
                    (StateEvent::PhaseDownloading, None)
                } else {
                    (StateEvent::PhaseProcessing, Some(name))
                };
                self.apply(id, ev, cx);
                if let Some(row) = self.rows.iter_mut().find(|r| r.id == id) {
                    row.phase = phase.clone();
                }
                self.store
                    .write(move |db| async move { db.set_phase(id, phase.as_deref()).await });
                if let Some(job) = self.jobs.get_mut(&id) {
                    job.last_write = now;
                }
            }
            JobEvent::Path(path) => {
                if let Some(job) = self.jobs.get_mut(&id) {
                    job.path = Some(path);
                }
            }
            JobEvent::Files(files) => self.partials.entry(id).or_default().extend(files),
            JobEvent::Log { level, message } => {
                self.logs
                    .update(cx, |l, cx| l.push(Some(id), &level, &message, cx));
                self.store
                    .write(move |db| async move { db.append_log(id, &level, &message).await });
            }
            JobEvent::Finished { .. } => self.finished(id, cx),
            JobEvent::Failed(error) => self.job_failed(id, error, cx),
        }
    }

    fn finished(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let path = self.jobs.get_mut(&id).and_then(|j| j.path.take());
        // shortcut: stat on the gpui thread (one file per completion), move to Io if it ever stalls.
        let size = path
            .as_deref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len() as i64);
        if self.apply(id, StateEvent::Finished, cx).is_none() {
            return;
        }
        self.jobs.remove(&id);
        self.partials.remove(&id);
        let Some(row) = self.rows.iter_mut().find(|r| r.id == id) else {
            return;
        };
        row.progress = 1.0;
        row.file_path = path.clone();
        row.file_size = size.or(row.bytes_total);
        let (size, total) = (row.file_size, row.bytes_total);
        self.store.write(move |db| async move {
            db.set_progress(id, 1.0, total, total).await?;
            if let Some(path) = path {
                db.set_final_path(id, &path, size).await?;
            }
            Ok(())
        });
        cx.emit(DownloadsEvent::Completed(id));
        self.pump(cx);
    }

    fn stream_ended(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        if self.jobs.contains_key(&id) {
            self.job_failed(
                id,
                EngineError::new(ErrorCode::Unknown, "the worker exited without a result"),
                cx,
            );
        }
    }

    /// A worker failed: retry on the same row with backoff when the code allows, else fail for good.
    fn job_failed(&mut self, id: DownloadId, error: EngineError, cx: &mut Context<Self>) {
        let now = self.clock.now_ms();
        let Some(row) = self.get(id) else { return };
        if !row.status.is_active() {
            return;
        }
        let (domain, attempts) = (row.domain.clone(), row.attempts + 1);
        let retry = should_retry(error.code, attempts, self.max_attempts);
        self.jobs.remove(&id);
        let event = if retry {
            StateEvent::FailRetryable
        } else {
            StateEvent::FailFinal
        };
        if self.apply(id, event, cx).is_none() {
            return;
        }
        let next = retry
            .then(|| next_attempt_at(&*self.clock, attempts, error.retry_after, &mut self.rng));
        if let Some(row) = self.rows.iter_mut().find(|r| r.id == id) {
            row.error_code = Some(error.code.as_str().to_string());
            row.error_message = Some(error.message.clone());
            row.next_attempt_at = next;
        }
        if error.code == ErrorCode::RateLimited {
            self.scheduler
                .penalize(&domain, now, error.retry_after, &self.limits);
        }
        let (code, message) = (error.code, error.message.clone());
        self.store.write(move |db| async move {
            db.set_error(id, Some((code, &message))).await?;
            db.set_retry(id, attempts - u32::from(!retry), next).await
        });
        if !retry {
            cx.emit(DownloadsEvent::Failed(id, error));
        }
        self.pump(cx);
    }

    // ---- tick ----

    fn needs_tick(&self) -> bool {
        self.rows.iter().any(|r| {
            r.status.is_active()
                || (r.status == DownloadStatus::Queued && r.next_attempt_at.is_some())
        })
    }

    fn ensure_tick(&mut self, cx: &mut Context<Self>) {
        if self.tick.is_some() || !self.needs_tick() {
            return;
        }
        self.tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                match this.update(cx, |this, cx| this.on_tick(cx)) {
                    Ok(true) => {}
                    _ => break,
                }
            }
        }));
    }

    /// One redraw tick: notify when something changed, release backoff-gated rows. False stops the loop.
    pub fn on_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if self.dirty {
            self.dirty = false;
            cx.notify();
        }
        let now = self.clock.now_ms();
        let due = self.scheduler.pick(&self.rows, now, &self.limits);
        if !due.is_empty() {
            self.pump(cx);
        }
        if self.needs_tick() {
            true
        } else {
            self.tick = None;
            false
        }
    }

    /// True while the 250 ms tick runs (tests: it must stop when nothing runs).
    pub fn ticking(&self) -> bool {
        self.tick.is_some()
    }
}
