//! The one Page / Section / Row layout every settings page shares: a centred column, sections
//! with a small heading, and rows holding a label, a description and a control.

use gpui::prelude::*;
use gpui::{AnyElement, App, FontWeight, SharedString, div, px};
use ui::{ActiveTheme as _, Text};

/// The width of the column the pages are centred in.
const COLUMN: f32 = 720.;
/// How wide the label side of a row stays at least, so a wide control never squeezes it away.
const LABEL_MIN: f32 = 220.;

/// The page: `title`, an optional notice under it, then the sections, in a column centred in the
/// room. The caller puts it in a scroller.
pub fn page(
    title: SharedString,
    notice: Option<AnyElement>,
    sections: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    div()
        .w_full()
        .flex()
        .justify_center()
        .p(theme.metrics.inset)
        .child(
            div()
                .w_full()
                .max_w(px(COLUMN))
                .flex()
                .flex_col()
                .gap(px(24.))
                .child(
                    div()
                        .text_size(theme.text(Text::Title))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .children(notice)
                .children(sections),
        )
        .into_any_element()
}

/// A group of rows in one bordered card under a small heading.
pub fn section(title: Option<SharedString>, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .children(title.map(|title| {
            div()
                .px(px(2.))
                .text_size(theme.text(Text::Label))
                .text_color(theme.muted_foreground)
                .font_weight(FontWeight::MEDIUM)
                .child(title)
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .children(rows.into_iter().enumerate().map(|(at, row)| {
                    div()
                        .px(px(12.))
                        .when(at > 0, |this| this.border_t_1().border_color(theme.border))
                        .child(row)
                })),
        )
        .into_any_element()
}

/// A row: label and description at the start, `control` at the end.
pub fn row(
    label: SharedString,
    detail: Option<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .min_h(px(52.))
        .py(px(10.))
        .child(text(label, detail, cx).flex_1().min_w(px(LABEL_MIN)))
        .child(div().flex_none().child(control))
        .text_size(theme.text(Text::Body))
        .into_any_element()
}

/// A row whose control is wide (a line of chips, a list): the control sits under the text.
pub fn stacked(
    label: SharedString,
    detail: Option<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .py(px(12.))
        .child(text(label, detail, cx))
        .child(control)
        .text_size(theme.text(Text::Body))
        .into_any_element()
}

fn text(label: SharedString, detail: Option<SharedString>, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().child(label))
        .children(detail.map(|detail| {
            div()
                .text_size(theme.text(Text::Small))
                .text_color(theme.muted_foreground)
                .child(detail)
        }))
}
