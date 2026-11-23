use crate::{
    FontId, GlyphId, LetterSpacing, Pixels, PlatformTextSystem, Point, SharedString, Size, point,
    px,
};
use collections::FxHashMap;
use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard};
use smallvec::SmallVec;
use std::{
    borrow::Borrow,
    hash::{Hash, Hasher},
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::LineWrapper;

/// A laid out and styled line of text
#[derive(Default, Debug)]
pub struct LineLayout {
    /// The font size for this line
    pub font_size: Pixels,
    /// The width of the line
    pub width: Pixels,
    /// The ascent of the line
    pub ascent: Pixels,
    /// The descent of the line
    pub descent: Pixels,
    /// The shaped runs that make up this line
    pub runs: Vec<ShapedRun>,
    /// The length of the line in utf-8 bytes
    pub len: usize,
}

/// A run of text that has been shaped .
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// The font id for this run
    pub font_id: FontId,
    /// The glyphs that make up this run
    pub glyphs: Vec<ShapedGlyph>,
}

/// A single glyph, ready to paint.
#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    /// The ID for this glyph, as determined by the text system.
    pub id: GlyphId,

    /// The position of this glyph in its containing line.
    pub position: Point<Pixels>,

    /// The index of this glyph in the original text.
    pub index: usize,

    /// Whether this glyph is an emoji
    pub is_emoji: bool,
}

/// Where the glyphs of a layout fall once it is wrapped, for a layout whose glyphs do not sit left
/// to right in reading order. Glyphs are numbered in reading order across all runs.
pub(crate) struct BidiLines {
    /// Each glyph's run and its place in that run.
    pub(crate) glyphs: Vec<(usize, usize)>,
    /// Each glyph's left edge, from the left edge of the wrapped line it falls on.
    pub(crate) x: Vec<Pixels>,
    /// How much of the line each glyph covers.
    pub(crate) advance: Vec<Pixels>,
    /// The glyphs on each wrapped line, and how wide that line is.
    pub(crate) lines: Vec<(Range<usize>, Pixels)>,
}

/// The glyphs drawn for one or more characters, as one piece of a line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cluster {
    /// The byte index of the first character the cluster draws.
    index: usize,
    left: Pixels,
    right: Pixels,
    /// Whether the cluster is read right to left, which puts the caret before it on its right
    /// edge.
    rtl: bool,
}

impl LineLayout {
    /// The index for the character at the given x coordinate
    pub fn index_for_x(&self, x: Pixels) -> Option<usize> {
        if x >= self.width {
            None
        } else if !self.reads_left_to_right() {
            Some(cluster_at(&self.reading_clusters(), x))
        } else {
            for run in self.runs.iter().rev() {
                for glyph in run.glyphs.iter().rev() {
                    if glyph.position.x <= x {
                        return Some(glyph.index);
                    }
                }
            }
            Some(0)
        }
    }

    /// closest_index_for_x returns the character boundary closest to the given x coordinate
    /// (e.g. to handle aligning up/down arrow keys)
    pub fn closest_index_for_x(&self, x: Pixels) -> usize {
        if !self.reads_left_to_right() {
            return closest_caret(&self.reading_clusters(), x, self.len);
        }
        let mut prev_index = 0;
        let mut prev_x = px(0.);

        for run in self.runs.iter() {
            for glyph in run.glyphs.iter() {
                if glyph.position.x >= x {
                    if glyph.position.x - x < x - prev_x {
                        return glyph.index;
                    } else {
                        return prev_index;
                    }
                }
                prev_index = glyph.index;
                prev_x = glyph.position.x;
            }
        }

        if self.len == 1 {
            if x > self.width / 2. {
                return 1;
            } else {
                return 0;
            }
        }

        self.len
    }

    /// The x position of the character at the given index
    pub fn x_for_index(&self, index: usize) -> Pixels {
        if !self.reads_left_to_right() {
            return caret_x(&self.reading_clusters(), index, self.width);
        }
        for run in &self.runs {
            for glyph in &run.glyphs {
                if glyph.index >= index {
                    return glyph.position.x;
                }
            }
        }
        self.width
    }

    /// Whether each glyph sits at or right of the one before it and draws a character at or after
    /// it, as in left-to-right text. Right-to-left text breaks one of the two, depending on whether
    /// the shaper hands its glyphs over in reading order or in visual order, and the layout is then
    /// measured by where its glyphs sit rather than by the order they come in.
    pub fn reads_left_to_right(&self) -> bool {
        let mut last = (0, Pixels::MIN);
        self.runs.iter().flat_map(|run| &run.glyphs).all(|glyph| {
            let ordered = glyph.index >= last.0 && glyph.position.x >= last.1;
            last = (glyph.index, glyph.position.x);
            ordered
        })
    }

    /// Each glyph's run and its place in that run, in reading order. Shapers hand right-to-left
    /// glyphs over in visual order or in a mix of both, so the character each glyph draws decides
    /// the order, and glyphs drawing the same character keep the order they were shaped in.
    fn reading_order(&self) -> Vec<(usize, usize)> {
        let mut glyphs = self
            .runs
            .iter()
            .enumerate()
            .flat_map(|(run_ix, run)| (0..run.glyphs.len()).map(move |glyph_ix| (run_ix, glyph_ix)))
            .collect::<Vec<_>>();
        glyphs.sort_by_key(|&(run_ix, glyph_ix)| self.runs[run_ix].glyphs[glyph_ix].index);
        glyphs
    }

    /// Where each glyph falls once this layout is wrapped at `boundaries`, for a layout that does
    /// not read left to right. Each glyph covers the line up to the next glyph to its right, and a
    /// wrapped line lays the glyphs between two boundaries side by side in the order they sat
    /// unwrapped, so right-to-left text still reads from the right on every line.
    pub(crate) fn bidi_lines(&self, boundaries: &[WrapBoundary]) -> BidiLines {
        let glyphs = self.reading_order();
        let x_of = |flat: usize| {
            let (run_ix, glyph_ix) = glyphs[flat];
            self.runs[run_ix].glyphs[glyph_ix].position.x
        };

        let mut order = (0..glyphs.len()).collect::<Vec<_>>();
        order.sort_by_key(|&flat| x_of(flat));
        let mut advance = vec![Pixels::ZERO; glyphs.len()];
        for (at, &flat) in order.iter().enumerate() {
            let right = order.get(at + 1).map_or(self.width, |&next| x_of(next));
            advance[flat] = (right - x_of(flat)).max(Pixels::ZERO);
        }

        let mut starts = vec![0];
        starts.extend(boundaries.iter().filter_map(|boundary| {
            glyphs
                .iter()
                .position(|&glyph| glyph == (boundary.run_ix, boundary.glyph_ix))
        }));
        starts.sort_unstable();
        starts.dedup();
        let mut x = vec![Pixels::ZERO; glyphs.len()];
        let mut widths = vec![Pixels::ZERO; starts.len()];
        for &flat in &order {
            let line = starts.partition_point(|&start| start <= flat) - 1;
            x[flat] = widths[line];
            widths[line] += advance[flat];
        }
        let ends = starts.iter().skip(1).copied().chain([glyphs.len()]);
        let lines = starts
            .iter()
            .copied()
            .zip(ends)
            .zip(widths)
            .map(|((start, end), width)| (start..end, width))
            .collect();

        BidiLines {
            glyphs,
            x,
            advance,
            lines,
        }
    }

    /// The clusters of this layout as it sits unwrapped, in reading order.
    fn reading_clusters(&self) -> Vec<Cluster> {
        let lines = self.bidi_lines(&[]);
        clusters(self, &lines, 0..lines.glyphs.len())
    }

    /// The corresponding Font at the given index
    pub fn font_id_for_index(&self, index: usize) -> Option<FontId> {
        for run in &self.runs {
            for glyph in &run.glyphs {
                if glyph.index >= index {
                    return Some(run.font_id);
                }
            }
        }
        None
    }

    /// Split this layout at a byte index, returning `(prefix, suffix)`.
    ///
    /// - `prefix` contains glyphs for bytes `[0, byte_index)` with original positions.
    ///   Its width equals the x-advance up to the split point.
    /// - `suffix` contains glyphs for bytes `[byte_index, len)` with positions
    ///   shifted left so the first glyph starts at x=0, and byte indices rebased to 0.
    /// - `font_size`, `ascent`, and `descent` are copied to both halves.
    pub fn split_at(&self, byte_index: usize) -> (LineLayout, LineLayout) {
        let x_offset = self.x_for_index(byte_index);

        // Partition glyph runs. A single run may contribute glyphs to both halves.
        let mut left_runs = Vec::new();
        let mut right_runs = Vec::new();

        for run in &self.runs {
            let split_pos = run.glyphs.partition_point(|g| g.index < byte_index);

            if split_pos > 0 {
                left_runs.push(ShapedRun {
                    font_id: run.font_id,
                    glyphs: run.glyphs[..split_pos].to_vec(),
                });
            }

            if split_pos < run.glyphs.len() {
                let right_glyphs = run.glyphs[split_pos..]
                    .iter()
                    .map(|g| ShapedGlyph {
                        id: g.id,
                        position: point(g.position.x - x_offset, g.position.y),
                        index: g.index - byte_index,
                        is_emoji: g.is_emoji,
                    })
                    .collect();
                right_runs.push(ShapedRun {
                    font_id: run.font_id,
                    glyphs: right_glyphs,
                });
            }
        }

        let left = LineLayout {
            font_size: self.font_size,
            width: x_offset,
            ascent: self.ascent,
            descent: self.descent,
            runs: left_runs,
            len: byte_index,
        };

        let right = LineLayout {
            font_size: self.font_size,
            width: self.width - x_offset,
            ascent: self.ascent,
            descent: self.descent,
            runs: right_runs,
            len: self.len - byte_index,
        };

        (left, right)
    }

    fn compute_wrap_boundaries(
        &self,
        text: &str,
        wrap_width: Pixels,
        max_lines: Option<usize>,
    ) -> SmallVec<[WrapBoundary; 1]> {
        let mut boundaries = SmallVec::new();
        let mut first_non_whitespace_ix = None;
        let mut last_candidate_ix = None;
        let mut last_candidate_x = px(0.);
        let mut last_boundary_at = 0;
        let mut last_boundary_x = px(0.);
        let mut prev_ch = '\0';
        // right-to-left glyphs sit out of reading order, so they are walked in reading order, each
        // measured by how much of the line it covers and laid end to end
        let (order, end) = match self.reads_left_to_right() {
            true => (
                self.runs
                    .iter()
                    .enumerate()
                    .flat_map(|(run_ix, run)| {
                        run.glyphs.iter().enumerate().map(move |(glyph_ix, glyph)| {
                            (WrapBoundary { run_ix, glyph_ix }, glyph.position.x)
                        })
                    })
                    .collect::<Vec<_>>(),
                self.width,
            ),
            false => {
                let lines = self.bidi_lines(&[]);
                let mut at = Pixels::ZERO;
                let order = lines
                    .glyphs
                    .iter()
                    .zip(&lines.advance)
                    .map(|(&(run_ix, glyph_ix), &advance)| {
                        let x = at;
                        at += advance;
                        (WrapBoundary { run_ix, glyph_ix }, x)
                    })
                    .collect::<Vec<_>>();
                (order, at)
            }
        };
        let mut glyphs = order
            .into_iter()
            .enumerate()
            .map(|(at, (boundary, x))| {
                let index = self.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index;
                let character = text[index..].chars().next().unwrap();
                (at, boundary, character, x)
            })
            .peekable();

        while let Some((at, boundary, ch, x)) = glyphs.next() {
            if ch == '\n' {
                continue;
            }

            // Here is very similar to `LineWrapper::wrap_line` to determine text wrapping,
            // but there are some differences, so we have to duplicate the code here.
            if LineWrapper::is_word_char(ch) {
                if prev_ch == ' ' && ch != ' ' && first_non_whitespace_ix.is_some() {
                    last_candidate_ix = Some((boundary, at));
                    last_candidate_x = x;
                }
            } else {
                if ch != ' ' && first_non_whitespace_ix.is_some() {
                    last_candidate_ix = Some((boundary, at));
                    last_candidate_x = x;
                }
            }

            if ch != ' ' && first_non_whitespace_ix.is_none() {
                first_non_whitespace_ix = Some(boundary);
            }

            let next_x = glyphs.peek().map_or(end, |(_, _, _, x)| *x);
            let width = next_x - last_boundary_x;

            if width > wrap_width && at > last_boundary_at {
                // When used line_clamp, we should limit the number of lines.
                if let Some(max_lines) = max_lines
                    && boundaries.len() >= max_lines.saturating_sub(1)
                {
                    break;
                }

                let (wrap_at, wrap_at_x) = match last_candidate_ix.take() {
                    Some((candidate, candidate_at)) => {
                        last_boundary_at = candidate_at;
                        (candidate, last_candidate_x)
                    }
                    None => {
                        last_boundary_at = at;
                        (boundary, x)
                    }
                };
                last_boundary_x = wrap_at_x;
                boundaries.push(wrap_at);
            }
            prev_ch = ch;
        }

        boundaries
    }
}

/// A line of text that has been wrapped to fit a given width
#[derive(Default, Debug)]
pub struct WrappedLineLayout {
    /// The line layout, pre-wrapping.
    pub unwrapped_layout: Arc<LineLayout>,

    /// The boundaries at which the line was wrapped
    pub wrap_boundaries: SmallVec<[WrapBoundary; 1]>,

    /// The width of the line, if it was wrapped
    pub wrap_width: Option<Pixels>,
}

/// A boundary at which a line was wrapped
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WrapBoundary {
    /// The index in the run just before the line was wrapped
    pub run_ix: usize,
    /// The index of the glyph just before the line was wrapped
    pub glyph_ix: usize,
}

impl WrappedLineLayout {
    /// The length of the underlying text, in utf8 bytes.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.unwrapped_layout.len
    }

    /// The width of this line, in pixels, whether or not it was wrapped.
    pub fn width(&self) -> Pixels {
        self.wrap_width
            .unwrap_or(Pixels::MAX)
            .min(self.unwrapped_layout.width)
    }

    /// The size of the whole wrapped text, for the given line_height.
    /// can span multiple lines if there are multiple wrap boundaries.
    pub fn size(&self, line_height: Pixels) -> Size<Pixels> {
        Size {
            width: self.width(),
            height: line_height * (self.wrap_boundaries.len() + 1),
        }
    }

    /// The ascent of a line in this layout
    pub fn ascent(&self) -> Pixels {
        self.unwrapped_layout.ascent
    }

    /// The descent of a line in this layout
    pub fn descent(&self) -> Pixels {
        self.unwrapped_layout.descent
    }

    /// The wrap boundaries in this layout
    pub fn wrap_boundaries(&self) -> &[WrapBoundary] {
        &self.wrap_boundaries
    }

    /// The font size of this layout
    pub fn font_size(&self) -> Pixels {
        self.unwrapped_layout.font_size
    }

    /// The runs in this layout, sans wrapping
    pub fn runs(&self) -> &[ShapedRun] {
        &self.unwrapped_layout.runs
    }

    /// The index corresponding to a given position in this layout for the given line height.
    ///
    /// See also [`Self::closest_index_for_position`].
    pub fn index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        self._index_for_position(position, line_height, false)
    }

    /// The closest index to a given position in this layout for the given line height.
    ///
    /// Closest means the character boundary closest to the given position.
    ///
    /// See also [`LineLayout::closest_index_for_x`].
    pub fn closest_index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        self._index_for_position(position, line_height, true)
    }

    fn _index_for_position(
        &self,
        mut position: Point<Pixels>,
        line_height: Pixels,
        closest: bool,
    ) -> Result<usize, usize> {
        let wrapped_line_ix = (position.y / line_height) as usize;

        let wrapped_line_start_index;
        let wrapped_line_start_x;
        if wrapped_line_ix > 0 {
            let Some(line_start_boundary) = self.wrap_boundaries.get(wrapped_line_ix - 1) else {
                return Err(0);
            };
            let run = &self.unwrapped_layout.runs[line_start_boundary.run_ix];
            let glyph = &run.glyphs[line_start_boundary.glyph_ix];
            wrapped_line_start_index = glyph.index;
            wrapped_line_start_x = glyph.position.x;
        } else {
            wrapped_line_start_index = 0;
            wrapped_line_start_x = Pixels::ZERO;
        };

        let wrapped_line_end_index;
        let wrapped_line_end_x;
        if wrapped_line_ix < self.wrap_boundaries.len() {
            let next_wrap_boundary_ix = wrapped_line_ix;
            let next_wrap_boundary = self.wrap_boundaries[next_wrap_boundary_ix];
            let run = &self.unwrapped_layout.runs[next_wrap_boundary.run_ix];
            let glyph = &run.glyphs[next_wrap_boundary.glyph_ix];
            wrapped_line_end_index = glyph.index;
            wrapped_line_end_x = glyph.position.x;
        } else {
            wrapped_line_end_index = self.unwrapped_layout.len;
            wrapped_line_end_x = self.unwrapped_layout.width;
        };

        let mut position_in_unwrapped_line = position;
        position_in_unwrapped_line.x += wrapped_line_start_x;
        if position_in_unwrapped_line.x < wrapped_line_start_x {
            Err(wrapped_line_start_index)
        } else if position_in_unwrapped_line.x >= wrapped_line_end_x {
            Err(wrapped_line_end_index)
        } else {
            if closest {
                Ok(self
                    .unwrapped_layout
                    .closest_index_for_x(position_in_unwrapped_line.x))
            } else {
                // The shaper can place a trailing zero-width wrap boundary glyph slightly past
                // the line's width, so the row can extend past where `index_for_x` has glyphs.
                self.unwrapped_layout
                    .index_for_x(position_in_unwrapped_line.x)
                    .ok_or(wrapped_line_end_index)
            }
        }
    }

    /// Returns the pixel position for the given byte index.
    pub fn position_for_index(&self, index: usize, line_height: Pixels) -> Option<Point<Pixels>> {
        let mut line_start_ix = 0;
        let mut line_end_indices = self
            .wrap_boundaries
            .iter()
            .map(|wrap_boundary| {
                let run = &self.unwrapped_layout.runs[wrap_boundary.run_ix];
                let glyph = &run.glyphs[wrap_boundary.glyph_ix];
                glyph.index
            })
            .chain([self.len()])
            .enumerate();
        for (ix, line_end_ix) in line_end_indices {
            let line_y = ix as f32 * line_height;
            if index < line_start_ix {
                break;
            } else if index > line_end_ix {
                line_start_ix = line_end_ix;
                continue;
            } else {
                let line_start_x = self.unwrapped_layout.x_for_index(line_start_ix);
                let x = self.unwrapped_layout.x_for_index(index) - line_start_x;
                return Some(point(x, line_y));
            }
        }

        None
    }
}

pub(crate) struct LineLayoutCache {
    previous_frame: Mutex<FrameCache>,
    current_frame: RwLock<FrameCache>,
    platform_text_system: Arc<dyn PlatformTextSystem>,
    /// Advances when [`TextSystem::add_fonts`] successfully changes the font database.
    font_generation: Arc<AtomicUsize>,
    /// Records the generation represented by both frame caches.
    cached_font_generation: AtomicUsize,
}

#[derive(Default)]
struct FrameCache {
    lines: FxHashMap<Arc<CacheKey>, Arc<LineLayout>>,
    wrapped_lines: FxHashMap<Arc<CacheKey>, Arc<WrappedLineLayout>>,
    used_lines: Vec<Arc<CacheKey>>,
    used_wrapped_lines: Vec<Arc<CacheKey>>,

    // Content-addressable caches keyed by caller-provided text hash + layout params.
    // These allow cache hits without materializing a contiguous `SharedString`.
    //
    // IMPORTANT: To support allocation-free lookups, we store these maps using a key type
    // (`HashedCacheKeyRef`) that can be computed without building a contiguous `&str`/`SharedString`.
    // On miss, we allocate once and store under an owned `HashedCacheKey`.
    lines_by_hash: FxHashMap<Arc<HashedCacheKey>, Arc<LineLayout>>,
    wrapped_lines_by_hash: FxHashMap<Arc<HashedCacheKey>, Arc<WrappedLineLayout>>,
    used_lines_by_hash: Vec<Arc<HashedCacheKey>>,
    used_wrapped_lines_by_hash: Vec<Arc<HashedCacheKey>>,
}

#[derive(Clone, Default)]
pub(crate) struct LineLayoutIndex {
    font_generation: usize,
    lines_index: usize,
    wrapped_lines_index: usize,
    lines_by_hash_index: usize,
    wrapped_lines_by_hash_index: usize,
}

impl LineLayoutCache {
    pub fn new(
        platform_text_system: Arc<dyn PlatformTextSystem>,
        font_generation: Arc<AtomicUsize>,
    ) -> Self {
        let cached_font_generation = font_generation.load(Ordering::Acquire);
        Self {
            previous_frame: Mutex::default(),
            current_frame: RwLock::default(),
            platform_text_system,
            font_generation,
            cached_font_generation: AtomicUsize::new(cached_font_generation),
        }
    }

    pub fn layout_index(&self) -> LineLayoutIndex {
        let font_generation = self.clear_if_font_generation_changed();
        let frame = self.current_frame.read();
        LineLayoutIndex {
            font_generation,
            lines_index: frame.used_lines.len(),
            wrapped_lines_index: frame.used_wrapped_lines.len(),
            lines_by_hash_index: frame.used_lines_by_hash.len(),
            wrapped_lines_by_hash_index: frame.used_wrapped_lines_by_hash.len(),
        }
    }

    pub fn reuse_layouts(&self, range: Range<LineLayoutIndex>) {
        let font_generation = self.clear_if_font_generation_changed();
        if range.start.font_generation != font_generation
            || range.end.font_generation != font_generation
        {
            return;
        }
        let mut current_frame = &mut *self.current_frame.write();
        let mut previous_frame = &mut *self.previous_frame.lock();

        for key in &previous_frame.used_lines[range.start.lines_index..range.end.lines_index] {
            if let Some((key, line)) = previous_frame.lines.remove_entry(key) {
                current_frame.lines.insert(key, line);
            }
            current_frame.used_lines.push(key.clone());
        }

        for key in &previous_frame.used_wrapped_lines
            [range.start.wrapped_lines_index..range.end.wrapped_lines_index]
        {
            if let Some((key, line)) = previous_frame.wrapped_lines.remove_entry(key) {
                current_frame.wrapped_lines.insert(key, line);
            }
            current_frame.used_wrapped_lines.push(key.clone());
        }

        for key in &previous_frame.used_lines_by_hash
            [range.start.lines_by_hash_index..range.end.lines_by_hash_index]
        {
            if let Some((key, line)) = previous_frame.lines_by_hash.remove_entry(key) {
                current_frame.lines_by_hash.insert(key, line);
            }
            current_frame.used_lines_by_hash.push(key.clone());
        }

        for key in &previous_frame.used_wrapped_lines_by_hash
            [range.start.wrapped_lines_by_hash_index..range.end.wrapped_lines_by_hash_index]
        {
            if let Some((key, line)) = previous_frame.wrapped_lines_by_hash.remove_entry(key) {
                current_frame.wrapped_lines_by_hash.insert(key, line);
            }
            current_frame.used_wrapped_lines_by_hash.push(key.clone());
        }
    }

    pub fn truncate_layouts(&self, index: LineLayoutIndex) {
        let font_generation = self.clear_if_font_generation_changed();
        if index.font_generation != font_generation {
            return;
        }
        let mut current_frame = &mut *self.current_frame.write();
        current_frame.used_lines.truncate(index.lines_index);
        current_frame
            .used_wrapped_lines
            .truncate(index.wrapped_lines_index);
        current_frame
            .used_lines_by_hash
            .truncate(index.lines_by_hash_index);
        current_frame
            .used_wrapped_lines_by_hash
            .truncate(index.wrapped_lines_by_hash_index);
    }

    pub fn finish_frame(&self) {
        let _font_generation = self.clear_if_font_generation_changed();
        let mut curr_frame = self.current_frame.write();
        let mut prev_frame = self.previous_frame.lock();
        std::mem::swap(&mut *prev_frame, &mut *curr_frame);
        curr_frame.lines.clear();
        curr_frame.wrapped_lines.clear();
        curr_frame.used_lines.clear();
        curr_frame.used_wrapped_lines.clear();

        curr_frame.lines_by_hash.clear();
        curr_frame.wrapped_lines_by_hash.clear();
        curr_frame.used_lines_by_hash.clear();
        curr_frame.used_wrapped_lines_by_hash.clear();
    }

    pub fn layout_wrapped_line<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        wrap_width: Option<Pixels>,
        max_lines: Option<usize>,
    ) -> Arc<WrappedLineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        let _font_generation = self.clear_if_font_generation_changed();
        let key = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width,
            force_width: None,
        } as &dyn AsCacheKeyRef;

        let current_frame = self.current_frame.upgradable_read();
        if let Some(layout) = current_frame.wrapped_lines.get(key) {
            return layout.clone();
        }

        let previous_frame_entry = self.previous_frame.lock().wrapped_lines.remove_entry(key);
        if let Some((key, layout)) = previous_frame_entry {
            let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
            current_frame
                .wrapped_lines
                .insert(key.clone(), layout.clone());
            current_frame.used_wrapped_lines.push(key);
            layout
        } else {
            drop(current_frame);
            let text = SharedString::from(text);
            let unwrapped_layout = self.layout_line::<&SharedString>(&text, font_size, runs, None);
            let wrap_boundaries = if let Some(wrap_width) = wrap_width {
                unwrapped_layout.compute_wrap_boundaries(text.as_ref(), wrap_width, max_lines)
            } else {
                SmallVec::new()
            };
            let layout = Arc::new(WrappedLineLayout {
                unwrapped_layout,
                wrap_boundaries,
                wrap_width,
            });
            let key = Arc::new(CacheKey {
                text,
                font_size,
                runs: SmallVec::from(runs),
                wrap_width,
                force_width: None,
            });

            let mut current_frame = self.current_frame.write();
            current_frame
                .wrapped_lines
                .insert(key.clone(), layout.clone());
            current_frame.used_wrapped_lines.push(key);

            layout
        }
    }

    pub fn layout_line<Text>(
        &self,
        text: Text,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
    ) -> Arc<LineLayout>
    where
        Text: AsRef<str>,
        SharedString: From<Text>,
    {
        let _font_generation = self.clear_if_font_generation_changed();
        let key = &CacheKeyRef {
            text: text.as_ref(),
            font_size,
            runs,
            wrap_width: None,
            force_width,
        } as &dyn AsCacheKeyRef;

        let current_frame = self.current_frame.upgradable_read();
        if let Some(layout) = current_frame.lines.get(key) {
            return layout.clone();
        }

        let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);
        if let Some((key, layout)) = self.previous_frame.lock().lines.remove_entry(key) {
            current_frame.lines.insert(key.clone(), layout.clone());
            current_frame.used_lines.push(key);
            layout
        } else {
            let text = SharedString::from(text);
            let mut layout = self
                .platform_text_system
                .layout_line(&text, font_size, runs);
            if let Some(force_width) = force_width {
                apply_force_width_to_layout(&mut layout, force_width);
            }
            apply_letter_spacing(&mut layout, runs);

            let key = Arc::new(CacheKey {
                text,
                font_size,
                runs: SmallVec::from(runs),
                wrap_width: None,
                force_width,
            });
            let layout = Arc::new(layout);
            current_frame.lines.insert(key.clone(), layout.clone());
            current_frame.used_lines.push(key);
            layout
        }
    }

    /// Try to retrieve a previously-shaped line layout using a caller-provided content hash.
    ///
    /// This is a *non-allocating* cache probe: it does not materialize any text. If the layout
    /// is not already cached in either the current frame or previous frame, returns `None`.
    ///
    /// Contract (caller enforced):
    /// - Same `text_hash` implies identical text content (collision risk accepted by caller).
    /// - `text_len` should be the UTF-8 byte length of the text (helps reduce accidental collisions).
    pub fn try_layout_line_by_hash(
        &self,
        text_hash: u64,
        text_len: usize,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
    ) -> Option<Arc<LineLayout>> {
        let _font_generation = self.clear_if_font_generation_changed();
        let key_ref = HashedCacheKeyRef {
            text_hash,
            text_len,
            font_size,
            runs,
            wrap_width: None,
            force_width,
        };

        let current_frame = self.current_frame.read();
        if let Some((_, layout)) = current_frame.lines_by_hash.iter().find(|(key, _)| {
            HashedCacheKeyRef {
                text_hash: key.text_hash,
                text_len: key.text_len,
                font_size: key.font_size,
                runs: key.runs.as_slice(),
                wrap_width: key.wrap_width,
                force_width: key.force_width,
            } == key_ref
        }) {
            return Some(layout.clone());
        }

        let previous_frame = self.previous_frame.lock();
        if let Some((_, layout)) = previous_frame.lines_by_hash.iter().find(|(key, _)| {
            HashedCacheKeyRef {
                text_hash: key.text_hash,
                text_len: key.text_len,
                font_size: key.font_size,
                runs: key.runs.as_slice(),
                wrap_width: key.wrap_width,
                force_width: key.force_width,
            } == key_ref
        }) {
            return Some(layout.clone());
        }

        None
    }

    /// Layout a line of text using a caller-provided content hash as the cache key.
    ///
    /// This enables cache hits without materializing a contiguous `SharedString` for `text`.
    /// If the cache misses, `materialize_text` is invoked to produce the `SharedString` for shaping.
    ///
    /// Contract (caller enforced):
    /// - Same `text_hash` implies identical text content (collision risk accepted by caller).
    /// - `text_len` should be the UTF-8 byte length of the text (helps reduce accidental collisions).
    pub fn layout_line_by_hash(
        &self,
        text_hash: u64,
        text_len: usize,
        font_size: Pixels,
        runs: &[FontRun],
        force_width: Option<Pixels>,
        materialize_text: impl FnOnce() -> SharedString,
    ) -> Arc<LineLayout> {
        let _font_generation = self.clear_if_font_generation_changed();
        let key_ref = HashedCacheKeyRef {
            text_hash,
            text_len,
            font_size,
            runs,
            wrap_width: None,
            force_width,
        };

        // Fast path: already cached (no allocation).
        let current_frame = self.current_frame.upgradable_read();
        if let Some((_, layout)) = current_frame.lines_by_hash.iter().find(|(key, _)| {
            HashedCacheKeyRef {
                text_hash: key.text_hash,
                text_len: key.text_len,
                font_size: key.font_size,
                runs: key.runs.as_slice(),
                wrap_width: key.wrap_width,
                force_width: key.force_width,
            } == key_ref
        }) {
            return layout.clone();
        }

        let mut current_frame = RwLockUpgradableReadGuard::upgrade(current_frame);

        // Try to reuse from previous frame without allocating; do a linear scan to find a matching key.
        // (We avoid `drain()` here because it would eagerly move all entries.)
        let mut previous_frame = self.previous_frame.lock();
        if let Some(existing_key) = previous_frame
            .used_lines_by_hash
            .iter()
            .find(|key| {
                HashedCacheKeyRef {
                    text_hash: key.text_hash,
                    text_len: key.text_len,
                    font_size: key.font_size,
                    runs: key.runs.as_slice(),
                    wrap_width: key.wrap_width,
                    force_width: key.force_width,
                } == key_ref
            })
            .cloned()
        {
            if let Some((key, layout)) = previous_frame.lines_by_hash.remove_entry(&existing_key) {
                current_frame
                    .lines_by_hash
                    .insert(key.clone(), layout.clone());
                current_frame.used_lines_by_hash.push(key);
                return layout;
            }
        }

        let text = materialize_text();
        let mut layout = self
            .platform_text_system
            .layout_line(&text, font_size, runs);
        if let Some(force_width) = force_width {
            apply_force_width_to_layout(&mut layout, force_width);
        }
        apply_letter_spacing(&mut layout, runs);

        let key = Arc::new(HashedCacheKey {
            text_hash,
            text_len,
            font_size,
            runs: SmallVec::from(runs),
            wrap_width: None,
            force_width,
        });
        let layout = Arc::new(layout);
        current_frame
            .lines_by_hash
            .insert(key.clone(), layout.clone());
        current_frame.used_lines_by_hash.push(key);
        layout
    }

    fn clear_if_font_generation_changed(&self) -> usize {
        let font_generation = self.font_generation.load(Ordering::Acquire);
        if self.cached_font_generation.load(Ordering::Acquire) == font_generation {
            return font_generation;
        }

        let mut current_frame = self.current_frame.write();
        if self.cached_font_generation.load(Ordering::Acquire) == font_generation {
            return font_generation;
        }

        *current_frame = FrameCache::default();
        *self.previous_frame.lock() = FrameCache::default();
        self.cached_font_generation
            .store(font_generation, Ordering::Release);
        font_generation
    }
}

// Combining marks (e.g. Thai vowel signs, Arabic diacritics) are shaped by
// HarfBuzz at the same x position as their base character. The force-width
// loop must not advance the cell counter for these zero-advance glyphs,
// otherwise they get displaced into the next cell. We detect them by checking
// whether shaped x has advanced by at least half a cell beyond the last base.
fn apply_force_width_to_layout(layout: &mut LineLayout, force_width: Pixels) {
    let mut glyph_pos: usize = 0;
    // NEG_INFINITY ensures the first glyph is always classified as a base.
    let mut last_base_shaped_x = px(f32::NEG_INFINITY);
    let mut last_base_actual_x = px(0.);

    for run in layout.runs.iter_mut() {
        for glyph in run.glyphs.iter_mut() {
            let shaped_x = glyph.position.x;

            if shaped_x > last_base_shaped_x + force_width * 0.5 {
                let forced_x = glyph_pos * force_width;
                if (shaped_x - forced_x).abs() > px(1.) {
                    glyph.position.x = forced_x;
                }
                last_base_shaped_x = shaped_x;
                last_base_actual_x = glyph.position.x;
                glyph_pos += 1;
            } else {
                glyph.position.x = last_base_actual_x + (shaped_x - last_base_shaped_x);
            }
        }
    }
}

/// A run of text with a single font.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[expect(missing_docs)]
pub struct FontRun {
    pub len: usize,
    pub font_id: FontId,
    pub letter_spacing: LetterSpacing,
}

/// Moves each glyph along by the letter spacing of the characters drawn to its left, and
/// widens the line by all of it. It walks the line in visual order, so right-to-left text
/// spreads apart as left-to-right text does. A cluster shaped into one glyph is spaced once.
fn apply_letter_spacing(layout: &mut LineLayout, runs: &[FontRun]) {
    if runs.iter().all(|run| run.letter_spacing.0 == 0.) {
        return;
    }
    let mut ends = Vec::with_capacity(runs.len());
    let mut end = 0;
    for run in runs {
        end += run.len;
        ends.push((end, layout.font_size * run.letter_spacing.0));
    }
    let spacing_at = |index: usize| {
        ends.iter()
            .find(|(end, _)| index < *end)
            .map_or(Pixels::ZERO, |(_, spacing)| *spacing)
    };
    // Where each cluster starts on screen, so the glyphs of one cluster stay together even
    // when a mark is drawn a little left of its base.
    let mut cluster_x = FxHashMap::<usize, Pixels>::default();
    for glyph in layout.runs.iter().flat_map(|run| &run.glyphs) {
        cluster_x
            .entry(glyph.index)
            .and_modify(|x| *x = (*x).min(glyph.position.x))
            .or_insert(glyph.position.x);
    }
    let mut glyphs: Vec<&mut ShapedGlyph> = layout
        .runs
        .iter_mut()
        .flat_map(|run| run.glyphs.iter_mut())
        .collect();
    glyphs.sort_by(|a, b| {
        cluster_x[&a.index]
            .partial_cmp(&cluster_x[&b.index])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.index.cmp(&b.index))
    });
    let mut offset = Pixels::ZERO;
    let mut previous = None;
    for glyph in glyphs {
        if previous != Some(glyph.index) {
            if let Some(index) = previous {
                offset += spacing_at(index);
            }
            previous = Some(glyph.index);
        }
        glyph.position.x += offset;
    }
    if let Some(index) = previous {
        offset += spacing_at(index);
    }
    layout.width += offset;
}

trait AsCacheKeyRef {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_>;
}

#[derive(Clone, Debug, Eq)]
struct CacheKey {
    text: SharedString,
    font_size: Pixels,
    runs: SmallVec<[FontRun; 1]>,
    wrap_width: Option<Pixels>,
    force_width: Option<Pixels>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct CacheKeyRef<'a> {
    text: &'a str,
    font_size: Pixels,
    runs: &'a [FontRun],
    wrap_width: Option<Pixels>,
    force_width: Option<Pixels>,
}

#[derive(Clone, Debug)]
struct HashedCacheKey {
    text_hash: u64,
    text_len: usize,
    font_size: Pixels,
    runs: SmallVec<[FontRun; 1]>,
    wrap_width: Option<Pixels>,
    force_width: Option<Pixels>,
}

#[derive(Copy, Clone)]
struct HashedCacheKeyRef<'a> {
    text_hash: u64,
    text_len: usize,
    font_size: Pixels,
    runs: &'a [FontRun],
    wrap_width: Option<Pixels>,
    force_width: Option<Pixels>,
}

impl PartialEq for dyn AsCacheKeyRef + '_ {
    fn eq(&self, other: &dyn AsCacheKeyRef) -> bool {
        self.as_cache_key_ref() == other.as_cache_key_ref()
    }
}

impl PartialEq for HashedCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.text_hash == other.text_hash
            && self.text_len == other.text_len
            && self.font_size == other.font_size
            && self.runs.as_slice() == other.runs.as_slice()
            && self.wrap_width == other.wrap_width
            && self.force_width == other.force_width
    }
}

impl Eq for HashedCacheKey {}

impl Hash for HashedCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text_hash.hash(state);
        self.text_len.hash(state);
        self.font_size.hash(state);
        self.runs.as_slice().hash(state);
        self.wrap_width.hash(state);
        self.force_width.hash(state);
    }
}

impl PartialEq for HashedCacheKeyRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.text_hash == other.text_hash
            && self.text_len == other.text_len
            && self.font_size == other.font_size
            && self.runs == other.runs
            && self.wrap_width == other.wrap_width
            && self.force_width == other.force_width
    }
}

impl Eq for HashedCacheKeyRef<'_> {}

impl Hash for HashedCacheKeyRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text_hash.hash(state);
        self.text_len.hash(state);
        self.font_size.hash(state);
        self.runs.hash(state);
        self.wrap_width.hash(state);
        self.force_width.hash(state);
    }
}

impl Eq for dyn AsCacheKeyRef + '_ {}

impl Hash for dyn AsCacheKeyRef + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_cache_key_ref().hash(state)
    }
}

impl AsCacheKeyRef for CacheKey {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_> {
        CacheKeyRef {
            text: &self.text,
            font_size: self.font_size,
            runs: self.runs.as_slice(),
            wrap_width: self.wrap_width,
            force_width: self.force_width,
        }
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.as_cache_key_ref().eq(&other.as_cache_key_ref())
    }
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_cache_key_ref().hash(state);
    }
}

impl<'a> Borrow<dyn AsCacheKeyRef + 'a> for Arc<CacheKey> {
    fn borrow(&self) -> &(dyn AsCacheKeyRef + 'a) {
        self.as_ref() as &dyn AsCacheKeyRef
    }
}

impl AsCacheKeyRef for CacheKeyRef<'_> {
    fn as_cache_key_ref(&self) -> CacheKeyRef<'_> {
        *self
    }
}

/// The clusters among `range` of `lines`, in reading order, each with the side it is read from.
fn clusters(layout: &LineLayout, lines: &BidiLines, range: Range<usize>) -> Vec<Cluster> {
    let mut clusters: Vec<Cluster> = Vec::new();
    for flat in range {
        let (run_ix, glyph_ix) = lines.glyphs[flat];
        let index = layout.runs[run_ix].glyphs[glyph_ix].index;
        let left = lines.x[flat];
        let right = left + lines.advance[flat];
        match clusters.last_mut() {
            Some(last) if last.index == index => {
                last.left = last.left.min(left);
                last.right = last.right.max(right);
            }
            _ => clusters.push(Cluster {
                index,
                left,
                right,
                rtl: false,
            }),
        }
    }

    // a cluster reads right to left when what is read after it sits to its left, or what is read
    // before it sits to its right
    let lefts = clusters
        .iter()
        .map(|cluster| cluster.left)
        .collect::<Vec<_>>();
    for (at, cluster) in clusters.iter_mut().enumerate() {
        let next = lefts.get(at + 1).is_some_and(|&next| next < cluster.left);
        let previous = at
            .checked_sub(1)
            .is_some_and(|before| lefts[before] > cluster.left);
        cluster.rtl = next || previous;
    }
    clusters
}

/// Where the caret before byte `index` sits among `clusters`: on the edge of the cluster it comes
/// before that is read first, or past the last cluster on its far side.
fn caret_x(clusters: &[Cluster], index: usize, end: Pixels) -> Pixels {
    let edge = |cluster: &Cluster, before: bool| match cluster.rtl == before {
        true => cluster.right,
        false => cluster.left,
    };
    match clusters.iter().find(|cluster| cluster.index >= index) {
        Some(cluster) => edge(cluster, true),
        None => clusters.last().map_or(end, |last| edge(last, false)),
    }
}

/// The byte index of the caret position nearest `x`. Each cluster offers the caret before it and
/// the one after it, on its two edges.
fn closest_caret(clusters: &[Cluster], x: Pixels, len: usize) -> usize {
    let mut closest = (len, Pixels::MAX);
    for (at, cluster) in clusters.iter().enumerate() {
        let after = clusters.get(at + 1).map_or(len, |next| next.index);
        let (before_x, after_x) = match cluster.rtl {
            true => (cluster.right, cluster.left),
            false => (cluster.left, cluster.right),
        };
        for (index, edge) in [(cluster.index, before_x), (after, after_x)] {
            let distance = (edge - x).abs();
            if distance < closest.1 {
                closest = (index, distance);
            }
        }
    }
    closest.0
}

/// The byte index of the cluster under `x`, or of the nearest one when `x` falls between them.
fn cluster_at(clusters: &[Cluster], x: Pixels) -> usize {
    clusters
        .iter()
        .find(|cluster| cluster.left <= x && x < cluster.right)
        .or_else(|| {
            clusters
                .iter()
                .min_by_key(|cluster| (cluster.left - x).abs().min((cluster.right - x).abs()))
        })
        .map_or(0, |cluster| cluster.index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GlyphId;

    fn glyph_at(x: f32, index: usize) -> ShapedGlyph {
        ShapedGlyph {
            id: GlyphId(0),
            position: point(px(x), px(0.)),
            index,
            is_emoji: false,
        }
    }

    fn make_layout(glyphs: Vec<ShapedGlyph>) -> LineLayout {
        LineLayout {
            font_size: px(16.),
            width: px(100.),
            ascent: px(12.),
            descent: px(4.),
            runs: vec![ShapedRun {
                font_id: FontId(0),
                glyphs,
            }],
            len: 0,
        }
    }

    fn glyph_x_positions(layout: &LineLayout) -> Vec<f32> {
        layout.runs[0]
            .glyphs
            .iter()
            .map(|g| f32::from(g.position.x))
            .collect()
    }

    #[test]
    fn test_force_width_latin_unchanged() {
        let cell_width = px(8.);
        let mut layout = make_layout(vec![glyph_at(0., 0), glyph_at(8., 1), glyph_at(16., 2)]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0., 8., 16.]);
    }

    #[test]
    fn test_force_width_combining_marks_not_advanced() {
        let cell_width = px(8.);
        // Simulates Thai "กี" — base consonant at x=0, combining vowel also at x=0
        let mut layout = make_layout(vec![
            glyph_at(0., 0), // ก (base)
            glyph_at(0., 3), // ี (combining mark, same x)
        ]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0., 0.]);
    }

    #[test]
    fn test_force_width_base_after_combining_mark() {
        let cell_width = px(8.);
        let mut layout = make_layout(vec![glyph_at(0., 0), glyph_at(0., 3), glyph_at(8., 6)]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0., 0., 8.]);
    }

    #[test]
    fn test_force_width_multiple_combining_marks() {
        let cell_width = px(8.);
        // Simulates "ก้" — base + vowel + tone mark (two combining marks stacked)
        let mut layout = make_layout(vec![
            glyph_at(0., 0), // ก (base)
            glyph_at(0., 3), // vowel (combining)
            glyph_at(0., 6), // tone mark (combining)
            glyph_at(8., 9), // next base
        ]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0., 0., 0., 8.]);
    }

    #[test]
    fn test_force_width_corrects_drifted_base_positions() {
        let cell_width = px(8.);
        // Font metrics don't perfectly match cell grid — glyphs drift >1px from cell boundary
        let mut layout = make_layout(vec![
            glyph_at(0.5, 0),  // within 1px tolerance, kept as-is
            glyph_at(10.2, 1), // >1px off from 8.0, corrected
            glyph_at(19.8, 2), // >1px off from 16.0, corrected
        ]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0.5, 8., 16.]);
    }

    #[test]
    fn test_force_width_combining_mark_after_within_tolerance_base() {
        let cell_width = px(8.);
        // Base glyph is within 1px of grid so it keeps its shaped position.
        // The combining mark must align to the base's actual position, not the grid slot.
        let mut layout = make_layout(vec![glyph_at(0.5, 0), glyph_at(0.5, 3)]);

        apply_force_width_to_layout(&mut layout, cell_width);

        let positions = glyph_x_positions(&layout);
        assert_eq!(positions, vec![0.5, 0.5]);
    }

    /// Four two-byte letters read right to left, each 10px wide, placed the way DirectWrite
    /// places them: the first letter read sits at the right.
    fn right_to_left() -> LineLayout {
        LineLayout {
            width: px(40.),
            len: 8,
            ..make_layout(vec![
                glyph_at(30., 0),
                glyph_at(20., 2),
                glyph_at(10., 4),
                glyph_at(0., 6),
            ])
        }
    }

    fn run(len: usize, ems: f32) -> FontRun {
        FontRun {
            len,
            font_id: FontId(0),
            letter_spacing: LetterSpacing(ems),
        }
    }

    #[test]
    fn left_to_right_text_keeps_its_own_measures() {
        let layout = make_layout(vec![glyph_at(0., 0), glyph_at(8., 1), glyph_at(16., 2)]);
        assert!(layout.reads_left_to_right());
        assert_eq!(layout.x_for_index(1), px(8.));
    }

    #[test]
    fn right_to_left_carets_sit_on_the_right_of_what_follows_them() {
        let layout = right_to_left();
        assert!(!layout.reads_left_to_right());

        assert_eq!(layout.x_for_index(0), px(40.));
        assert_eq!(layout.x_for_index(2), px(30.));
        assert_eq!(layout.x_for_index(8), px(0.));

        assert_eq!(layout.closest_index_for_x(px(39.)), 0);
        assert_eq!(layout.closest_index_for_x(px(21.)), 4);
        assert_eq!(layout.closest_index_for_x(px(1.)), 8);

        assert_eq!(layout.index_for_x(px(35.)), Some(0));
        assert_eq!(layout.index_for_x(px(5.)), Some(6));
    }

    #[test]
    fn a_caret_among_right_to_left_letters_inside_left_to_right_text() {
        // two right-to-left letters, a space and two latin ones: the two letters sit swapped
        let layout = LineLayout {
            width: px(50.),
            len: 7,
            ..make_layout(vec![
                glyph_at(10., 0),
                glyph_at(0., 2),
                glyph_at(20., 4),
                glyph_at(30., 5),
                glyph_at(40., 6),
            ])
        };

        assert_eq!(layout.x_for_index(0), px(20.));
        assert_eq!(layout.x_for_index(2), px(10.));
        assert_eq!(layout.x_for_index(5), px(30.));
        assert_eq!(layout.x_for_index(7), px(50.));
    }

    #[test]
    fn wrapped_right_to_left_lines_keep_reading_from_the_right() {
        let layout = right_to_left();
        let lines = layout.bidi_lines(&[WrapBoundary {
            run_ix: 0,
            glyph_ix: 2,
        }]);

        assert_eq!(lines.lines, vec![(0..2, px(20.)), (2..4, px(20.))]);
        // the first letter read on each line sits at that line's right
        assert_eq!(lines.x, vec![px(10.), px(0.), px(10.), px(0.)]);
    }

    #[test]
    fn right_to_left_text_wraps_between_its_words() {
        // "אב גד": two Hebrew words, each letter 10px, read from the right
        let layout = LineLayout {
            width: px(50.),
            len: 9,
            ..make_layout(vec![
                glyph_at(40., 0),
                glyph_at(30., 2),
                glyph_at(20., 4),
                glyph_at(10., 5),
                glyph_at(0., 7),
            ])
        };

        let boundaries = layout.compute_wrap_boundaries("אב גד", px(35.), None);
        assert_eq!(
            boundaries.as_slice(),
            &[WrapBoundary {
                run_ix: 0,
                glyph_ix: 3
            }]
        );
    }

    #[test]
    fn test_letter_spacing_follows_each_character_in_its_run() {
        // 16px text, the first two characters spaced 0.25em (4px), the third not.
        let mut layout = make_layout(vec![glyph_at(0., 0), glyph_at(8., 1), glyph_at(16., 2)]);

        apply_letter_spacing(&mut layout, &[run(2, 0.25), run(1, 0.)]);

        assert_eq!(glyph_x_positions(&layout), vec![0., 12., 24.]);
        assert_eq!(layout.width, px(108.));
    }

    #[test]
    fn test_letter_spacing_spreads_right_to_left_text() {
        // Right-to-left: the first character is drawn rightmost. 0.5em is 8px.
        let mut layout = make_layout(vec![glyph_at(16., 0), glyph_at(8., 1), glyph_at(0., 2)]);

        apply_letter_spacing(&mut layout, &[run(3, 0.5)]);

        assert_eq!(glyph_x_positions(&layout), vec![32., 16., 0.]);
        assert_eq!(layout.width, px(124.));
    }

    #[test]
    fn test_letter_spacing_spaces_a_cluster_once() {
        // A base and its combining mark share an index and move together.
        let mut layout = make_layout(vec![glyph_at(0., 0), glyph_at(0., 0), glyph_at(8., 3)]);

        apply_letter_spacing(&mut layout, &[run(4, 0.5)]);

        assert_eq!(glyph_x_positions(&layout), vec![0., 0., 16.]);
        assert_eq!(layout.width, px(116.));
    }
}
