//! A text field of several lines, for text that comes a line at a time (the intake's links). It
//! grows from one line to `MAX_LINES`, then scrolls. Enter is left to the form around it (submit),
//! Shift+Enter breaks the line. Editing keys are the `Input` ones, through the same context.
//! With `set_notes` it draws as a bare sheet for a larger surface around it (the intake's
//! editor): numbered lines, a note at each line's end, wrong lines marked.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    App, AvailableSpace, Bounds, BoxShadow, ClipboardItem, Context, CursorStyle, Element,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, Font, GlobalElementId,
    InspectorElementId, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, PaintQuad, Pixels,
    Point, ScrollHandle, ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle,
    Window, WrappedLine, actions, div, fill, point, px, relative, size, svg,
};
use i18n::t;
use unicode_segmentation::UnicodeSegmentation as _;

use crate::button::Button;
use crate::input::{
    Backspace, BackspaceWord, Copy, Cut, Delete, End, Home, INPUT_CONTEXT, Left, Paste, Right,
    SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight, Space, WordLeft, WordRight,
};
use crate::ledger::MONO;
use crate::theme::ActiveTheme as _;

actions!(
    text_area,
    [
        /// Breaks the line at the caret (Shift+Enter).
        Newline,
        LineUp,
        LineDown,
        SelectLineUp,
        SelectLineDown
    ]
);

/// The key context a text area adds to `Input`'s, for the keys only it takes.
pub const TEXT_AREA_CONTEXT: &str = "TextArea";
/// How many lines the area grows to before it scrolls.
pub const MAX_LINES: f32 = 6.;
const CARET: Pixels = px(2.);
/// How much of the foreground the field takes as its fill and its edge, and its focus ring.
const FILL: f32 = 0.045;
const EDGE: f32 = 0.15;
const RING: f32 = 0.4;
const RING_WIDTH: Pixels = px(2.);
/// A sheet's line-number gutter, in font sizes, and the share of its width its notes take.
const GUTTER: f32 = 2.5;
const NOTE_SHARE: f32 = 0.34;

/// `text` with every `\r\n` and lone `\r` turned into `\n`, the one line break the area keeps.
pub fn unix_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn clamp(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn previous_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp(text, offset);
    text[..offset]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp(text, offset);
    text[offset..]
        .graphemes(true)
        .next()
        .map_or(offset, |g| offset + g.len())
}

fn previous_word(text: &str, offset: usize) -> usize {
    let head = text[..clamp(text, offset)].trim_end();
    head.rfind(char::is_whitespace).map_or(0, |index| {
        index + head[index..].chars().next().map_or(1, char::len_utf8)
    })
}

fn next_word(text: &str, offset: usize) -> usize {
    let offset = clamp(text, offset);
    let tail = &text[offset..];
    let word = tail.trim_start();
    let skipped = tail.len() - word.len();
    offset + skipped + word.find(char::is_whitespace).unwrap_or(word.len())
}

/// The start of the line `offset` is on.
fn line_start(text: &str, offset: usize) -> usize {
    text[..clamp(text, offset)].rfind('\n').map_or(0, |i| i + 1)
}

/// The end of the line `offset` is on, before its line break.
fn line_end(text: &str, offset: usize) -> usize {
    let offset = clamp(text, offset);
    text[offset..].find('\n').map_or(text.len(), |i| offset + i)
}

fn to_utf16(text: &str, offset: usize) -> usize {
    text[..clamp(text, offset)].encode_utf16().count()
}

fn from_utf16(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (index, character) in text.char_indices() {
        if utf16 >= offset {
            return index;
        }
        utf16 += character.len_utf16();
    }
    text.len()
}

/// The paragraphs as last painted: one wrapped line per `\n`-separated line of the text, with the
/// byte offset each starts at and its top, both from `origin`.
#[derive(Default)]
struct Shaped {
    lines: Vec<WrappedLine>,
    starts: Vec<usize>,
    tops: Vec<Pixels>,
    origin: Point<Pixels>,
    line_height: Pixels,
}

impl Shaped {
    fn shape(
        text: &str,
        width: Pixels,
        runs: &[TextRun],
        font_size: Pixels,
        window: &Window,
    ) -> Self {
        let line_height = window.line_height();
        let lines = window
            .text_system()
            .shape_text(
                SharedString::from(text.to_owned()),
                font_size,
                runs,
                Some(width),
                None,
            )
            .map(|lines| lines.into_vec())
            .unwrap_or_default();
        let mut starts = Vec::with_capacity(lines.len());
        let mut tops = Vec::with_capacity(lines.len());
        let (mut start, mut top) = (0, Pixels::ZERO);
        for line in &lines {
            starts.push(start);
            tops.push(top);
            start += line.len() + 1;
            top += line.size(line_height).height;
        }
        Self {
            lines,
            starts,
            tops,
            origin: Point::default(),
            line_height,
        }
    }

    fn height(&self) -> Pixels {
        match (self.tops.last(), self.lines.last()) {
            (Some(top), Some(line)) => *top + line.size(self.line_height).height,
            _ => self.line_height,
        }
    }

    /// The window position of the caret at `offset`, at the top of its row.
    fn position(&self, offset: usize) -> Point<Pixels> {
        let at = self.starts.partition_point(|start| *start <= offset).max(1) - 1;
        let Some(line) = self.lines.get(at) else {
            return self.origin;
        };
        let local = (offset - self.starts[at]).min(line.len());
        let p = line
            .position_for_index(local, self.line_height)
            .unwrap_or_default();
        self.origin + point(p.x, self.tops[at] + p.y)
    }

    /// The offset closest to the window position `position`.
    fn offset(&self, position: Point<Pixels>) -> usize {
        let y = position.y - self.origin.y;
        let at = self.tops.partition_point(|top| *top <= y).max(1) - 1;
        let Some(line) = self.lines.get(at) else {
            return 0;
        };
        let height = line.size(self.line_height).height;
        let local = point(
            (position.x - self.origin.x).max(Pixels::ZERO),
            (y - self.tops[at]).clamp(Pixels::ZERO, height - px(1.)),
        );
        let index = line
            .closest_index_for_position(local, self.line_height)
            .unwrap_or_else(|index| index);
        self.starts[at] + index.min(line.len())
    }
}

/// Emitted after a paste into the area, so its owner can tidy what arrived.
pub struct Pasted;

/// What a sheet says at the end of one of its lines (`TextArea::set_notes`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineNote {
    pub text: SharedString,
    /// The line is wrong: drawn in the danger colour with a wavy underline.
    pub bad: bool,
}

/// A field of several lines. See the module docs.
pub struct TextArea {
    focus_handle: FocusHandle,
    /// The Fluent key of the placeholder.
    hint: SharedString,
    /// The Fluent key of the name a screen reader reads.
    name: Option<SharedString>,
    icon: Option<SharedString>,
    clearable: bool,
    /// How many lines the area grows to before it scrolls.
    max_lines: f32,
    /// Set while the area is a sheet: one note per line of the text.
    notes: Option<Vec<LineNote>>,
    /// A small count at the end of the field, before its clear button ("+4").
    badge: Option<SharedString>,
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    shaped: Shaped,
    scroll: ScrollHandle,
    /// Set by a caret move, so the next paint scrolls it into sight.
    reveal: bool,
    selecting: bool,
}

impl TextArea {
    /// An empty area whose placeholder is the Fluent message `hint`.
    pub fn new(hint: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            hint: hint.into(),
            name: None,
            icon: None,
            clearable: false,
            max_lines: MAX_LINES,
            notes: None,
            badge: None,
            content: String::new(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            shaped: Shaped::default(),
            scroll: ScrollHandle::new(),
            reveal: false,
            selecting: false,
        }
    }

    /// Names the area for screen readers with the Fluent message `key`.
    pub fn name(mut self, key: impl Into<SharedString>) -> Self {
        self.name = Some(key.into());
        self
    }

    /// The icon before the first line.
    pub fn icon(mut self, path: impl Into<SharedString>) -> Self {
        self.icon = Some(path.into());
        self
    }

    /// A clear button at the end while there is text.
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// Grows to `lines` lines at most (`MAX_LINES` unless set), then scrolls; 1 keeps the area
    /// one line tall in its layout while it still holds and edits several.
    pub fn max_lines(mut self, lines: f32) -> Self {
        self.max_lines = lines.max(1.);
        self
    }

    /// Changes how many lines the area grows to, as `max_lines` does.
    pub fn set_max_lines(&mut self, lines: f32, cx: &mut Context<Self>) {
        self.max_lines = lines.max(1.);
        self.reveal = true;
        cx.notify();
    }

    /// `Some` draws the area as a bare sheet for a surface around it: no field chrome, icon or
    /// clear button, numbered lines, and `notes[i]` at the end of line `i`. `None` is the field.
    pub fn set_notes(&mut self, notes: Option<Vec<LineNote>>, cx: &mut Context<Self>) {
        if self.notes != notes {
            self.notes = notes;
            cx.notify();
        }
    }

    /// Shows `badge` as a small chip at the end of the field, or none.
    pub fn set_badge(&mut self, badge: Option<SharedString>, cx: &mut Context<Self>) {
        if self.badge != badge {
            self.badge = badge;
            cx.notify();
        }
    }

    /// Puts the caret at the very start, so a one-line field shows its first line.
    pub fn move_to_start(&mut self, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    /// The field as it looks focused, without editing: its icon, the first line and the badge.
    /// For the field's place while the area itself is drawn elsewhere (a sheet over the page).
    pub fn face(&self, window: &Window, cx: &App) -> gpui::Div {
        let theme = *cx.theme();
        let first = self.content.lines().next().unwrap_or_default().to_owned();
        let line = window.line_height();
        // the same box as the field's one line, so swapping them moves nothing
        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .py(((theme.metrics.field - line) / 2.).max(Pixels::ZERO))
            .px_3()
            .rounded(theme.radius)
            .bg(theme.foreground.opacity(FILL))
            .border_1()
            .border_color(theme.table_active_border)
            .shadow(vec![ring(&theme)])
            .cursor(CursorStyle::IBeam)
            .when_some(self.icon.clone(), |this, path| {
                this.child(
                    svg()
                        .path(icons::path(path))
                        .size_4()
                        .flex_none()
                        .text_color(theme.muted_foreground),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(line)
                    .truncate()
                    .font_family(MONO)
                    .child(first),
            )
            .children(self.badge.clone().map(|text| {
                div()
                    .flex_none()
                    .h(line)
                    .flex()
                    .items_center()
                    .child(badge(text, &theme))
            }))
    }

    /// The text, lines joined by `\n`.
    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replaces the text (line breaks made `\n`) and puts the caret at its end.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = unix_lines(&text.into());
        self.marked_range = None;
        self.move_to(self.content.len(), cx);
    }

    /// Moves keyboard focus into the area.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus_handle, cx);
    }

    fn cursor(&self) -> usize {
        match self.selection_reversed {
            true => self.selected_range.start,
            false => self.selected_range.end,
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = clamp(&self.content, offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.reveal = true;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = clamp(&self.content, offset);
        match self.selection_reversed {
            true => self.selected_range.start = offset,
            false => self.selected_range.end = offset,
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.reveal = true;
        cx.notify();
    }

    fn step(&mut self, motion: fn(&str, usize) -> usize, cx: &mut Context<Self>) {
        let offset = motion(&self.content, self.cursor());
        self.move_to(offset, cx);
    }

    fn extend(&mut self, motion: fn(&str, usize) -> usize, cx: &mut Context<Self>) {
        let offset = motion(&self.content, self.cursor());
        self.select_to(offset, cx);
    }

    fn erase(
        &mut self,
        motion: fn(&str, usize) -> usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.extend(motion, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    /// The offset one row above (`-1.`) or below (`1.`) the caret, kept at its x; past the first
    /// or last row it is the start or end of the text.
    fn vertical(&self, rows: f32) -> usize {
        let shaped = &self.shaped;
        let caret = shaped.position(self.cursor());
        let y = caret.y + shaped.line_height * (rows + 0.5);
        if y < shaped.origin.y {
            0
        } else if y >= shaped.origin.y + shaped.height() {
            self.content.len()
        } else {
            shaped.offset(point(caret.x, y))
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.erase(previous_boundary, window, cx);
    }

    fn backspace_word(&mut self, _: &BackspaceWord, window: &mut Window, cx: &mut Context<Self>) {
        self.erase(previous_word, window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.erase(next_boundary, window, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        match self.selected_range.is_empty() {
            true => self.step(previous_boundary, cx),
            false => self.move_to(self.selected_range.start, cx),
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        match self.selected_range.is_empty() {
            true => self.step(next_boundary, cx),
            false => self.move_to(self.selected_range.end, cx),
        }
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.step(previous_word, cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.step(next_word, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.extend(previous_boundary, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.extend(next_boundary, cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.step(line_start, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.step(line_end, cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.extend(line_start, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.extend(line_end, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn line_up(&mut self, _: &LineUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.vertical(-1.), cx);
    }

    fn line_down(&mut self, _: &LineDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.vertical(1.), cx);
    }

    fn select_line_up(&mut self, _: &SelectLineUp, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.vertical(-1.), cx);
    }

    fn select_line_down(&mut self, _: &SelectLineDown, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.vertical(1.), cx);
    }

    fn space(&mut self, _: &Space, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, " ", window, cx);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn write_selection(&self, cx: &mut Context<Self>) -> bool {
        let range = clamp(&self.content, self.selected_range.start)
            ..clamp(&self.content, self.selected_range.end);
        if range.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.content[range].to_owned()));
        true
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.write_selection(cx);
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.write_selection(cx) {
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &unix_lines(&text), window, cx);
            cx.emit(Pasted);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus(window, cx);
        let offset = self.shaped.offset(event.position);
        match (event.click_count, event.modifiers.shift) {
            (0 | 1, true) => self.select_to(offset, cx),
            (0 | 1, false) => self.move_to(offset, cx),
            // a double click takes the whole line, which is one link
            (2, _) => {
                self.move_to(line_start(&self.content, offset), cx);
                self.select_to(line_end(&self.content, offset), cx);
            }
            _ => self.select_all(&SelectAll, window, cx),
        }
        self.selecting = true;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting && event.dragging() {
            let offset = self.shaped.offset(event.position);
            self.select_to(offset, cx);
        }
    }

    fn edited_range(&self, range_utf16: Option<&Range<usize>>) -> Range<usize> {
        let range = range_utf16
            .map(|r| from_utf16(&self.content, r.start)..from_utf16(&self.content, r.end))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let start = clamp(&self.content, range.start);
        start..clamp(&self.content, range.end).max(start)
    }
}

impl gpui::EventEmitter<Pasted> for TextArea {}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.edited_range(Some(&range_utf16));
        actual.replace(to_utf16(&self.content, range.start)..to_utf16(&self.content, range.end));
        Some(self.content[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: to_utf16(&self.content, self.selected_range.start)
                ..to_utf16(&self.content, self.selected_range.end),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|r| to_utf16(&self.content, r.start)..to_utf16(&self.content, r.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.edited_range(range_utf16.as_ref());
        self.content.replace_range(range.clone(), text);
        self.marked_range = None;
        self.move_to(range.start + text.len(), cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.edited_range(range_utf16.as_ref());
        self.content.replace_range(range.clone(), text);
        let caret = range.start + text.len();
        self.marked_range = (!text.is_empty()).then_some(range.start..caret);
        let selected = selected.map_or(caret..caret, |s| {
            range.start + from_utf16(text, s.start)..range.start + from_utf16(text, s.end)
        });
        self.selected_range =
            clamp(&self.content, selected.start)..clamp(&self.content, selected.end);
        self.selection_reversed = false;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let start = from_utf16(&self.content, range_utf16.start);
        let p = self.shaped.position(start);
        Some(Bounds::new(p, size(CARET, self.shaped.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(to_utf16(&self.content, self.shaped.offset(position)))
    }
}

/// The lines themselves: measured to the width they get, so the area grows with them.
struct Lines {
    area: Entity<TextArea>,
}

impl IntoElement for Lines {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct Prepared {
    shaped: Shaped,
    caret: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    /// A sheet's line numbers and notes, each with where it is drawn.
    marks: Vec<(Point<Pixels>, ShapedLine)>,
    notes: Vec<(Point<Pixels>, ShapedLine)>,
    /// The column the notes are clipped to.
    note_column: Bounds<Pixels>,
}

/// Where a sheet's text stands inside `width`: its left inset (the gutter) and its own width.
/// A field's text takes the whole width.
fn columns(sheet: bool, width: Pixels, font_size: Pixels) -> (Pixels, Pixels) {
    match sheet {
        false => (Pixels::ZERO, width),
        true => {
            let gutter = font_size * GUTTER;
            (gutter, (width - gutter - width * NOTE_SHARE).max(px(1.)))
        }
    }
}

impl Lines {
    /// The text drawn (the placeholder while empty), its runs (a sheet's wrong lines marked),
    /// its font size and whether the area is a sheet.
    fn text(&self, window: &Window, cx: &App) -> (String, Vec<TextRun>, Pixels, bool) {
        let area = self.area.read(cx);
        let theme = cx.theme();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let sheet = area.notes.is_some();
        let base = TextRun {
            len: 0,
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        if area.content.is_empty() {
            let text = i18n::lookup(&area.hint, None).to_string();
            let run = TextRun {
                len: text.len(),
                color: theme.muted_foreground,
                ..base
            };
            return (text, vec![run], font_size, sheet);
        }
        // the links are mono, the placeholder and the notes keep the surrounding face
        let mono = TextRun {
            font: Font {
                family: MONO.into(),
                ..style.font()
            },
            ..base
        };
        let notes = area.notes.as_deref().unwrap_or_default();
        let mut runs: Vec<TextRun> = area
            .content
            .split('\n')
            .enumerate()
            .map(|(at, line)| match notes.get(at).is_some_and(|n| n.bad) {
                true => TextRun {
                    len: line.len() + 1,
                    color: theme.danger,
                    underline: Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(theme.danger),
                        wavy: true,
                    }),
                    ..mono.clone()
                },
                false => TextRun {
                    len: line.len() + 1,
                    ..mono.clone()
                },
            })
            .collect();
        // the last line has no break after it
        if let Some(last) = runs.last_mut() {
            last.len -= 1;
        }
        runs.retain(|run| run.len > 0);
        (area.content.clone(), runs, font_size, sheet)
    }
}

impl Element for Lines {
    type RequestLayoutState = ();
    type PrepaintState = Prepared;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (text, runs, font_size, sheet) = self.text(window, cx);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(width) => width,
                _ => px(10_000.),
            });
            let (_, text_width) = columns(sheet, width, font_size);
            let shaped = Shaped::shape(&text, text_width, &runs, font_size, window);
            size(width, shaped.height())
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = *cx.theme();
        let (text, runs, font_size, sheet) = self.text(window, cx);
        let (gutter, text_width) = columns(sheet, bounds.size.width, font_size);
        let mut shaped = Shaped::shape(&text, text_width, &runs, font_size, window);
        shaped.origin = bounds.origin + point(gutter, Pixels::ZERO);
        let (left, right) = (shaped.origin.x, shaped.origin.x + text_width);
        let area = self.area.read(cx);
        let lh = shaped.line_height;
        let (selection, caret) = match area.content.is_empty() || area.selected_range.is_empty() {
            true => {
                let at = shaped.position(if area.content.is_empty() {
                    0
                } else {
                    area.cursor()
                });
                (
                    vec![],
                    Some(fill(Bounds::new(at, size(CARET, lh)), theme.foreground)),
                )
            }
            false => {
                let (a, b) = (
                    shaped.position(area.selected_range.start),
                    shaped.position(area.selected_range.end),
                );
                let color = theme.selection.opacity(0.4);
                let rect = |x0: Pixels, x1: Pixels, y: Pixels| {
                    fill(
                        Bounds::from_corners(point(x0, y), point(x1.max(x0 + CARET), y + lh)),
                        color,
                    )
                };
                let mut quads = vec![];
                if a.y == b.y {
                    quads.push(rect(a.x, b.x, a.y));
                } else {
                    quads.push(rect(a.x, right, a.y));
                    if b.y > a.y + lh {
                        quads.push(fill(
                            Bounds::from_corners(point(left, a.y + lh), point(right, b.y)),
                            color,
                        ));
                    }
                    quads.push(rect(left, b.x, b.y));
                }
                (quads, None)
            }
        };
        // a sheet numbers its lines in the gutter and writes each note at the end of its line
        let note_column = Bounds::from_corners(
            point(right + font_size, bounds.top()),
            point(bounds.right(), bounds.bottom()),
        );
        let (mut marks, mut notes) = (vec![], vec![]);
        if let Some(line_notes) = area.notes.as_ref().filter(|_| !area.content.is_empty()) {
            let small = theme.text(crate::Text::Small);
            let style = window.text_style();
            let shape = |text: String, font: Font, color| {
                let run = TextRun {
                    len: text.len(),
                    font,
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window
                    .text_system()
                    .shape_line(text.into(), small, &[run], None)
            };
            let mono = Font {
                family: MONO.into(),
                ..style.font()
            };
            for (at, top) in shaped.tops.iter().enumerate() {
                let y = bounds.top() + *top;
                let number = shape((at + 1).to_string(), mono.clone(), theme.muted_foreground);
                let x = left - font_size * 0.75 - number.width();
                marks.push((point(x, y), number));
                if let Some(note) = line_notes.get(at).filter(|n| !n.text.is_empty()) {
                    let color = match note.bad {
                        true => theme.danger,
                        false => theme.muted_foreground,
                    };
                    let line = shape(note.text.to_string(), style.font(), color);
                    // right-aligned; a note wider than its column starts at the column and is cut
                    let x = (bounds.right() - line.width()).max(note_column.left());
                    notes.push((point(x, y), line));
                }
            }
        }
        // keep the caret in the scrolled view after a move or an edit
        if area.reveal {
            let caret = shaped.position(area.cursor());
            let view = area.scroll.bounds();
            let mut offset = area.scroll.offset();
            if caret.y < view.top() {
                offset.y += view.top() - caret.y;
            } else if caret.y + lh > view.bottom() {
                offset.y -= caret.y + lh - view.bottom();
            }
            if offset != area.scroll.offset() {
                area.scroll.set_offset(offset);
                window.refresh();
            }
            self.area.update(cx, |area, _| area.reveal = false);
        }
        Prepared {
            shaped,
            caret,
            selection,
            marks,
            notes,
            note_column,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepared: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.area.read(cx).focus_handle.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.area.clone()),
            cx,
        );
        let focused = focus.is_focused(window);
        if focused {
            for quad in prepared.selection.drain(..) {
                window.paint_quad(quad);
            }
        }
        let shaped = std::mem::take(&mut prepared.shaped);
        let lh = shaped.line_height;
        for (line, top) in shaped.lines.iter().zip(&shaped.tops) {
            line.paint(
                shaped.origin + point(Pixels::ZERO, *top),
                lh,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
        }
        for (at, mark) in &prepared.marks {
            mark.paint(*at, lh, gpui::TextAlign::Left, None, window, cx)
                .ok();
        }
        let column = prepared.note_column;
        window.with_content_mask(Some(gpui::ContentMask { bounds: column }), |window| {
            for (at, note) in &prepared.notes {
                note.paint(*at, lh, gpui::TextAlign::Left, None, window, cx)
                    .ok();
            }
        });
        if focused && let Some(caret) = prepared.caret.take() {
            window.paint_quad(caret);
        }
        self.area.update(cx, |area, _| area.shaped = shaped);
    }
}

impl Render for TextArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let focused = self.focus_handle.is_focused(window);
        let line = window.line_height();
        // one line stands as tall as the large field; more lines grow it, up to MAX_LINES
        let pad = ((theme.metrics.field - line) / 2.).max(Pixels::ZERO);
        let edge = match focused {
            true => theme.table_active_border,
            false => theme.foreground.opacity(EDGE),
        };
        let label = self.name.as_ref().map_or_else(
            || i18n::lookup(&self.hint, None),
            |key| i18n::lookup(key, None),
        );
        // a sheet is bare: the surface around it draws the edge, the padding and the actions
        let field = self.notes.is_none();
        let line_box = || div().flex_none().h(line).flex().items_center();
        div()
            .id(&self.focus_handle)
            .role(gpui::accesskit::Role::MultilineTextInput)
            .aria_label(label)
            .when(!self.content.is_empty(), |this| {
                this.aria_value(SharedString::from(self.content.clone()))
            })
            .flex()
            .items_start()
            .gap_2()
            .w_full()
            .min_w_0()
            .when(field, |this| {
                this.px_3()
                    // the padding stays outside the scrolled lines, so a scrolled view shows
                    // whole lines
                    .py(pad)
                    .rounded(theme.radius)
                    .bg(theme.foreground.opacity(FILL))
                    .border_1()
                    .border_color(edge)
            })
            .when(field && focused, |this| this.shadow(vec![ring(&theme)]))
            .key_context(format!("{INPUT_CONTEXT} {TEXT_AREA_CONTEXT}").as_str())
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::backspace_word))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::line_up))
            .on_action(cx.listener(Self::line_down))
            .on_action(cx.listener(Self::select_line_up))
            .on_action(cx.listener(Self::select_line_down))
            .on_action(cx.listener(Self::space))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.selecting = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.selecting = false),
            )
            .when_some(self.icon.clone().filter(|_| field), |this, path| {
                this.child(
                    line_box().child(
                        svg()
                            .path(icons::path(path))
                            .size_4()
                            .text_color(theme.muted_foreground),
                    ),
                )
            })
            .child(
                div()
                    .id("text-area-lines")
                    .flex_1()
                    .min_w_0()
                    .max_h(line * self.max_lines)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(Lines { area: cx.entity() }),
            )
            .when_some(self.badge.clone().filter(|_| field), |this, text| {
                this.child(line_box().child(badge(text, &theme)))
            })
            .when(
                field && self.clearable && !self.content.is_empty(),
                |this| {
                    this.child(
                        line_box().child(
                            Button::new("text-area-clear")
                                .icon("icons/x.svg")
                                .tooltip("common-clear")
                                .aria_label(t!("common-clear"))
                                .small()
                                .ghost()
                                .px_1()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_text("", cx);
                                    this.focus(window, cx);
                                })),
                        ),
                    )
                },
            )
    }
}

/// The focus ring a focused field wears.
fn ring(theme: &crate::theme::Theme) -> BoxShadow {
    BoxShadow {
        color: theme.table_active_border.opacity(RING),
        offset: point(Pixels::ZERO, Pixels::ZERO),
        blur_radius: Pixels::ZERO,
        spread_radius: RING_WIDTH,
        inset: false,
    }
}

/// The field's count chip: ink on the selection tint, small.
fn badge(text: SharedString, theme: &crate::theme::Theme) -> gpui::Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.table_active_border.opacity(0.5))
        .bg(theme.table_active)
        .text_color(theme.table_active_border)
        .text_size(theme.text(crate::Text::Small))
        .font_features(crate::tabular())
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_keeps_a_gutter_and_a_note_column() {
        assert_eq!(columns(false, px(500.), px(12.)), (px(0.), px(500.)));
        let (gutter, text) = columns(true, px(500.), px(12.));
        assert_eq!(gutter, px(30.));
        assert_eq!(text, px(500.) - px(30.) - px(500.) * NOTE_SHARE);
        // never a negative width, however narrow
        assert_eq!(columns(true, px(10.), px(12.)).1, px(1.));
    }

    #[test]
    fn line_breaks_become_newlines() {
        assert_eq!(unix_lines("a\r\nb\rc\nd"), "a\nb\nc\nd");
    }

    #[test]
    fn home_and_end_stay_on_their_line() {
        let text = "one\ntwo\nthree";
        assert_eq!((line_start(text, 5), line_end(text, 5)), (4, 7));
        assert_eq!((line_start(text, 0), line_end(text, 0)), (0, 3));
        assert_eq!((line_start(text, 13), line_end(text, 13)), (8, 13));
        assert_eq!(line_start(text, 4), 4);
    }

    #[test]
    fn walks_characters_and_words() {
        assert_eq!(previous_boundary("hé", 3), 1);
        assert_eq!(next_boundary("hé", 1), 3);
        assert_eq!(next_boundary("a", 1), 1);
        assert_eq!(previous_word("ab cd", 5), 3);
        assert_eq!(next_word("ab cd", 0), 2);
        assert_eq!(next_word("ab\ncd", 2), 5);
    }

    #[test]
    fn converts_utf16_offsets() {
        assert_eq!(to_utf16("😀a", 4), 2);
        assert_eq!(from_utf16("😀a", 2), 4);
        assert_eq!(from_utf16("😀a", 99), 5);
    }
}
