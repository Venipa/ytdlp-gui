//! `Fixes`: after a download finishes, ffprobe reads the file; a site rule's (or the default) fix
//! runs ffmpeg on it, and a file that may not play everywhere gets a hint the detail panel shows
//! with a button that runs the Compatible fix. Plans are pure (`domain::fixes`); this runs them.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use domain::fixes::{
    Plan, SmallerOpts, Streams, Unplayable, describe, hw_encoder, parse_ffprobe, plan_chain,
    progress_share, scaled_height, smaller_bitrate, smaller_gains_nothing, source_bps, unplayable,
};
use domain::{DownloadId, Fix, FixChain, JobSpec, ToolKind};
use gpui::{App, Context, Entity};

use crate::{AppSettings, Downloads, Io, Store, Tools};

/// What the detail panel shows about a finished file.
#[derive(Debug, Clone, PartialEq)]
pub enum FixState {
    /// Waiting for a free place: only so many fixes run at once.
    Queued(FixChain),
    Running(FixChain),
    Failed(FixChain, String),
    /// The file may not play in the system players.
    Unplayable(Unplayable),
}

/// A fix asked for: what to run, and the Smaller file choices of the download's site rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Job {
    fix: FixChain,
    quality: Option<u8>,
    height: Option<u32>,
}

/// What `Slots::submit` did with a request.
#[derive(Debug, PartialEq, Eq)]
enum Submitted<T> {
    /// A place was free: start it now.
    Start(T),
    Queued,
    /// The download already runs or waits.
    Ignored,
}

/// The places for running fixes: at most `limit` run, the rest wait first come, first served.
struct Slots<T> {
    running: HashSet<DownloadId>,
    waiting: VecDeque<(DownloadId, T)>,
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self {
            running: HashSet::new(),
            waiting: VecDeque::new(),
        }
    }
}

impl<T> Slots<T> {
    fn submit(&mut self, id: DownloadId, job: T, limit: usize) -> Submitted<T> {
        if self.running.contains(&id) || self.waiting.iter().any(|(w, _)| *w == id) {
            return Submitted::Ignored;
        }
        if self.running.len() < limit {
            self.running.insert(id);
            return Submitted::Start(job);
        }
        self.waiting.push_back((id, job));
        Submitted::Queued
    }

    /// Starts waiting jobs while places are free (also after the limit was raised).
    fn fill(&mut self, limit: usize) -> Vec<(DownloadId, T)> {
        let mut start = vec![];
        while self.running.len() < limit
            && let Some((id, job)) = self.waiting.pop_front()
        {
            self.running.insert(id);
            start.push((id, job));
        }
        start
    }

    /// `id` is done (or failed): its place goes to the next waiting jobs.
    fn finish(&mut self, id: DownloadId, limit: usize) -> Vec<(DownloadId, T)> {
        self.running.remove(&id);
        self.fill(limit)
    }
}

pub struct Fixes {
    io: Io,
    store: Store,
    tools: Entity<Tools>,
    settings: Entity<AppSettings>,
    downloads: Entity<Downloads>,
    states: HashMap<DownloadId, FixState>,
    /// How far each running fix is.
    shares: HashMap<DownloadId, Share>,
    slots: Slots<Job>,
    /// Hints the user closed.
    dismissed: HashSet<DownloadId>,
}

/// ffmpeg's encoder list, read once per run of the app.
static ENCODERS: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn tool(path: &Option<String>, name: &str) -> PathBuf {
    path.clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(name))
}

async fn run(program: &Path, args: &[String]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = cmd.output().await.map_err(|e| {
        let name = program.file_name().unwrap_or_default().to_string_lossy();
        format!("cannot run {name}: {e}")
    })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ffmpeg failed")
            .to_owned())
    }
}

async fn probe(ffprobe: &Path, file: &Path) -> Result<Streams, String> {
    let args: Vec<String> = [
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_streams",
        "-show_format",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([file.to_string_lossy().into_owned()])
    .collect();
    parse_ffprobe(&run(ffprobe, &args).await?).ok_or_else(|| "ffprobe output not understood".into())
}

/// The fixes of `chain` for the log: "Smaller+Normalize".
fn names(chain: FixChain) -> String {
    chain
        .fixes()
        .iter()
        .map(|f| format!("{f:?}"))
        .collect::<Vec<_>>()
        .join("+")
}

/// Removes a file, logging anything but "already gone".
/// Moves a finished file that waited for its fix out of the temp folder into its download
/// folder, so a failed fix still leaves the download where it belongs. `None` when it was not
/// waiting there or cannot move (a file of that name is already there).
fn leave_temp(file: &Path) -> Option<PathBuf> {
    let home = domain::fixes::out_of_temp(file.parent()?)?;
    let dest = home.join(file.file_name()?);
    if dest.exists() {
        log::warn!(
            "fixes: a file of that name is already in the download folder, the download stays in the temp folder"
        );
        return None;
    }
    let moved = std::fs::create_dir_all(&home).and_then(|_| std::fs::rename(file, &dest));
    match moved {
        Ok(()) => Some(dest),
        Err(e) => {
            log::warn!("fixes: cannot move the download out of the temp folder: {e}");
            None
        }
    }
}

fn remove(file: &Path) {
    if let Err(e) = std::fs::remove_file(file)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("fixes: cannot remove the original file: {e}");
    }
}

/// Runs `plan` on `file`, writing next to it; returns the new file (the original is replaced).
/// How far a running fix is, in thousandths (0 until ffmpeg reports).
type Share = Arc<AtomicU32>;

/// Runs ffmpeg with `-progress` on stdout, storing in `share` how much of `total_us` is done.
async fn run_watched(
    ffmpeg: &Path,
    args: &[String],
    total_us: Option<u64>,
    share: &Share,
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(["-progress", "pipe:1", "-nostats"])
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    let (stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
    let watch = async {
        let Some(stdout) = stdout else { return };
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(done) = total_us.and_then(|t| progress_share(&line, t)) {
                share.store((done * 1000.) as u32, Ordering::Relaxed);
            }
        }
    };
    let errors = async {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_string(&mut text).await;
        }
        text
    };
    let ((), errors, status) = tokio::join!(watch, errors, child.wait());
    let status = status.map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    if status.success() {
        return Ok(());
    }
    Err(errors
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("ffmpeg failed")
        .to_owned())
}

async fn apply(
    ffmpeg: &Path,
    file: &Path,
    plan: &Plan,
    total_us: Option<u64>,
    share: &Share,
) -> Result<PathBuf, String> {
    share.store(0, Ordering::Relaxed);
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = file.parent().unwrap_or_else(|| Path::new("."));
    // a download that waited in the temp folder: the result goes into its download folder
    let home = domain::fixes::out_of_temp(dir).unwrap_or_else(|| dir.to_path_buf());
    let tmp = dir.join(format!("{stem}.fixing.{}", plan.ext));
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.extend(plan.input.iter().cloned());
    args.extend(["-i".into(), file.to_string_lossy().into_owned()]);
    args.extend(plan.output.iter().cloned());
    args.push(tmp.to_string_lossy().into_owned());
    if let Err(error) = run_watched(ffmpeg, &args, total_us, share).await {
        remove(&tmp);
        return Err(error);
    }
    let dest = home.join(format!("{stem}.{}", plan.ext));
    // a different extension must not overwrite some other file of that name
    let dest = match dest != file && dest.exists() {
        true => home.join(format!("{stem} (fixed).{}", plan.ext)),
        false => dest,
    };
    let moved = std::fs::create_dir_all(&home).and_then(|_| std::fs::rename(&tmp, &dest));
    if let Err(e) = moved {
        remove(&tmp);
        return Err(format!("cannot replace the file: {e}"));
    }
    if dest != file {
        remove(file);
    }
    Ok(dest)
}

/// The video encoder a plan runs (`copy` when it keeps the video, `none` for audio).
fn video_encoder(plan: &Plan) -> String {
    plan.output
        .windows(2)
        .find(|w| w[0] == "-c:v")
        .map_or_else(|| "none".into(), |w| w[1].clone())
}

/// What Smaller file makes of `st`, for the log: "av1 4.1 Mb/s 2160p -> hevc 2.6 Mb/s 1080p (quality 80%)".
fn smaller_line(st: &Streams, opts: SmallerOpts) -> String {
    let mbps = |b: u64| format!("{:.1} Mb/s", b as f64 / 1e6);
    let height = |h: Option<i64>| h.map_or(String::new(), |h| format!(" {h}p"));
    let to = match smaller_bitrate(st, opts) {
        Some(b) => mbps(b),
        None => "constant quality".into(),
    };
    format!(
        "{} {}{} -> hevc {to}{} (quality {}%)",
        st.video.as_deref().unwrap_or("?"),
        source_bps(st).map_or_else(|| "? Mb/s".into(), mbps),
        height(st.height),
        height(scaled_height(st, opts).or(st.height)),
        opts.quality
    )
}

/// Runs `fix` on `file`: probes it, picks encoders (hardware unless `encoder` is `software`,
/// software again when the hardware run fails) and applies the plan. Returns the new file and
/// the video encoder that made it.
#[allow(clippy::too_many_arguments)]
async fn fix_file(
    id: DownloadId,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    file: PathBuf,
    fix: FixChain,
    encoder: String,
    smaller: SmallerOpts,
    share: Share,
) -> Result<(PathBuf, String), String> {
    let streams = probe(&ffprobe, &file).await?;
    let total = streams.duration_us;
    let mut fix = fix;
    if fix.video == Some(Fix::Smaller) && smaller_gains_nothing(&streams, smaller) {
        log::info!(
            "fixes: download {id}: video kept, already as small as quality {}% allows ({})",
            smaller.quality,
            smaller_line(&streams, smaller)
        );
        fix.video = None;
    }
    if fix.is_empty() {
        // nothing left to run: a download that waited for its fix only moves home
        return Ok((leave_temp(&file).unwrap_or(file), "none".into()));
    }
    let name = names(fix);
    if fix.video == Some(Fix::Smaller) && streams.video.is_some() {
        log::info!(
            "fixes: Smaller on download {id}: {}",
            smaller_line(&streams, smaller)
        );
    }
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let macos = cfg!(target_os = "macos");
    let soft = plan_chain(fix, &ext, &streams, "libx264", "libx265", macos, smaller);
    if encoder == "software" {
        return Ok((
            apply(&ffmpeg, &file, &soft, total, &share).await?,
            video_encoder(&soft),
        ));
    }
    let first = ENCODERS.get().is_none();
    if first {
        let list = run(&ffmpeg, &["-hide_banner".into(), "-encoders".into()])
            .await
            .unwrap_or_else(|e| {
                log::warn!("fixes: cannot list ffmpeg encoders, using the CPU: {e}");
                String::new()
            });
        ENCODERS.set(list).ok();
    }
    let list = ENCODERS.get().map(String::as_str).unwrap_or("");
    let h264 = hw_encoder(list, "h264").unwrap_or_else(|| "libx264".into());
    let hevc = hw_encoder(list, "hevc").unwrap_or_else(|| "libx265".into());
    if first {
        log::info!("fixes: encoders picked: {h264}, {hevc}");
    }
    let mut hard = plan_chain(fix, &ext, &streams, &h264, &hevc, macos, smaller);
    if hard != soft {
        // a GPU encoder: decode on the GPU too (AV1/VP9 decoding was the CPU's whole load);
        // ffmpeg decodes in software when the GPU cannot
        hard.input
            .splice(0..0, ["-hwaccel".to_string(), "auto".to_string()]);
    }
    match apply(&ffmpeg, &file, &hard, total, &share).await {
        Ok(done) => Ok((done, video_encoder(&hard))),
        // a listed GPU encoder can still fail (no GPU, old driver): software is the fallback
        Err(error) if hard != soft && encoder == "auto" => {
            log::warn!("fixes: {name} on the GPU ({h264}, {hevc}) failed, using the CPU: {error}");
            Ok((
                apply(&ffmpeg, &file, &soft, total, &share).await?,
                video_encoder(&soft),
            ))
        }
        Err(error) => Err(error),
    }
}

impl Fixes {
    pub fn new(
        io: Io,
        store: Store,
        tools: Entity<Tools>,
        settings: Entity<AppSettings>,
        downloads: Entity<Downloads>,
        cx: &mut Context<Self>,
    ) -> Self {
        // a raised limit starts waiting fixes right away
        cx.observe(&settings, |this, _, cx| this.fill(cx)).detach();
        cx.subscribe(&downloads, |this, _, event: &crate::DownloadsEvent, cx| {
            match event {
                crate::DownloadsEvent::Completed(id) => this.finished(*id, cx),
                // downloads that waited for their fix when the app last quit
                crate::DownloadsEvent::Loaded => {
                    let waiting: Vec<_> = this
                        .downloads
                        .read(cx)
                        .rows()
                        .iter()
                        .filter(|r| {
                            r.status == domain::DownloadStatus::Completed && !r.file_missing
                        })
                        .filter(|r| {
                            r.file_path.as_deref().is_some_and(|p| {
                                Path::new(p)
                                    .parent()
                                    .and_then(domain::fixes::out_of_temp)
                                    .is_some()
                            })
                        })
                        .map(|r| r.id)
                        .collect();
                    for id in waiting {
                        log::info!("fixes: download {id} still waits for its fix, starting it");
                        this.finished(id, cx);
                    }
                }
                _ => {}
            }
        })
        .detach();
        Self {
            io,
            store,
            tools,
            settings,
            downloads,
            states: HashMap::new(),
            shares: HashMap::new(),
            slots: Slots::default(),
            dismissed: HashSet::new(),
        }
    }

    /// What the detail panel shows for `id`.
    pub fn state(&self, id: DownloadId) -> Option<&FixState> {
        self.states
            .get(&id)
            .filter(|_| !self.dismissed.contains(&id))
    }

    /// The fix running on `id`'s file, for the row's status.
    pub fn running(&self, id: DownloadId) -> Option<FixChain> {
        match self.states.get(&id) {
            Some(FixState::Running(fix)) => Some(*fix),
            _ => None,
        }
    }

    /// How far the running fix of `id` is (0 to 1), once ffmpeg has said.
    pub fn progress(&self, id: DownloadId) -> Option<f32> {
        let done = self.shares.get(&id)?.load(Ordering::Relaxed);
        (done > 0).then(|| done as f32 / 1000.)
    }

    /// The fix of `id` that waits for a free place, for the row's status.
    pub fn queued(&self, id: DownloadId) -> Option<FixChain> {
        match self.states.get(&id) {
            Some(FixState::Queued(fix)) => Some(*fix),
            _ => None,
        }
    }

    /// How many fixes may run at once.
    fn limit(&self, cx: &App) -> usize {
        usize::from(self.settings.read(cx).get().fix_concurrency.clamp(1, 4))
    }

    /// Starts waiting fixes while places are free.
    fn fill(&mut self, cx: &mut Context<Self>) {
        let limit = self.limit(cx);
        for (id, job) in self.slots.fill(limit) {
            self.start(id, job, cx);
        }
    }

    /// `id`'s fix ended: the next waiting ones take its place.
    fn release(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let limit = self.limit(cx);
        for (id, job) in self.slots.finish(id, limit) {
            self.start(id, job, cx);
        }
    }

    pub fn dismiss(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.dismissed.insert(id);
        cx.notify();
    }

    fn paths(&self, cx: &App) -> (PathBuf, PathBuf) {
        let tools = self.tools.read(cx);
        let path = |kind| tools.get(kind).and_then(|s| s.path.clone());
        (
            tool(&path(ToolKind::Ffmpeg), "ffmpeg"),
            tool(&path(ToolKind::Ffprobe), "ffprobe"),
        )
    }

    /// A download finished: run its fix (from its stored spec), then check the result.
    fn finished(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let read = self.store.read(move |db| async move {
            let detail = db.get_detail(id).await?;
            Ok(detail
                .map(|d| serde_json::from_str::<JobSpec>(&d.options_json))
                .transpose()?)
        });
        cx.spawn(async move |this, cx| {
            let job = read
                .await
                .inspect_err(|e| {
                    log::warn!("fixes: cannot read the spec of download {id}, no fix runs: {e:#}")
                })
                .ok()
                .flatten()
                .and_then(|spec| {
                    (!spec.fix.is_empty()).then_some(Job {
                        fix: spec.fix,
                        quality: spec.smaller_quality,
                        height: spec.smaller_height,
                    })
                });
            this.update(cx, |this, cx| match job {
                Some(job) => this.submit(id, job, cx),
                None => this.check(id, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Reads the finished file of `id` with ffprobe and records a hint when it may not play.
    pub fn check(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let Some(file) = self
            .downloads
            .read(cx)
            .get(id)
            .and_then(|r| r.file_path.clone())
        else {
            return;
        };
        let (_, ffprobe) = self.paths(cx);
        let io = self.io.clone();
        cx.spawn(async move |this, cx| {
            let file = PathBuf::from(file);
            let ext = file
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            let result = io.spawn(async move { probe(&ffprobe, &file).await }).await;
            let streams = match result {
                Ok(Ok(streams)) => streams,
                Ok(Err(e)) => {
                    log::warn!("fixes: cannot probe download {id}: {e}");
                    return;
                }
                Err(e) => {
                    log::warn!("fixes: cannot probe download {id}: {e}");
                    return;
                }
            };
            this.update(cx, |this, cx| {
                match unplayable(&ext, &streams) {
                    Some(why) => this.states.insert(id, FixState::Unplayable(why)),
                    None => this.states.remove(&id),
                };
                // the format column says what the file holds now, after any fix
                let label = describe(&ext, &streams);
                this.downloads
                    .update(cx, |d, cx| d.set_format(id, label, cx));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs `fix` (one fix, or a chain to retry) on the finished file of `id` (the hint's button): Smaller file uses the settings.
    pub fn run(&mut self, id: DownloadId, fix: impl Into<FixChain>, cx: &mut Context<Self>) {
        let job = Job {
            fix: fix.into(),
            quality: None,
            height: None,
        };
        self.submit(id, job, cx);
    }

    /// Runs `job` now, or when a place frees up; one already running or waiting is left alone.
    fn submit(&mut self, id: DownloadId, job: Job, cx: &mut Context<Self>) {
        let fix = job.fix;
        let name = names(fix);
        let has_file = self
            .downloads
            .read(cx)
            .get(id)
            .is_some_and(|r| r.file_path.is_some());
        if !has_file {
            log::warn!("fixes: download {id} has no file, {name} skipped");
            return;
        }
        let limit = self.limit(cx);
        match self.slots.submit(id, job, limit) {
            Submitted::Ignored => {}
            Submitted::Queued => {
                self.dismissed.remove(&id);
                self.states.insert(id, FixState::Queued(fix));
                log::info!("fixes: {name} on download {id} waits for a free place");
                cx.notify();
            }
            Submitted::Start(job) => self.start(id, job, cx),
        }
    }

    /// Runs `job` on the finished file of `id`; its place in `slots` is already taken.
    fn start(&mut self, id: DownloadId, job: Job, cx: &mut Context<Self>) {
        let fix = job.fix;
        let name = names(fix);
        let Some(file) = self
            .downloads
            .read(cx)
            .get(id)
            .and_then(|r| r.file_path.clone())
        else {
            log::warn!("fixes: download {id} has no file, {name} skipped");
            self.states.remove(&id);
            self.release(id, cx);
            return;
        };
        let (ffmpeg, ffprobe) = self.paths(cx);
        let (encoder, smaller) = {
            let s = self.settings.read(cx).get();
            (
                s.encoder.clone(),
                SmallerOpts::resolve(s, job.quality, job.height),
            )
        };
        self.dismissed.remove(&id);
        self.states.insert(id, FixState::Running(fix));
        cx.notify();
        let io = self.io.clone();
        let downloads = self.downloads.clone();
        let before = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        log::info!(
            "fixes: {name} on download {id} started ({} MB, encoder setting {encoder})",
            before / 1_000_000
        );
        let started = std::time::Instant::now();
        let source = PathBuf::from(&file);
        let share = Share::default();
        self.shares.insert(id, share.clone());
        // redraw the percent while ffmpeg works
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let going = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.shares.contains_key(&id)
                    })
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let job = io.spawn(fix_file(
                id,
                ffmpeg,
                ffprobe,
                source.clone(),
                fix,
                encoder,
                smaller,
                share,
            ));
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            this.update(cx, |this, cx| {
                this.shares.remove(&id);
                match result {
                    Ok((done, used)) => {
                        this.states.remove(&id);
                        let size = std::fs::metadata(&done).ok().map(|m| m.len() as i64);
                        log::info!(
                            "fixes: {name} on download {id} done in {:.0} s with {used}: {} MB to {} MB",
                            started.elapsed().as_secs_f32(),
                            before / 1_000_000,
                            size.unwrap_or(0) / 1_000_000
                        );
                        let path = done.to_string_lossy().into_owned();
                        downloads.update(cx, |d, cx| d.replace_file(id, path, size, cx));
                        this.check(id, cx);
                    }
                    Err(error) => {
                        log::warn!("fixes: {name} on download {id} failed: {error}");
                        this.states.insert(id, FixState::Failed(fix, error));
                        if let Some(out) = leave_temp(&source) {
                            let path = out.to_string_lossy().into_owned();
                            downloads.update(cx, |d, cx| d.move_file(id, path, cx));
                        }
                    }
                }
                this.release(id, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs every fix on a real 2 s AV1/Opus clip. Needs `YTDLPD_TEST_FFMPEG` and
    /// `YTDLPD_TEST_FFPROBE`; skipped without them (CI has no ffmpeg).
    #[test]
    fn every_fix_produces_a_playable_file() {
        let (Ok(ffmpeg), Ok(ffprobe)) = (
            std::env::var("YTDLPD_TEST_FFMPEG"),
            std::env::var("YTDLPD_TEST_FFPROBE"),
        ) else {
            eprintln!("skipped: set YTDLPD_TEST_FFMPEG and YTDLPD_TEST_FFPROBE");
            return;
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let chains = Fix::ALL.map(FixChain::from).into_iter().chain([FixChain {
            video: Some(Fix::Smaller),
            normalize: true,
            repair: true,
        }]);
        for fix in chains {
            let label = names(fix);
            // the whole chain runs on a download that waited in the temp folder
            let waited = fix.normalize && fix.repair;
            let temp = dir.path().join(domain::fixes::TEMP_NAME);
            std::fs::create_dir_all(&temp).unwrap();
            let src = match waited {
                true => temp.join(format!("{label}.mp4")),
                false => dir.path().join(format!("{label}.mp4")),
            };
            let make: Vec<String> = [
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=25:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libsvtav1",
                // a real download's rate: a test pattern alone would sit under Smaller's floor
                "-b:v",
                "1M",
                "-c:a",
                "libopus",
                "-shortest",
            ]
            .iter()
            .map(|s| s.to_string())
            .chain([src.to_string_lossy().into_owned()])
            .collect();
            rt.block_on(run(Path::new(&ffmpeg), &make)).unwrap();
            let src_copy = dir.path().join(format!("{label}-source.mp4"));
            std::fs::copy(&src, &src_copy).unwrap();
            let share = Share::default();
            let (done, _) = rt
                .block_on(fix_file(
                    DownloadId(0),
                    ffmpeg.clone().into(),
                    ffprobe.clone().into(),
                    src.clone(),
                    fix,
                    "auto".into(),
                    SmallerOpts {
                        quality: 60,
                        max_height: 0,
                    },
                    share.clone(),
                ))
                .unwrap_or_else(|e| panic!("{label}: {e}"));
            if waited {
                assert_eq!(done.parent(), Some(dir.path()), "the result goes home");
                assert_eq!(
                    std::fs::read_dir(&temp).unwrap().count(),
                    0,
                    "temp left empty"
                );
            }
            // ffmpeg reported its progress up to (about) the end
            assert!(share.load(Ordering::Relaxed) >= 900, "{label}: {share:?}");
            let streams = rt.block_on(probe(Path::new(&ffprobe), &done)).unwrap();
            assert!(
                streams.video.is_some() && streams.audio.is_some(),
                "{label}: {streams:?}"
            );
            if fix.video == Some(Fix::Compatible) {
                let ext = done.extension().unwrap().to_string_lossy().into_owned();
                assert_eq!(unplayable(&ext, &streams), None, "{streams:?}");
                assert!(!src.exists() || src == done);
            }
            if fix.video == Some(Fix::Smaller) {
                let before = std::fs::metadata(&src_copy).unwrap().len();
                let after = std::fs::metadata(&done).unwrap().len();
                assert_eq!(streams.video.as_deref(), Some("hevc"), "{streams:?}");
                // the audio is re-encoded to AAC (the clip's Opus is tiny), so compare the video
                let source = rt.block_on(probe(Path::new(&ffprobe), &src_copy)).unwrap();
                let (was, now) = (source_bps(&source), source_bps(&streams));
                eprintln!("Smaller video bit rate: {was:?} to {now:?}");
                assert!(
                    now.unwrap() as f64 <= was.unwrap() as f64 * 1.15,
                    "Smaller grew the video: {was:?} to {now:?}"
                );
                eprintln!("Smaller: {before} B to {after} B");
            }
            eprintln!("{label}: {} {streams:?}", done.display());
        }

        // AV1 at 80% cannot shrink: the video is kept, nothing re-encodes, and a download that
        // waited in the temp folder only moves home, byte for byte
        let temp = dir.path().join(domain::fixes::TEMP_NAME);
        let waiting = temp.join("kept.mp4");
        std::fs::copy(dir.path().join("Smaller-source.mp4"), &waiting).unwrap();
        let bytes = std::fs::read(&waiting).unwrap();
        let (done, used) = rt
            .block_on(fix_file(
                DownloadId(0),
                ffmpeg.into(),
                ffprobe.into(),
                waiting.clone(),
                FixChain::from(Fix::Smaller),
                "auto".into(),
                SmallerOpts {
                    quality: 80,
                    max_height: 0,
                },
                Share::default(),
            ))
            .unwrap();
        assert_eq!((done.parent(), used.as_str()), (Some(dir.path()), "none"));
        assert_eq!(std::fs::read(&done).unwrap(), bytes);
        assert!(!waiting.exists());
    }

    #[test]
    fn fixes_run_up_to_the_limit_in_order() {
        let id = DownloadId;
        let mut slots = Slots::<u8>::default();
        assert_eq!(slots.submit(id(1), 1, 2), Submitted::Start(1));
        assert_eq!(slots.submit(id(2), 2, 2), Submitted::Start(2));
        assert_eq!(slots.submit(id(3), 3, 2), Submitted::Queued);
        assert_eq!(slots.submit(id(4), 4, 2), Submitted::Queued);
        // running or waiting: asked again does nothing
        assert_eq!(slots.submit(id(1), 9, 2), Submitted::Ignored);
        assert_eq!(slots.submit(id(4), 9, 2), Submitted::Ignored);
        // one ends: the oldest waiting takes its place
        assert_eq!(slots.finish(id(1), 2), vec![(id(3), 3)]);
        assert_eq!(slots.running.len(), 2);
        // a raised limit starts the rest at once
        assert_eq!(slots.fill(3), vec![(id(4), 4)]);
        assert!(slots.fill(4).is_empty());
        // a lowered limit starts nothing until enough have ended
        assert_eq!(slots.submit(id(5), 5, 1), Submitted::Queued);
        assert!(slots.finish(id(2), 1).is_empty());
        assert!(slots.finish(id(3), 1).is_empty());
        assert_eq!(slots.finish(id(4), 1), vec![(id(5), 5)]);
    }

    #[test]
    fn the_log_line_names_both_sides() {
        let st = Streams {
            video: Some("av1".into()),
            height: Some(2160),
            video_bps: Some(4_100_000),
            ..Default::default()
        };
        let opts = SmallerOpts {
            quality: 80,
            max_height: 1080,
        };
        assert_eq!(
            smaller_line(&st, opts),
            "av1 4.1 Mb/s 2160p -> hevc 1.1 Mb/s 1080p (quality 80%)"
        );
    }
}
