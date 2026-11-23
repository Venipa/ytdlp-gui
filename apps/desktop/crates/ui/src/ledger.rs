//! The Ledger parts: call numbers, the ruled progress line, thumbnails, filter chips, the side
//! nav and the search input with its parsed tokens.

use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, ElementId, Entity, Pixels, SharedString, Window, canvas, div, fill, point, px,
    size,
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
/// Where the probing pulse's clock starts.
static PULSE_EPOCH: std::sync::LazyLock<std::time::Instant> =
    std::sync::LazyLock::new(std::time::Instant::now);
/// Thickness of the amber fill and of the faint track it runs along.
const FILL: f32 = 3.;
/// How strong an active rule's track is, as an alpha of amber: the whole row width reads as
/// the distance to go.
const TRACK: f32 = 0.24;
/// A chip's side padding and the gap between its label and count.
const CHIP_INSET: Pixels = px(10.);
const CHIP_GAP: Pixels = px(6.);
/// The search field's width range: it gives way to the chips beside it in a narrow window.
const SEARCH_MIN: f32 = 140.;
const SEARCH_MAX: f32 = 280.;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

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

/// What a ledger rule shows. Only work in motion carries a rule; every other row sits on the
/// list's plain hairline divider, so a still row never reads as a broken line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    /// Active: a faint amber track with an amber fill up to the fraction, 0 to 1.
    Fill(f32),
    /// Reading the page: a short amber stretch travelling along the faint track.
    Pulse,
    /// Still: the plain 1px divider in the `border` colour, for a list whose lines speak (the
    /// welcome tools) and keep a rule node whatever their state.
    Hairline,
}

/// The ruled line under a working row, which is also its progress.
#[derive(IntoElement)]
pub struct ProgressRule {
    rule: Rule,
    spoken: Option<(ElementId, SharedString)>,
}

impl ProgressRule {
    /// A rule in state `rule`.
    pub fn new(rule: Rule) -> Self {
        Self { rule, spoken: None }
    }

    /// Puts the rule in the accessibility tree as a progress indicator named `name`; a rule
    /// that only decorates a row that already speaks stays out of it.
    pub fn spoken(mut self, id: impl Into<ElementId>, name: impl Into<SharedString>) -> Self {
        self.spoken = Some((id.into(), name.into()));
        self
    }
}

impl RenderOnce for ProgressRule {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let rule = self.rule;
        let spoken = self.spoken;
        let fraction = match rule {
            Rule::Fill(done) => Some(f64::from(done.clamp(0., 1.))),
            Rule::Pulse | Rule::Hairline => None,
        };
        let line = canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                let base = bounds.origin.y + bounds.size.height;
                let at = |x: Pixels, w: Pixels| gpui::Bounds {
                    origin: point(x, base - px(FILL)),
                    size: size(w, px(FILL)),
                };
                if rule == Rule::Hairline {
                    let line = gpui::Bounds {
                        origin: point(bounds.origin.x, base - px(1.)),
                        size: size(bounds.size.width, px(1.)),
                    };
                    window.paint_quad(fill(line, theme.border));
                    return;
                }
                // one line: a faint amber track the row wide, the fill or the stretch solid over it
                window.paint_quad(fill(
                    at(bounds.origin.x, bounds.size.width),
                    theme.progress_bar.opacity(TRACK),
                ));
                match rule {
                    Rule::Fill(done) => {
                        let width = bounds.size.width * done.clamp(0., 1.);
                        window.paint_quad(fill(at(bounds.origin.x, width), theme.progress_bar));
                    }
                    Rule::Pulse => {
                        // a run is 1.4 s; a reduced-motion setting leaves the stretch at rest
                        let phase = match cx.reduce_motion() {
                            true => 0.,
                            false => {
                                window.request_animation_frame();
                                (PULSE_EPOCH.elapsed().as_secs_f32() / 1.4).fract()
                            }
                        };
                        let len = bounds.size.width * 0.18;
                        let from = bounds.origin.x - len + (bounds.size.width + len) * phase;
                        let left = from.max(bounds.origin.x);
                        let right = (from + len).min(bounds.origin.x + bounds.size.width);
                        if right > left {
                            window.paint_quad(fill(at(left, right - left), theme.progress_bar));
                        }
                    }
                    Rule::Hairline => {}
                }
            },
        )
        .w_full()
        .h(px(FILL))
        .flex_none();
        let node = div()
            .id(spoken
                .as_ref()
                .map_or_else(|| "rule".into(), |(id, _)| id.clone()))
            .w_full()
            .child(line);
        match spoken {
            Some((_, name)) => node
                .role(Role::ProgressIndicator)
                .aria_label(name)
                .when_some(fraction, |this, value| {
                    this.aria_numeric_value(value)
                        .aria_min_numeric_value(0.)
                        .aria_max_numeric_value(1.)
                }),
            None => node,
        }
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
    alt: Option<(ElementId, SharedString)>,
}

impl Thumbnail {
    /// A thumbnail `width` wide, 16:9. A source that is not a link is a file path.
    pub fn new(source: Option<SharedString>, width: Pixels) -> Self {
        let source = source.map(
            |s| match s.starts_with("http") || s.starts_with("file://") {
                true => s,
                false => format!("file://{s}").into(),
            },
        );
        Self {
            source,
            width,
            audio: false,
            pill: None,
            alt: None,
        }
    }

    /// Shows a note icon instead of a film strip when there is no image.
    pub fn audio(mut self, audio: bool) -> Self {
        self.audio = audio;
        self
    }

    /// Puts the cover in the accessibility tree as an image described by `alt`.
    pub fn alt(mut self, id: impl Into<ElementId>, alt: impl Into<SharedString>) -> Self {
        self.alt = Some((id.into(), alt.into()));
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
        let alt = self.alt;
        div()
            .id(alt
                .as_ref()
                .map_or_else(|| "thumbnail".into(), |(id, _)| id.clone()))
            .when_some(alt, |this, (_, alt)| this.role(Role::Image).aria_label(alt))
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
                        .px(px(4.))
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
        let id = self.id.clone();
        crate::focus::focusable(div().id(self.id.clone()), &self.id, window, cx)
            .role(Role::Button)
            .aria_label(name)
            .flex()
            .flex_none()
            .items_center()
            .gap(CHIP_GAP)
            .h(theme.metrics.control)
            .px(CHIP_INSET)
            .rounded(theme.radius)
            .border_1()
            .cursor_pointer()
            .whitespace_nowrap()
            .text_size(theme.text(Text::Label))
            // the chosen chip takes the sidebar's filled pill; the others are bare, muted text
            .border_color(gpui::transparent_black())
            .bg(match selected {
                true => theme.secondary_active,
                false => gpui::transparent_black(),
            })
            .text_color(match selected {
                true => theme.foreground,
                false => theme.muted_foreground,
            })
            .when(!selected, |this| {
                this.hover(|style| style.bg(theme.table_hover).text_color(theme.foreground))
                    .active(|style| style.bg(theme.secondary_hover))
            })
            .child(self.label)
            .when_some(self.count, |this, count| {
                this.child(
                    div()
                        .text_color(theme.muted_foreground)
                        .font_features(tabular())
                        .child(grouped(count)),
                )
            })
            .when_some(self.on_click, |this, handler| this.on_click(handler))
            .map(|this| crate::motion::pressing(this, &id, crate::motion::PRESS_SCALE, window, cx))
    }
}

/// A count with its thousands grouped, 10000 as "10,000".
// shortcut: the comma is en-US's, the only locale shipped; take the separator from Fluent when a
// second locale lands.
pub fn grouped(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A row of the app's sidebar: `icon` and `label`, muted until hovered or `on`. The open page
/// takes the quiet filled pill (`secondary_active` under foreground text, no accent), a hovered
/// row the lighter `table_hover`; focus draws the usual ring and a press scales it. On the
/// `rail` the row shows its icon only and names itself in a tooltip beside it, `tip` being that
/// tooltip's Fluent key. The caller adds a count with `.detail()` and the click.
pub fn nav_item(
    id: impl Into<ElementId>,
    label: SharedString,
    icon: &'static str,
    tip: &'static str,
    on: bool,
    rail: bool,
    cx: &App,
) -> crate::Button {
    let theme = cx.theme();
    let row = crate::Button::new(id)
        .ghost()
        .icon(icon)
        .tint(match on {
            true => theme.foreground,
            false => theme.muted_foreground,
        })
        .selected(on)
        .lit(on, theme.secondary_active)
        .w_full()
        .h(theme.metrics.control + px(4.));
    match rail {
        true => row.name(label).tooltip_trailing(tip),
        false => row
            .label(label)
            .px(theme.metrics.pad)
            .gap(px(10.))
            .justify_start(),
    }
}

/// A search field plus the tokens it parsed, valid ones ink-blue and unparsed ones red.
// shortcut: tokens are listed beside the field, not drawn inside its text, move them in when
// the input element supports styled runs.
#[derive(IntoElement)]
pub struct TokenInput {
    input: Entity<Input>,
    tokens: Vec<(SharedString, bool)>,
    spoken: Option<(ElementId, SharedString)>,
}

impl TokenInput {
    /// Wraps `input`; `tokens` are the `key:value` terms as typed and whether each parsed.
    pub fn new(input: &Entity<Input>, tokens: Vec<(SharedString, bool)>) -> Self {
        Self {
            input: input.clone(),
            tokens,
            spoken: None,
        }
    }

    /// Puts the field and its tokens in the accessibility tree as a search region named `name`.
    pub fn spoken(mut self, id: impl Into<ElementId>, name: impl Into<SharedString>) -> Self {
        self.spoken = Some((id.into(), name.into()));
        self
    }
}

impl RenderOnce for TokenInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let spoken = self.spoken;
        div()
            .id(spoken
                .as_ref()
                .map_or_else(|| "token-input".into(), |(id, _)| id.clone()))
            .when_some(spoken, |this, (_, name)| {
                this.role(Role::Search).aria_label(name)
            })
            .flex()
            .flex_1()
            .min_w_0()
            .justify_end()
            .items_center()
            .gap(px(6.))
            .children(self.tokens.into_iter().map(|(text, valid)| {
                div()
                    .px(px(4.))
                    .rounded(theme.radius)
                    .font_family(MONO)
                    .text_size(theme.text(Text::Small))
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
            .child(
                div()
                    .flex_1()
                    .min_w(px(SEARCH_MIN))
                    .max_w(px(SEARCH_MAX))
                    .child(self.input),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::grouped;

    #[test]
    fn counts_group_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(10000), "10,000");
        assert_eq!(grouped(1234567), "1,234,567");
    }
}
