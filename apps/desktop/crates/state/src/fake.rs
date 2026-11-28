//! `FakeBackend`, `FakeTools` and the `--demo <scene>` seed rows: the UI-facing contract. The UI builds
//! screens against these; tests drive them by hand (`FakeBackend::manual`).
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use domain::{
    Backend, DownloadId, DownloadStatus, EngineError, ErrorCode, FormatInfo, JobEvent, JobHandle,
    JobSpec, PlaylistEntry, Preset, ProbeOpts, ProbeResult, Settings, ToolEvent, ToolKind,
    ToolSource, ToolState, ToolStatus, ToolsApi,
};
use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use storage::{NewDownload, SeedRow};
use tokio::runtime::Handle;

/// Demo scenes for `--demo <scene>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// No downloads, tools ready.
    Empty,
    /// A handful of queued rows that run (fake progress) plus a little history.
    Downloading,
    /// 10k rows of mixed history.
    Mixed,
    /// One failed row per error code, plus a retry waiting on its backoff.
    Failed,
    /// Nothing stored, no `first_run_done`, every tool missing.
    FirstRun,
}

impl Scene {
    /// Names accepted by `--demo`.
    pub const NAMES: &'static [&'static str] =
        &["empty", "downloading", "mixed", "failed", "first-run"];

    /// Parses a `--demo` value.
    pub fn parse(name: &str) -> Option<Scene> {
        Some(match name {
            "empty" => Scene::Empty,
            "downloading" => Scene::Downloading,
            "mixed" => Scene::Mixed,
            "failed" => Scene::Failed,
            "first-run" => Scene::FirstRun,
            _ => return None,
        })
    }
}

// ---- backend ----

enum Mode {
    /// Events are sent by the test.
    Manual,
    /// A tokio task plays a script; `speed` 2.0 runs twice as fast.
    Auto { handle: Handle, speed: f64 },
}

#[derive(Default)]
struct Inner {
    next: u64,
    senders: HashMap<u64, UnboundedSender<JobEvent>>,
    aborts: HashMap<u64, tokio::task::AbortHandle>,
    by_id: HashMap<DownloadId, JobHandle>,
    started: Vec<(DownloadId, JobSpec)>,
    killed: Vec<JobHandle>,
}

/// Scripted backend. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct FakeBackend {
    inner: Arc<Mutex<Inner>>,
    mode: Arc<Mode>,
}

impl FakeBackend {
    /// Events are pushed by hand with `send`/`finish`/`fail`.
    pub fn manual() -> Self {
        Self {
            inner: Arc::default(),
            mode: Arc::new(Mode::Manual),
        }
    }

    /// Plays a download script on `handle` (the Io runtime). URL words steer it: `fail` (network error
    /// at 40%), `private`, `429`, `slow` (4x longer).
    pub fn auto(handle: Handle, speed: f64) -> Self {
        Self {
            inner: Arc::default(),
            mode: Arc::new(Mode::Auto { handle, speed }),
        }
    }

    /// Jobs started so far, in order.
    pub fn started(&self) -> Vec<(DownloadId, JobSpec)> {
        self.inner
            .lock()
            .expect("fake backend lock")
            .started
            .clone()
    }

    /// Handles killed so far.
    pub fn killed(&self) -> Vec<JobHandle> {
        self.inner.lock().expect("fake backend lock").killed.clone()
    }

    /// The live handle of a started job.
    pub fn handle_of(&self, id: DownloadId) -> Option<JobHandle> {
        self.inner
            .lock()
            .expect("fake backend lock")
            .by_id
            .get(&id)
            .copied()
    }

    /// Manual mode: delivers an event to a running job.
    pub fn send(&self, id: DownloadId, event: JobEvent) {
        let inner = self.inner.lock().expect("fake backend lock");
        if let Some(tx) = inner.by_id.get(&id).and_then(|h| inner.senders.get(&h.0)) {
            tx.unbounded_send(event).ok();
        }
    }

    /// Manual mode: `Finished` and close the stream.
    pub fn finish(&self, id: DownloadId) {
        self.send(
            id,
            JobEvent::Finished {
                info_json: None,
                info_path: None,
            },
        );
        self.close(id);
    }

    /// Manual mode: `Failed` and close the stream.
    pub fn fail(&self, id: DownloadId, error: EngineError) {
        self.send(id, JobEvent::Failed(error));
        self.close(id);
    }

    fn close(&self, id: DownloadId) {
        let mut inner = self.inner.lock().expect("fake backend lock");
        if let Some(h) = inner.by_id.remove(&id) {
            inner.senders.remove(&h.0);
        }
    }
}

fn leaf(url: &str) -> String {
    let tail = url.rsplit(['/', '=']).next().unwrap_or(url);
    format!("Demo video {}", tail.trim())
}

/// A canned probe result; URL words pick the shape (`playlist`) or the failure.
pub fn fake_probe(url: &str) -> Result<ProbeResult, EngineError> {
    if url.contains("private") {
        return Err(EngineError::new(
            ErrorCode::Private,
            "Private video. Sign in if you've been granted access",
        ));
    }
    if url.contains("unsupported") {
        return Err(EngineError::new(
            ErrorCode::UnsupportedUrl,
            "Unsupported URL",
        ));
    }
    if url.contains("probefail") {
        return Err(EngineError::new(
            ErrorCode::Network,
            "Unable to download webpage: connection reset",
        ));
    }
    let mut r = ProbeResult {
        url: url.to_string(),
        webpage_url: Some(url.to_string()),
        extractor: Some("Youtube".into()),
        id: Some("demo".into()),
        title: Some(leaf(url)),
        uploader: Some("Demo Channel".into()),
        duration_s: Some(212.0),
        thumbnail_url: None,
        ..Default::default()
    };
    if url.contains("playlist") {
        r.title = Some("Demo playlist".into());
        r.entries = Some(
            (1..=25)
                .map(|i| PlaylistEntry {
                    index: i,
                    id: Some(format!("id{i}")),
                    url: format!("https://www.youtube.com/watch?v=demo{i}"),
                    title: Some(format!("Playlist entry {i}")),
                    duration_s: Some(100.0 + f64::from(i)),
                    thumbnail_url: None,
                })
                .collect(),
        );
    } else {
        for (i, h) in [2160u32, 1440, 1080, 720, 480].into_iter().enumerate() {
            r.formats.push(FormatInfo {
                id: format!("v{h}"),
                ext: Some("mp4".into()),
                width: Some(h * 16 / 9),
                height: Some(h),
                fps: Some(30.0),
                vcodec: Some("avc1".into()),
                acodec: None,
                filesize: Some(40_000_000 / (i as u64 + 1)),
                ..Default::default()
            });
        }
        for (id, ext, abr) in [("a140", "m4a", 128.0), ("a251", "webm", 160.0)] {
            r.formats.push(FormatInfo {
                id: id.into(),
                ext: Some(ext.into()),
                acodec: Some("opus".into()),
                abr: Some(abr),
                filesize: Some(3_500_000),
                ..Default::default()
            });
        }
    }
    Ok(r)
}

async fn play(id: DownloadId, spec: JobSpec, tx: UnboundedSender<JobEvent>, speed: f64) {
    let step = Duration::from_millis((250.0 / speed.max(0.01)) as u64);
    let slow = if spec.url.contains("slow") { 4 } else { 1 };
    let steps = (16 + (id.0 % 5) * 6) as u64 * slow;
    let total = 80_000_000u64 + (id.0 as u64 % 7) * 9_000_000;
    let send = |e: JobEvent| tx.unbounded_send(e).is_ok();
    tokio::time::sleep(step * 2).await;
    if !send(JobEvent::Ready) || !send(JobEvent::Phase("downloading".into())) {
        return;
    }
    send(JobEvent::Log {
        level: "info".into(),
        message: format!("[download] Destination: {}", spec.output_dir),
    });
    for i in 1..=steps {
        tokio::time::sleep(step).await;
        let done = total * i / steps;
        let fail_at = steps * 2 / 5;
        if i == fail_at {
            let error = if spec.url.contains("private") {
                Some(EngineError::new(
                    ErrorCode::Private,
                    "Private video. Sign in if you've been granted access",
                ))
            } else if spec.url.contains("429") {
                Some(EngineError::new(
                    ErrorCode::RateLimited,
                    "HTTP Error 429: Too Many Requests",
                ))
            } else if spec.url.contains("fail") {
                Some(EngineError::new(
                    ErrorCode::Network,
                    "Connection reset by peer",
                ))
            } else {
                None
            };
            if let Some(e) = error {
                send(JobEvent::Failed(e));
                return;
            }
        }
        if !send(JobEvent::Progress {
            done,
            total: Some(total),
            speed: Some(6_500_000.0),
            eta: Some((steps - i) as f64 * step.as_secs_f64()),
            frag: None,
        }) {
            return;
        }
    }
    if !matches!(spec.preset, Preset::Audio(domain::AudioFormat::Best)) {
        send(JobEvent::Phase("merging".into()));
        tokio::time::sleep(step * 3).await;
    }
    let ext = if spec.preset.kind() == domain::MediaKind::Audio {
        "m4a"
    } else {
        "mp4"
    };
    send(JobEvent::Path(format!(
        "{}/{}.{ext}",
        spec.output_dir,
        leaf(&spec.url)
    )));
    send(JobEvent::Finished {
        info_json: None,
        info_path: None,
    });
}

impl Backend for FakeBackend {
    fn probe(
        &self,
        url: String,
        _opts: ProbeOpts,
    ) -> BoxFuture<'static, Result<ProbeResult, EngineError>> {
        match &*self.mode {
            Mode::Manual => async move { fake_probe(&url) }.boxed(),
            Mode::Auto { handle, speed } => {
                let wait = Duration::from_millis((500.0 / speed.max(0.01)) as u64);
                let job = handle.spawn(async move {
                    tokio::time::sleep(wait).await;
                    fake_probe(&url)
                });
                async move {
                    job.await.unwrap_or_else(|e| {
                        Err(EngineError::new(ErrorCode::Unknown, e.to_string()))
                    })
                }
                .boxed()
            }
        }
    }

    fn start(
        &self,
        id: DownloadId,
        spec: JobSpec,
    ) -> BoxFuture<'static, Result<(JobHandle, BoxStream<'static, JobEvent>), EngineError>> {
        let (tx, rx) = unbounded();
        let handle = {
            let mut inner = self.inner.lock().expect("fake backend lock");
            inner.next += 1;
            let handle = JobHandle(inner.next);
            inner.started.push((id, spec.clone()));
            inner.by_id.insert(id, handle);
            inner.senders.insert(handle.0, tx.clone());
            handle
        };
        if let Mode::Auto { handle: rt, speed } = &*self.mode {
            let task = rt.spawn(play(id, spec, tx, *speed));
            self.inner
                .lock()
                .expect("fake backend lock")
                .aborts
                .insert(handle.0, task.abort_handle());
        }
        async move { Ok((handle, rx.boxed())) }.boxed()
    }

    fn kill(&self, handle: JobHandle) -> BoxFuture<'static, ()> {
        let mut inner = self.inner.lock().expect("fake backend lock");
        inner.killed.push(handle);
        inner.senders.remove(&handle.0);
        inner.by_id.retain(|_, h| *h != handle);
        if let Some(abort) = inner.aborts.remove(&handle.0) {
            abort.abort();
        }
        async {}.boxed()
    }
}

// ---- tools ----

/// Scripted tools: all ready, or all missing until `ensure` installs them.
pub struct FakeTools {
    handle: Option<Handle>,
    state: Arc<Mutex<(Vec<ToolStatus>, usize)>>,
}

fn status(kind: ToolKind, ready: bool) -> ToolStatus {
    ToolStatus {
        kind,
        state: if ready {
            ToolState::Ready
        } else {
            ToolState::Missing
        },
        source: ready.then_some(ToolSource::Managed),
        path: ready.then(|| format!("/fake/{kind}")),
        version: ready.then(|| "1.0".to_string()),
    }
}

impl FakeTools {
    /// With a runtime `handle` installs take about a second with progress; without one they finish at once.
    /// `missing` starts ffmpeg, ffprobe and deno as not installed (yt-dlp is always baked in).
    pub fn new(handle: Option<Handle>, missing: bool) -> Self {
        let tools = [
            ToolKind::Ffmpeg,
            ToolKind::Ffprobe,
            ToolKind::Deno,
            ToolKind::YtDlp,
            ToolKind::Aria2c,
        ]
        .into_iter()
        // optional aria2c is never installed: it stays missing
        .map(|k| {
            status(
                k,
                k != ToolKind::Aria2c && (!missing || k == ToolKind::YtDlp),
            )
        })
        .collect();
        Self {
            handle,
            state: Arc::new(Mutex::new((tools, 0))),
        }
    }

    /// The next `n` `ensure` calls fail with a retryable connection reset.
    pub fn fail_first(&self, n: usize) {
        self.state.lock().expect("fake tools lock").1 = n;
    }
}

impl ToolsApi for FakeTools {
    fn status(&self) -> BoxFuture<'static, Vec<ToolStatus>> {
        let all = self.state.lock().expect("fake tools lock").0.clone();
        async move { all }.boxed()
    }

    fn ensure(&self, kind: ToolKind) -> BoxStream<'static, ToolEvent> {
        let state = self.state.clone();
        let fail = {
            let mut s = state.lock().expect("fake tools lock");
            let fail = s.1 > 0;
            s.1 = s.1.saturating_sub(1);
            fail
        };
        let events = move || -> Vec<ToolEvent> {
            if fail {
                return vec![ToolEvent::Failed(EngineError::new(
                    ErrorCode::Network,
                    "connection reset by peer",
                ))];
            }
            let done = status(kind, true);
            let mut s = state.lock().expect("fake tools lock");
            if let Some(slot) = s.0.iter_mut().find(|t| t.kind == kind) {
                *slot = done.clone();
            }
            let mut out: Vec<_> = (1..=4)
                .map(|i| ToolEvent::Progress {
                    done: i * 25,
                    total: Some(100),
                })
                .collect();
            out.push(ToolEvent::Done(done));
            out
        };
        match &self.handle {
            None => futures::stream::iter(events()).boxed(),
            Some(handle) => {
                let (tx, rx) = unbounded();
                handle.spawn(async move {
                    for e in events() {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        if tx.unbounded_send(e).is_err() {
                            return;
                        }
                    }
                });
                rx.boxed()
            }
        }
    }

    fn set_custom_path(
        &self,
        kind: ToolKind,
        path: String,
    ) -> BoxFuture<'static, Result<ToolStatus, EngineError>> {
        async move {
            if path.contains("bad") {
                return Err(EngineError::new(
                    ErrorCode::Unknown,
                    format!("{path} is not {kind}"),
                ));
            }
            Ok(ToolStatus {
                source: Some(ToolSource::Custom),
                path: Some(path),
                ..status(kind, true)
            })
        }
        .boxed()
    }
}

// ---- demo rows ----

const SITES: &[(&str, &str)] = &[
    ("www.youtube.com", "Youtube"),
    ("vimeo.com", "Vimeo"),
    ("soundcloud.com", "Soundcloud"),
    ("www.twitch.tv", "Twitch"),
    ("www.reddit.com", "Reddit"),
    ("bandcamp.com", "Bandcamp"),
    ("www.dailymotion.com", "Dailymotion"),
    ("archive.org", "Archive"),
];
const WORDS: &[&str] = &[
    "Aurora", "Harbor", "Quiet", "Machines", "Slow", "Cinema", "Field", "Notes", "Midnight",
    "Garden", "Signal", "Paper", "Orbit", "Winter", "Tape", "Kitchen", "Concrete", "Radio",
    "Lights", "Atlas",
];

fn demo_row(i: usize, status: DownloadStatus, created_at: i64) -> SeedRow {
    let (host, extractor) = SITES[i % SITES.len()];
    let audio = i % 4 == 3;
    let preset: Preset = if audio {
        "audio:mp3"
    } else {
        ["video:best", "video:1080", "video:720"][i % 3]
    }
    .parse()
    .expect("valid preset");
    let url = format!("https://{host}/watch?v=demo{i:05}");
    let spec = Settings {
        output_dir: "C:/Videos/ytdlpd".into(),
        ..Default::default()
    };
    let job = JobSpec {
        url,
        preset,
        format_spec: if audio {
            "ba/b".into()
        } else {
            "bv*+ba/b".into()
        },
        output_dir: spec.output_dir,
        outtmpl: spec.outtmpl,
        playlist_index: None,
        embed_thumbnail: false,
        embed_metadata: true,
        embed_chapters: true,
        subtitles: Default::default(),
        sponsorblock: Default::default(),
        cookies: Default::default(),
        proxy: None,
        rate_limit: None,
        retries: None,
        ffmpeg_location: None,
        js_runtime: None,
        extra_args: vec![],
        aria2c: None,
        aria2c_args: vec![],
        fix: Default::default(),
        smaller_quality: None,
        smaller_height: None,
    };
    let mut new = NewDownload::from_spec(&job).expect("spec serialises");
    let (a, b, c) = (
        WORDS[i % WORDS.len()],
        WORDS[(i * 7 + 3) % WORDS.len()],
        WORDS[(i * 13 + 5) % WORDS.len()],
    );
    new.title = Some(format!("{a} {b} {c} {i}"));
    new.uploader = Some(format!("{b} {c} Channel"));
    new.extractor = Some(extractor.to_string());
    new.video_id = Some(format!("demo{i:05}"));
    // a few rows keep no thumbnail so the icon fallback shows
    if i % 7 != 6 {
        new.thumbnail_url = Some(format!(
            "file://{}/../../assets/fixtures/thumbs/t{}.jpg",
            env!("CARGO_MANIFEST_DIR"),
            i % 6
        ));
    }
    new.duration_s = Some(60.0 + (i % 50) as f64 * 37.0);
    new.format_label = Some(match preset {
        Preset::Audio(_) => "mp3".into(),
        Preset::Video(domain::VideoQuality::Best) => "best mp4".into(),
        Preset::Video(q) => format!("{q}p mp4"),
        Preset::Custom => "custom".into(),
    });
    let size = 4_000_000 + (i as i64 * 7_919_113) % 900_000_000;
    let done = status == DownloadStatus::Completed;
    SeedRow {
        file_path: done.then(|| {
            format!(
                "C:/Videos/ytdlpd/{a} {b} {c} {i}.{}",
                if audio { "mp3" } else { "mp4" }
            )
        }),
        file_size: (done || status == DownloadStatus::Paused).then_some(size),
        progress: match status {
            DownloadStatus::Completed => 1.0,
            DownloadStatus::Paused => 0.37,
            _ => 0.0,
        },
        new,
        status,
        reason: None,
        error: None,
        attempts: 0,
        next_attempt_at: None,
        created_at,
    }
}

/// Rows a scene starts with. `now` is the wall clock in ms.
pub fn seed_rows(scene: Scene, now: i64) -> Vec<SeedRow> {
    use DownloadStatus::*;
    match scene {
        Scene::Empty | Scene::FirstRun => vec![],
        Scene::Downloading => {
            let mut rows = Vec::new();
            for i in 0..2 {
                rows.push(demo_row(i, Completed, now - 3_600_000 + i as i64));
            }
            for i in 2..9 {
                rows.push(demo_row(i, Queued, now - 60_000 + i as i64 * 1000));
            }
            rows
        }
        Scene::Mixed => {
            let n = 10_000usize;
            (0..n)
                .map(|i| {
                    let status = match i % 40 {
                        0 => Failed,
                        1 => Cancelled,
                        2 => Paused,
                        r if r < 6 && i > n - 40 => Queued,
                        _ => Completed,
                    };
                    let mut row = demo_row(i, status, now - (n - i) as i64 * 90_000);
                    if status == Failed {
                        row.error = Some((
                            ErrorCode::Network,
                            "Unable to download webpage: connection reset".into(),
                        ));
                        row.attempts = 5;
                    }
                    row
                })
                .collect()
        }
        Scene::Failed => {
            let mut rows = Vec::new();
            let codes: Vec<ErrorCode> = ErrorCode::ALL
                .iter()
                .copied()
                .filter(|c| *c != ErrorCode::Cancelled)
                .collect();
            for (i, code) in codes.into_iter().enumerate() {
                let mut row = demo_row(i, Failed, now - 600_000 + i as i64 * 1000);
                row.error = Some((
                    code,
                    format!("{code}: demo failure text for this error code"),
                ));
                row.attempts = if code.retryable() { 5 } else { 1 };
                rows.push(row);
            }
            let mut waiting = demo_row(40, Queued, now - 5000);
            waiting.error = Some((ErrorCode::Network, "Connection reset by peer".into()));
            waiting.attempts = 2;
            waiting.next_attempt_at = Some(now + 40_000);
            rows.push(waiting);
            rows
        }
    }
}
