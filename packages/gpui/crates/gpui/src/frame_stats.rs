//! Frame timing written to a file for benchmarks.
//!
//! Set `GPUI_FRAME_STATS=<path>` before launch and every platform frame request ("tick") appends
//! one `F` line to that file, with an `S` summary line once a second. The header line gives the
//! wall-clock time that `F` timestamps count from, so a runner can cut a measurement window.
//! With the variable unset the recorder never exists and every hook reduces to one load of an
//! initialised `LazyLock`.
//!
//! All hooks run on the thread that drives frames. Phase times nest as follows: `tick` covers the
//! whole frame request, `draw` covers render, prepaint, layout, paint and finish, and `present`
//! covers acquire, encode and queue_present. `layout` is taffy's `compute_layout`, which runs
//! inside prepaint, so the recorded prepaint excludes it.

use std::{
    fmt::Write as _,
    fs::OpenOptions,
    io::{BufWriter, Write as _},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use crate::FrameRequestSource;

/// The environment variable that names the output file.
pub const ENV_VAR: &str = "GPUI_FRAME_STATS";

/// A timed part of a frame. See the module docs for how the phases nest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Phase {
    /// The whole platform frame request, from callback entry to return.
    Tick,
    /// `on_next_frame` callbacks run before drawing, which is where animations step.
    Callbacks,
    /// `Window::draw` as a whole.
    Draw,
    /// Root `request_layout`, which renders every dirty view and builds the taffy tree.
    Render,
    /// Everything between request_layout and paint, including layout.
    Prepaint,
    /// Taffy layout, including text measurement callbacks.
    Layout,
    /// Root paint, deferred draws and overlays.
    Paint,
    /// `PlatformWindow::draw`, the renderer's whole share of the frame.
    Present,
    /// Waiting for a swapchain image in `get_current_texture`.
    Acquire,
    /// Writing instance data, encoding passes and submitting them.
    Encode,
    /// `Queue::present`, which hands the image to the compositor.
    QueuePresent,
}

const PHASES: usize = 11;

/// Column names of an `F` line, in order.
const COLUMNS: &str = "t_ms src drew presented tick_us callbacks_us render_us layout_us \
                       prepaint_us paint_us finish_us acquire_us encode_us queue_present_us \
                       present_other_us gap_us";

static RECORDER: LazyLock<Option<Mutex<Recorder>>> = LazyLock::new(|| {
    let path = std::env::var_os(ENV_VAR)?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .inspect_err(|error| eprintln!("frame_stats: cannot open {path:?}: {error}"))
        .ok()?;
    let mut out = BufWriter::new(file);
    let now = Instant::now();
    let unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let _ = writeln!(
        out,
        "# pid {} epoch_unix_ms {unix_ms} columns: {COLUMNS}",
        std::process::id()
    );
    Some(Mutex::new(Recorder {
        out,
        epoch: now,
        frame: Frame::default(),
        depth: 0,
        last_present: None,
        second: Second::new(now),
    }))
});

/// The open output and the frame being accumulated.
struct Recorder {
    out: BufWriter<std::fs::File>,
    epoch: Instant,
    frame: Frame,
    /// Nesting depth of ticks, so a re-entrant request does not cut the outer frame short.
    depth: u32,
    last_present: Option<Instant>,
    second: Second,
}

/// Accumulated times and flags of one tick.
#[derive(Default)]
struct Frame {
    phases: [Duration; PHASES],
    native: bool,
    drew: bool,
    presented: bool,
    gap: Option<Duration>,
}

/// Counters for the one-second summary line.
struct Second {
    start: Instant,
    ticks: u32,
    native: u32,
    drawn: u32,
    presented: u32,
    drawn_ticks: Vec<u32>,
    gaps: Vec<u32>,
    phase_sums: [Duration; PHASES],
}

impl Second {
    fn new(start: Instant) -> Self {
        Self {
            start,
            ticks: 0,
            native: 0,
            drawn: 0,
            presented: 0,
            drawn_ticks: Vec::with_capacity(256),
            gaps: Vec::with_capacity(256),
            phase_sums: [Duration::ZERO; PHASES],
        }
    }
}

/// Starts timing a whole frame request and finishes it when dropped. Construct one at the top
/// of the platform's frame request handler so early returns are counted too.
#[must_use]
pub struct Tick(Option<Instant>);

impl Tick {
    /// Begins a tick, or returns an inert guard when stats are off.
    pub fn begin(source: FrameRequestSource) -> Self {
        let Some(recorder) = recorder() else {
            return Self(None);
        };
        let mut recorder = recorder.lock().unwrap_or_else(|e| e.into_inner());
        recorder.depth += 1;
        // The frame was reset when the last tick finished. A present made between ticks, such as
        // a held frame going out just before this request, stays with it.
        if recorder.depth == 1 {
            recorder.frame.native = source == FrameRequestSource::NativeCallback;
        }
        Self(Some(Instant::now()))
    }
}

impl Drop for Tick {
    fn drop(&mut self) {
        let Some(start) = self.0 else {
            return;
        };
        let Some(recorder) = recorder() else {
            return;
        };
        let mut recorder = recorder.lock().unwrap_or_else(|e| e.into_inner());
        recorder.depth = recorder.depth.saturating_sub(1);
        if recorder.depth == 0 {
            recorder.frame.phases[Phase::Tick as usize] += start.elapsed();
            recorder.finish_tick();
        }
    }
}

/// Whether `GPUI_FRAME_STATS` is set and its file opened.
#[inline]
pub fn enabled() -> bool {
    recorder().is_some()
}

/// The current time when stats are on, for a later [`record`].
#[inline]
pub fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// Adds the time since `since` to `phase` of the current frame. `None` is a no-op.
#[inline]
pub fn record(phase: Phase, since: Option<Instant>) {
    let Some(since) = since else {
        return;
    };
    let elapsed = since.elapsed();
    if let Some(recorder) = recorder() {
        let mut recorder = recorder.lock().unwrap_or_else(|e| e.into_inner());
        recorder.frame.phases[phase as usize] += elapsed;
    }
}

/// Marks that the current tick drew a new frame.
#[inline]
pub fn mark_drawn() {
    if let Some(recorder) = recorder() {
        recorder
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frame
            .drew = true;
    }
}

/// Marks that the renderer handed an image to the compositor during the current tick.
#[inline]
pub fn mark_presented() {
    if let Some(recorder) = recorder() {
        let now = Instant::now();
        let mut recorder = recorder.lock().unwrap_or_else(|e| e.into_inner());
        recorder.frame.presented = true;
        recorder.frame.gap = recorder.last_present.map(|last| now - last);
        recorder.last_present = Some(now);
    }
}

#[inline]
fn recorder() -> Option<&'static Mutex<Recorder>> {
    RECORDER.as_ref()
}

impl Recorder {
    fn finish_tick(&mut self) {
        let now = Instant::now();
        let frame = std::mem::take(&mut self.frame);
        let us = |phase: Phase| frame.phases[phase as usize].as_micros() as u64;
        let layout = us(Phase::Layout);
        let prepaint = us(Phase::Prepaint).saturating_sub(layout);
        let finish = us(Phase::Draw)
            .saturating_sub(us(Phase::Render) + us(Phase::Prepaint) + us(Phase::Paint));
        let present_other = us(Phase::Present)
            .saturating_sub(us(Phase::Acquire) + us(Phase::Encode) + us(Phase::QueuePresent));
        let gap = frame.gap.map_or(0, |gap| gap.as_micros() as u64);
        let _ = writeln!(
            self.out,
            "F {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            (now - self.epoch).as_millis(),
            match frame.native {
                true => "n",
                false => "l",
            },
            frame.drew as u8,
            frame.presented as u8,
            us(Phase::Tick),
            us(Phase::Callbacks),
            us(Phase::Render),
            layout,
            prepaint,
            us(Phase::Paint),
            finish,
            us(Phase::Acquire),
            us(Phase::Encode),
            us(Phase::QueuePresent),
            present_other,
            gap,
        );

        let second = &mut self.second;
        second.ticks += 1;
        second.native += frame.native as u32;
        second.drawn += frame.drew as u32;
        second.presented += frame.presented as u32;
        if frame.drew {
            second.drawn_ticks.push(us(Phase::Tick) as u32);
        }
        if frame.presented && gap > 0 {
            second.gaps.push(gap as u32);
        }
        for (sum, phase) in second.phase_sums.iter_mut().zip(frame.phases) {
            *sum += phase;
        }
        if now - second.start >= Duration::from_secs(1) {
            self.write_second(now);
        }
    }

    fn write_second(&mut self, now: Instant) {
        let second = std::mem::replace(&mut self.second, Second::new(now));
        let span = (now - second.start).as_secs_f64();
        let mut ticks = second.drawn_ticks;
        let mut gaps = second.gaps;
        ticks.sort_unstable();
        gaps.sort_unstable();
        let ms = |v: u32| v as f64 / 1000.0;
        let drawn = second.drawn.max(1) as f64;
        let mean = |phase: Phase| second.phase_sums[phase as usize].as_secs_f64() * 1000.0 / drawn;
        let mut line = String::new();
        let _ = write!(
            line,
            "S {:.1} ticks {} native {} drawn {} presented {} fps {:.1} \
             tick_ms p50 {:.2} p95 {:.2} p99 {:.2} max {:.2} gap_ms p50 {:.2} p95 {:.2} max {:.2} \
             per_drawn_ms tick {:.2} cb {:.2} render {:.2} layout {:.2} prepaint {:.2} paint {:.2} \
             present {:.2} acquire {:.2} encode {:.2} queue_present {:.2}",
            (now - self.epoch).as_secs_f64(),
            second.ticks,
            second.native,
            second.drawn,
            second.presented,
            second.presented as f64 / span,
            ms(percentile(&ticks, 50)),
            ms(percentile(&ticks, 95)),
            ms(percentile(&ticks, 99)),
            ms(ticks.last().copied().unwrap_or(0)),
            ms(percentile(&gaps, 50)),
            ms(percentile(&gaps, 95)),
            ms(gaps.last().copied().unwrap_or(0)),
            mean(Phase::Tick),
            mean(Phase::Callbacks),
            mean(Phase::Render),
            mean(Phase::Layout),
            mean(Phase::Prepaint) - mean(Phase::Layout),
            mean(Phase::Paint),
            mean(Phase::Present),
            mean(Phase::Acquire),
            mean(Phase::Encode),
            mean(Phase::QueuePresent),
        );
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }
}

/// The nearest-rank percentile of an ascending slice, or zero when it is empty.
fn percentile(sorted: &[u32], pct: usize) -> u32 {
    match sorted.len() {
        0 => 0,
        len => sorted[((len * pct).div_ceil(100)).saturating_sub(1).min(len - 1)],
    }
}
