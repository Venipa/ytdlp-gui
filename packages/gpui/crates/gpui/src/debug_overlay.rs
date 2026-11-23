//! A developer overlay that paints frame-time statistics directly into the
//! scene, bypassing layout, text, and view invalidation entirely (to avoid
//! infinitely triggering new frames).

use crate::{
    BorderStyle, Bounds, ContentMask, Corners, Edges, EntityId, Hsla, Pixels, Quad, ScaledPixels,
    Scene, Size, point, rgba, size, transparent_black,
};
use std::{
    collections::VecDeque,
    env,
    time::{Duration, Instant},
};

#[allow(missing_docs)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum DebugFrameOverlayMode {
    #[default]
    Hidden,
    Minimal,
    Full,
}

impl DebugFrameOverlayMode {
    /// Returns the next mode in the Hidden → FrameTime → Detailed cycle.
    pub fn next(self) -> Self {
        match self {
            Self::Hidden => Self::Minimal,
            Self::Minimal => Self::Full,
            Self::Full => Self::Hidden,
        }
    }
}

/// How long a view stays washed after it re-renders. One frame would be gone
/// before anyone could see it, so the mark fades out over this instead.
const FLASH: Duration = Duration::from_millis(160);

/// The number of most recent draw durations retained for percentile statistics.
const MAX_SAMPLES: usize = 1000;

const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
/// Glyph advance and line advance, in font cells.
const CHAR_ADVANCE: f32 = (GLYPH_WIDTH + 1) as f32;
const LINE_ADVANCE: f32 = (GLYPH_HEIGHT + 2) as f32;
/// Padding between the panel edge and the text, in font cells.
const PANEL_PADDING: f32 = 2.0;
/// Margin between the panel and the window corner, in font cells.
const PANEL_MARGIN: f32 = 4.0;
/// Side of one square font cell, in logical pixels.
const CELL_SIZE: f32 = 2.0;

fn text_color() -> Hsla {
    rgba(0x33ff33ff).into()
}

fn panel_color() -> Hsla {
    rgba(0x000000aa).into()
}

fn repaint_fill() -> Hsla {
    rgba(0xff00ff2b).into()
}

fn repaint_edge() -> Hsla {
    rgba(0xff00ffbb).into()
}

/// What one frame spent in each draw phase, and how much of the view tree it
/// rebuilt rather than replayed from a cached prepaint.
#[derive(Clone, Copy, Default)]
pub(crate) struct FrameWork {
    pub layout: Duration,
    pub prepaint: Duration,
    pub paint: Duration,
    pub rendered: u32,
    pub reused: u32,
    pub dirty: u32,
    pub refreshing: bool,
    pub views: u32,
}

pub(crate) struct DebugFrameOverlay {
    mode: DebugFrameOverlayMode,
    draw_durations: VecDeque<Duration>,
    total_frame_count: u64,
    work: FrameWork,
    counting: FrameWork,
    repaints: Vec<Repaint>,
    notified: Vec<EntityId>,
    tally: Vec<(EntityId, u32)>,
    flushed: Instant,
    logging: bool,
    show_repaints: bool,
}

struct Repaint {
    view: EntityId,
    bounds: Bounds<Pixels>,
    at: Instant,
}

impl DebugFrameOverlay {
    pub(crate) fn new() -> Self {
        Self {
            mode: mode_from_env(),
            draw_durations: VecDeque::new(),
            total_frame_count: 0,
            work: FrameWork::default(),
            counting: FrameWork::default(),
            repaints: Vec::new(),
            notified: Vec::new(),
            tally: Vec::new(),
            flushed: Instant::now(),
            logging: flag_from_env("GPUI_LOG_NOTIFIES"),
            show_repaints: flag_from_env("GPUI_SHOW_REPAINTS"),
        }
    }

    pub(crate) fn show_repaints(&self) -> bool {
        self.show_repaints
    }

    pub(crate) fn set_show_repaints(&mut self, show: bool) {
        self.show_repaints = show;
        self.repaints.clear();
    }

    /// Records a view that re-rendered, so the wash can mark it. A view that
    /// keeps rendering keeps its mark fresh; one that stops fades out.
    pub(crate) fn note_repaint(&mut self, view: EntityId, bounds: Bounds<Pixels>) {
        if !self.show_repaints {
            return;
        }
        let at = Instant::now();
        match self.repaints.iter_mut().find(|mark| mark.view == view) {
            Some(mark) => {
                mark.bounds = bounds;
                mark.at = at;
            }
            None => self.repaints.push(Repaint { view, bounds, at }),
        }
    }

    /// Notes the view a notify named, to be washed once its bounds are known.
    pub(crate) fn note_notified(&mut self, view: EntityId) {
        if self.show_repaints && !self.notified.contains(&view) {
            self.notified.push(view);
        }
        if self.logging {
            match self.tally.iter_mut().find(|(id, _)| *id == view) {
                Some((_, count)) => *count += 1,
                None => self.tally.push((view, 1)),
            }
        }
    }

    /// Hands over a second's worth of notifies for logging, if it is time.
    pub(crate) fn take_tally(&mut self) -> Option<Vec<(EntityId, u32)>> {
        if !self.logging || self.flushed.elapsed() < Duration::from_secs(1) {
            return None;
        }
        self.flushed = Instant::now();
        let mut tally = std::mem::take(&mut self.tally);
        tally.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(*count));
        Some(tally)
    }

    pub(crate) fn was_notified(&self, view: EntityId) -> bool {
        self.notified.contains(&view)
    }

    /// Whether a mark is still fading, and so another frame is needed to carry
    /// it out. Without this the last drawn frame would keep its wash forever.
    pub(crate) fn wants_frame(&self) -> bool {
        !self.repaints.is_empty()
    }

    /// Records how the frame started: how many views were marked dirty, and
    /// whether something asked the whole window to refresh.
    pub(crate) fn note_start(&mut self, dirty: u32, refreshing: bool) {
        self.counting.dirty = dirty;
        self.counting.refreshing = refreshing;
    }

    pub(crate) fn note_rendered_view(&mut self) {
        self.counting.views += 1;
    }

    pub(crate) fn note_view(&mut self, reused: bool) {
        match reused {
            true => self.counting.reused += 1,
            false => self.counting.rendered += 1,
        }
    }

    pub(crate) fn note_phases(&mut self, layout: Duration, prepaint: Duration, paint: Duration) {
        self.counting.layout = layout;
        self.counting.prepaint = prepaint;
        self.counting.paint = paint;
    }

    /// Starts a fresh tally, retiring marks that have finished fading.
    pub(crate) fn begin_frame(&mut self) {
        self.repaints.retain(|mark| mark.at.elapsed() < FLASH);
        self.counting = FrameWork::default();
    }

    /// Drops the notifies this frame has already accounted for.
    pub(crate) fn settle_notified(&mut self) {
        self.notified.clear();
    }

    /// Hands the frame being counted over to the readout and starts a new one.
    pub(crate) fn settle_frame(&mut self) {
        self.work = self.counting;
        self.counting = FrameWork::default();
    }

    pub(crate) fn mode(&self) -> DebugFrameOverlayMode {
        self.mode
    }

    pub(crate) fn set_mode(&mut self, mode: DebugFrameOverlayMode) {
        self.mode = mode;
    }

    /// Clears the draw-duration samples, restarting the percentile window.
    /// The total frame count is left untouched.
    pub(crate) fn reset_stats(&mut self) {
        self.draw_durations.clear();
        self.repaints.clear();
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.mode != DebugFrameOverlayMode::Hidden || self.show_repaints
    }

    pub(crate) fn record_frame(&mut self, draw_duration: Duration) {
        self.total_frame_count += 1;
        if self.draw_durations.len() >= MAX_SAMPLES {
            self.draw_durations.pop_front();
        }
        self.draw_durations.push_back(draw_duration);
    }

    pub(crate) fn paint(&self, scene: &mut Scene, viewport_size: Size<Pixels>, scale_factor: f32) {
        if !self.is_enabled() {
            return;
        }

        self.paint_repaints(scene, viewport_size, scale_factor);

        let lines = self.lines();
        if lines.is_empty() {
            return;
        }
        let max_line_chars = lines.iter().map(|line| line.len()).max().unwrap_or(0);
        // Ensure at least one physical pixel per cell so the text stays legible
        // at fractional downscale factors.
        let cell = (CELL_SIZE * scale_factor).max(1.0);

        let panel_width = cell * (max_line_chars as f32 * CHAR_ADVANCE + 2.0 * PANEL_PADDING);
        let panel_height = cell * (lines.len() as f32 * LINE_ADVANCE + 2.0 * PANEL_PADDING);
        let viewport = viewport_size.scale(scale_factor);
        let panel_left = viewport.width.0 - panel_width - cell * PANEL_MARGIN;
        let panel_top = cell * PANEL_MARGIN;

        let content_mask = ContentMask {
            bounds: Bounds {
                origin: point(ScaledPixels(0.), ScaledPixels(0.)),
                size: viewport,
            },
        };

        scene.insert_primitive(solid_quad(
            scaled_bounds(panel_left, panel_top, panel_width, panel_height),
            &content_mask,
            panel_color(),
        ));

        let text_color = text_color();
        for (line_index, line) in lines.iter().enumerate() {
            let line_top = panel_top + cell * (PANEL_PADDING + line_index as f32 * LINE_ADVANCE);
            for (char_index, character) in line.chars().enumerate() {
                let Some(rows) = glyph(character) else {
                    continue;
                };
                let glyph_left =
                    panel_left + cell * (PANEL_PADDING + char_index as f32 * CHAR_ADVANCE);
                for (row_index, row) in rows.iter().enumerate() {
                    let row_top = line_top + cell * row_index as f32;
                    // Merge horizontal runs of lit cells into single quads.
                    let mut column = 0;
                    while column < GLYPH_WIDTH {
                        if row & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                            column += 1;
                            continue;
                        }
                        let run_start = column;
                        while column < GLYPH_WIDTH && row & (1 << (GLYPH_WIDTH - 1 - column)) != 0 {
                            column += 1;
                        }
                        scene.insert_primitive(solid_quad(
                            scaled_bounds(
                                glyph_left + cell * run_start as f32,
                                row_top,
                                cell * (column - run_start) as f32,
                                cell,
                            ),
                            &content_mask,
                            text_color,
                        ));
                    }
                }
            }
        }
    }

    /// Washes every view that re-rendered this frame, the way Android's surface
    /// update flash does: what lights up is what the frame actually rebuilt.
    fn paint_repaints(&self, scene: &mut Scene, viewport_size: Size<Pixels>, scale_factor: f32) {
        if self.repaints.is_empty() {
            return;
        }

        let viewport = viewport_size.scale(scale_factor);
        let content_mask = ContentMask {
            bounds: Bounds {
                origin: point(ScaledPixels(0.), ScaledPixels(0.)),
                size: viewport,
            },
        };
        let edge = (scale_factor).max(1.);
        for mark in &self.repaints {
            let left = 1. - mark.at.elapsed().as_secs_f32() / FLASH.as_secs_f32();
            if left <= 0. {
                continue;
            }
            let bounds = mark.bounds.scale(scale_factor);
            scene.insert_primitive(solid_quad(
                bounds,
                &content_mask,
                repaint_fill().opacity(left),
            ));
            scene.insert_primitive(Quad {
                order: 0,
                border_style: BorderStyle::Solid,
                bounds,
                content_mask,
                background: transparent_black().into(),
                border_color: repaint_edge().opacity(left),
                corner_radii: Corners::default(),
                border_widths: Edges::all(ScaledPixels(edge)),
                corner_smoothing: 0.,
                pad: 0,
            });
        }
    }

    fn lines(&self) -> Vec<String> {
        let current = self.draw_durations.back().copied();
        match self.mode {
            DebugFrameOverlayMode::Hidden => Vec::new(),
            DebugFrameOverlayMode::Minimal => vec![format_ms(current)],
            DebugFrameOverlayMode::Full => {
                let mut sorted: Vec<Duration> = self.draw_durations.iter().copied().collect();
                sorted.sort_unstable();
                let percentile = |numerator: usize| {
                    (!sorted.is_empty()).then(|| sorted[(sorted.len() - 1) * numerator / 100])
                };
                // Past five digits the count would break the column
                // alignment, so it saturates instead.
                let frame_count = if self.total_frame_count > 99_999 {
                    "LOTS".to_string()
                } else {
                    self.total_frame_count.to_string()
                };
                // Labels are padded to a uniform width so the fixed-width
                // durations start in the same column on every line.
                let work = self.work;
                vec![
                    format!("CUR {}", format_ms(current)),
                    format!("1%  {}", format_ms(percentile(99))),
                    format!("10% {}", format_ms(percentile(90))),
                    format!("MAX {}", format_ms(sorted.last().copied())),
                    format!("FORM {}", format_ms(Some(work.layout))),
                    format!("PREP {}", format_ms(Some(work.prepaint))),
                    format!("PANT {}", format_ms(Some(work.paint))),
                    format!(
                        "CACHE {:>2}/{:<2}",
                        work.rendered,
                        work.rendered + work.reused
                    ),
                    format!("VIEWS {:>5}", work.views),
                    format!("DIRTY {:>5}", work.dirty),
                    format!("REFRESH {:>3}", u32::from(work.refreshing)),
                    format!("FRAMES {frame_count:>5}"),
                ]
            }
        }
    }
}

/// Formats as `abc.d MS`, right-aligned in room for three integer digits and
/// one decimal (padded with spaces, not zeroes), so stacked readouts align.
fn format_ms(duration: Option<Duration>) -> String {
    match duration {
        Some(duration) => {
            let ms = duration.as_secs_f32() * 1000.0;
            format!("{ms:>5.1} MS")
        }
        None => "   -- MS".into(),
    }
}

fn flag_from_env(name: &str) -> bool {
    env::var(name)
        .map(|value| value == "1" || value == "true")
        .unwrap_or(false)
}

fn mode_from_env() -> DebugFrameOverlayMode {
    match env::var("GPUI_DEBUG_OVERLAY").as_deref() {
        Ok("minimal") | Ok("1") => DebugFrameOverlayMode::Minimal,
        Ok("full") => DebugFrameOverlayMode::Full,
        _ => DebugFrameOverlayMode::Hidden,
    }
}

fn scaled_bounds(left: f32, top: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(left), ScaledPixels(top)),
        size: size(ScaledPixels(width), ScaledPixels(height)),
    }
}

fn solid_quad(
    bounds: Bounds<ScaledPixels>,
    content_mask: &ContentMask<ScaledPixels>,
    color: Hsla,
) -> Quad {
    Quad {
        order: 0,
        border_style: BorderStyle::Solid,
        bounds,
        content_mask: *content_mask,
        background: color.into(),
        border_color: transparent_black(),
        corner_radii: Corners::default(),
        border_widths: Edges::default(),
        corner_smoothing: 0.,
        pad: 0,
    }
}

/// Returns the 5x7 bitmap for the given character, one `u8` of column bits per
/// row with the most significant of the 5 bits leftmost. Only the characters
/// used by the overlay's readouts are defined.
fn glyph(character: char) -> Option<[u8; GLYPH_HEIGHT]> {
    Some(match character {
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '%' => [
            0b11001, 0b11001, 0b00010, 0b00100, 0b01000, 0b10011, 0b10011,
        ],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character the readouts can produce must have a glyph, or it
    /// would silently render as blank space.
    #[test]
    fn all_rendered_characters_have_glyphs() {
        let mut overlay = DebugFrameOverlay::new();
        overlay.set_mode(DebugFrameOverlayMode::Full);

        let mut lines = Vec::new();
        for duration in [
            Duration::ZERO,
            Duration::from_micros(1),
            Duration::from_micros(8_333),
            Duration::from_millis(123),
            Duration::from_secs(2),
        ] {
            overlay.record_frame(duration);
            lines.extend(overlay.lines());
        }
        // Counts beyond five digits render as "LOTS".
        overlay.total_frame_count = 100_000;
        lines.extend(overlay.lines());

        // An enabled overlay with no samples yet renders placeholders.
        let mut empty = DebugFrameOverlay::new();
        empty.set_mode(DebugFrameOverlayMode::Full);
        lines.extend(empty.lines());

        for line in lines {
            for character in line.chars() {
                assert!(
                    character == ' ' || glyph(character).is_some(),
                    "no glyph for {character:?} in line {line:?}"
                );
            }
        }
    }

    #[test]
    fn percentile_lows_are_reported_as_times() {
        let mut overlay = DebugFrameOverlay::new();
        overlay.set_mode(DebugFrameOverlayMode::Full);
        for milliseconds in 1..=100 {
            overlay.record_frame(Duration::from_millis(milliseconds));
        }
        let lines = overlay.lines();
        assert_eq!(lines[0], "CUR 100.0 MS");
        assert_eq!(lines[1], "1%   99.0 MS");
        assert_eq!(lines[2], "10%  90.0 MS");
        assert_eq!(lines[3], "MAX 100.0 MS");
        assert_eq!(lines[4], "FRAMES   100");
    }

    #[test]
    fn reset_clears_durations_but_keeps_frame_count() {
        let mut overlay = DebugFrameOverlay::new();
        overlay.set_mode(DebugFrameOverlayMode::Full);
        for _ in 0..10 {
            overlay.record_frame(Duration::from_millis(10));
        }
        overlay.reset_stats();
        let lines = overlay.lines();
        assert_eq!(lines[0], "CUR    -- MS");
        assert_eq!(lines[3], "MAX    -- MS");
        assert_eq!(lines[4], "FRAMES    10");
        overlay.record_frame(Duration::from_millis(20));
        let lines = overlay.lines();
        assert_eq!(lines[0], "CUR  20.0 MS");
        assert_eq!(lines[4], "FRAMES    11");
    }

    #[test]
    fn frame_count_accumulates_across_mode_changes() {
        let mut overlay = DebugFrameOverlay::new();
        for _ in 0..3 {
            overlay.record_frame(Duration::from_millis(10));
        }
        overlay.set_mode(DebugFrameOverlayMode::Minimal);
        assert_eq!(overlay.lines(), vec![" 10.0 MS".to_string()]);
        overlay.set_mode(DebugFrameOverlayMode::Full);
        overlay.record_frame(Duration::from_millis(10));
        assert_eq!(overlay.lines()[4], "FRAMES     4");
        overlay.set_mode(DebugFrameOverlayMode::Hidden);
        overlay.record_frame(Duration::from_millis(10));
        overlay.set_mode(DebugFrameOverlayMode::Full);
        assert_eq!(overlay.lines()[4], "FRAMES     5");
    }

    #[test]
    fn frame_count_is_right_aligned_and_saturates() {
        let mut overlay = DebugFrameOverlay::new();
        overlay.set_mode(DebugFrameOverlayMode::Full);
        overlay.record_frame(Duration::from_millis(10));
        assert_eq!(overlay.lines()[4], "FRAMES     1");
        overlay.total_frame_count = 99_999;
        assert_eq!(overlay.lines()[4], "FRAMES 99999");
        overlay.total_frame_count = 100_000;
        assert_eq!(overlay.lines()[4], "FRAMES  LOTS");
    }

    #[test]
    fn toggling_on_shows_previous_frame_immediately() {
        let mut overlay = DebugFrameOverlay::new();
        overlay.record_frame(Duration::from_millis(10));
        overlay.set_mode(DebugFrameOverlayMode::Minimal);
        assert_eq!(overlay.lines(), vec![" 10.0 MS".to_string()]);
    }
}
