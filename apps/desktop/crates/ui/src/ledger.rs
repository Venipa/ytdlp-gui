//! The Ledger parts: call numbers, the ruled progress line, thumbnails, filter chips, the side
//! nav and the search input with its parsed tokens.

use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, ElementId, Entity, Pixels, SharedString, Window, canvas, div, fill,
    point, px, size,
};

use crate::artwork::Artwork;
use crate::input::Input;
use crate::metrics::Text;
use crate::theme::ActiveTheme as _;
use crate::time::tabular;

/// The family of call numbers, ids, format codes, logs and the intake text.
pub const MONO: &str = "IBM Plex Mono";

/// Width of the call-number column.
const CALL_WIDTH: f32 = 56.;
/// Distance between the dots of a dotted track, and the dot size.
const DOT_PITCH: f32 = 4.;
const DOT: f32 = 1.;
/// Thickness of the amber fill over the track.
const FILL: f32 = 2.;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type SelectHandler = std::rc::Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// The four-digit job id that starts every ledger line.
#[derive(IntoElement)]
pub struct CallNumber {
    number: i64,
    selected: bool,
}

impl CallNumber {
    /// A call number for job `number`.
    pub fn new(number: i64) -> Self {
        Self {
            number,
            selected: false,
        }
    }

    /// Lights the number ink-blue while its row is selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl RenderOnce for CallNumber {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        div()
            .w(px(CALL_WIDTH))
            .flex_none()
            .font_family(MONO)
            .text_size(theme.text(Text::Small))
            .text_color(match self.selected {
                true => theme.table_active_border,
                false => theme.muted_foreground,
            })
            .child(format!("{:04}", self.number))
    }
}

/// What a ledger rule shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    /// Waiting, paused, cancelled: a dotted track.
    Dotted,
    /// Active: the dotted track with an amber fill up to the fraction, 0 to 1.
    Fill(f32),
    /// Done: a quiet solid rule.
    Solid,
    /// Failed: a solid red rule.
    Failed,
}

/// The ruled line under a row, which is also its progress.
// shortcut: the probing pulse is static, animate the tail dots when motion presets are wired.
#[derive(IntoElement)]
pub struct ProgressRule {
    rule: Rule,
}

impl ProgressRule {
    /// A rule in state `rule`.
    pub fn new(rule: Rule) -> Self {
        Self { rule }
    }
}

impl RenderOnce for ProgressRule {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let rule = self.rule;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let base = bounds.origin.y + bounds.size.height;
                let at = |x: Pixels, w: Pixels, h: Pixels| gpui::Bounds {
                    origin: point(x, base - h),
                    size: size(w, h),
                };
                match rule {
                    Rule::Solid | Rule::Failed => {
                        let color = match rule {
                            Rule::Failed => theme.danger,
                            _ => theme.border,
                        };
                        window.paint_quad(fill(
                            at(bounds.origin.x, bounds.size.width, px(1.)),
                            color,
                        ));
                    }
                    Rule::Dotted | Rule::Fill(_) => {
                        let mut x = bounds.origin.x;
                        let end = bounds.origin.x + bounds.size.width;
                        while x < end {
                            window.paint_quad(fill(
                                at(x, px(DOT), px(DOT)),
                                theme.muted_foreground.opacity(0.5),
                            ));
                            x += px(DOT_PITCH);
                        }
                        if let Rule::Fill(done) = rule {
                            let width = bounds.size.width * done.clamp(0., 1.);
                            window.paint_quad(fill(
                                at(bounds.origin.x, width, px(FILL)),
                                theme.progress_bar,
                            ));
                        }
                    }
                }
            },
        )
        .w_full()
        .h(px(FILL))
        .flex_none()
    }
}

/// A 16:9 cover: the image at `path` or `url` when there is one, else an icon on a drawer.
/// Decoding, the skeleton while it loads and the cache are the artwork loader's.
#[derive(IntoElement)]
pub struct Thumbnail {
    source: Option<SharedString>,
    width: Pixels,
    audio: bool,
    pill: Option<SharedString>,
}

impl Thumbnail {
    /// A thumbnail `width` wide, 16:9. A source that is not a link is a file path.
    pub fn new(source: Option<SharedString>, width: Pixels) -> Self {
        let source = source.map(|s| match s.starts_with("http") || s.starts_with("file://") {
            true => s,
            false => format!("file://{s}").into(),
        });
        Self {
            source,
            width,
            audio: false,
            pill: None,
        }
    }

    /// Shows a note icon instead of a film strip when there is no image.
    pub fn audio(mut self, audio: bool) -> Self {
        self.audio = audio;
        self
    }

    /// A small label over the bottom right corner, as the duration.
    pub fn pill(mut self, pill: impl Into<SharedString>) -> Self {
        self.pill = Some(pill.into());
        self
    }
}

impl RenderOnce for Thumbnail {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let height = self.width * 9. / 16.;
        let art = Artwork::new(self.source)
            .size(self.width)
            .wide(height)
            .corner_radius(px(2.))
            .fallback(match self.audio {
                true => "icons/music.svg",
                false => "icons/film.svg",
            });
        div()
            .relative()
            .flex_none()
            .w(self.width)
            .h(height)
            .rounded(px(2.))
            .overflow_hidden()
            .bg(theme.secondary)
            .child(art)
            .when_some(self.pill, |this, pill| {
                this.child(
                    div()
                        .absolute()
                        .right_1()
                        .bottom_1()
                        .px_1()
                        .rounded(px(2.))
                        .bg(gpui::black().opacity(0.7))
                        .text_color(gpui::white())
                        .text_size(theme.text(Text::Tiny))
                        .font_features(tabular())
                        .child(pill),
                )
            })
    }
}

/// A filter chip: a label with a count, selectable.
#[derive(IntoElement)]
pub struct Chip {
    id: ElementId,
    label: SharedString,
    count: Option<usize>,
    selected: bool,
    on_click: Option<ClickHandler>,
}

impl Chip {
    /// A chip named `label`.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            count: None,
            selected: false,
            on_click: None,
        }
    }

    /// Shows a count after the label.
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// Marks the chip as the chosen one.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Runs `handler` on click.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let selected = self.selected;
        let name = match self.count {
            Some(count) => format!("{} {count}", self.label),
            None => self.label.to_string(),
        };
        crate::focus::focusable(div().id(self.id.clone()), &self.id, window, cx)
            .role(Role::Button)
            .aria_label(name)
            .flex()
            .items_center()
            .gap_1()
            .h(theme.metrics.control_small)
            .px_2()
            .rounded(theme.radius)
            .border_1()
            .cursor_pointer()
            .text_size(theme.text(Text::Label))
            .border_color(match selected {
                true => theme.table_active_border,
                false => theme.border,
            })
            .bg(match selected {
                true => theme.table_active,
                false => gpui::transparent_black(),
            })
            .hover(|style| style.bg(theme.table_hover))
            .child(self.label)
            .when_some(self.count, |this, count| {
                this.child(
                    div()
                        .text_color(theme.muted_foreground)
                        .font_features(tabular())
                        .child(count.to_string()),
                )
            })
            .when_some(self.on_click, |this, handler| this.on_click(handler))
    }
}

/// One entry of a [`SideNav`].
pub struct SideNavItem {
    /// Already translated label.
    pub label: SharedString,
}

/// A vertical list of pages with the open one lit.
#[derive(IntoElement)]
pub struct SideNav {
    items: Vec<SideNavItem>,
    icons: Vec<&'static str>,
    selected: usize,
    on_select: SelectHandler,
}

impl SideNav {
    /// A nav over `items` with `selected` lit; `on_select` receives the clicked index.
    pub fn new(
        items: Vec<SideNavItem>,
        selected: usize,
        on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            items,
            icons: Vec::new(),
            selected,
            on_select: std::rc::Rc::new(on_select),
        }
    }

    /// Icon paths for the items, in order; items past the list have none.
    pub fn icons(mut self, icons: impl IntoIterator<Item = &'static str>) -> Self {
        self.icons = icons.into_iter().collect();
        self
    }
}

impl RenderOnce for SideNav {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(168.))
            .gap_px()
            .p_2()
            .border_r_1()
            .border_color(theme.sidebar_border)
            .children(self.items.into_iter().enumerate().map(|(index, item)| {
                let on = index == self.selected;
                let select = self.on_select.clone();
                let icon = self.icons.get(index).copied();
                let id: ElementId = ("side-nav", index).into();
                crate::focus::focusable(div().id(id.clone()), &id, window, cx)
                    .role(Role::Tab)
                    .aria_label(item.label.clone())
                    .aria_selected(on)
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .text_size(theme.text(Text::Body))
                    .when(on, |this| {
                        this.bg(theme.sidebar_accent)
                            .text_color(theme.table_active_border)
                    })
                    .when(!on, |this| this.hover(|style| style.bg(theme.table_hover)))
                    .on_click(move |_, window, cx| select(index, window, cx))
                    .when_some(icon, |this, path| {
                        this.child(
                            gpui::svg()
                                .path(icons::path(path))
                                .size(px(16.))
                                .flex_none()
                                .text_color(match on {
                                    true => theme.table_active_border,
                                    false => theme.muted_foreground,
                                }),
                        )
                    })
                    .child(item.label)
            }))
    }
}

/// A search field plus the tokens it parsed, valid ones ink-blue and unparsed ones red.
// shortcut: tokens are listed beside the field, not drawn inside its text, move them in when
// the input element supports styled runs.
#[derive(IntoElement)]
pub struct TokenInput {
    input: Entity<Input>,
    tokens: Vec<(SharedString, bool)>,
}

impl TokenInput {
    /// Wraps `input`; `tokens` are the `key:value` terms as typed and whether each parsed.
    pub fn new(input: &Entity<Input>, tokens: Vec<(SharedString, bool)>) -> Self {
        Self {
            input: input.clone(),
            tokens,
        }
    }
}

impl RenderOnce for TokenInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        div()
            .flex()
            .items_center()
            .gap_2()
            .children(self.tokens.into_iter().map(|(text, valid)| {
                div()
                    .px_1()
                    .rounded(theme.radius)
                    .font_family(MONO)
                    .text_size(theme.text(Text::Tiny))
                    .text_color(match valid {
                        true => theme.table_active_border,
                        false => theme.danger,
                    })
                    .bg(match valid {
                        true => theme.table_active,
                        false => gpui::transparent_black(),
                    })
                    .child(text)
            }))
            .child(div().w(px(240.)).child(self.input))
    }
}
