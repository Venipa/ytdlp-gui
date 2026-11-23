//! The detail panel: everything known about the selected download.

use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, px};
use i18n::t;
use ui::{ActiveTheme as _, Button, CallNumber, MONO, Panel, Side, Text, Thumbnail};

use crate::downloads::{Downloads, err_key};
use crate::sample::{Data, Row};

/// The 360 px right panel for `row`.
pub fn panel(row: &Row, data: &Data, cx: &mut Context<Downloads>) -> AnyElement {
    let theme = *cx.theme();
    let s = &row.summary;
    let pencil = theme.muted_foreground;
    let line = |label: SharedString, value: AnyElement| {
        div()
            .flex()
            .gap_3()
            .child(div().w(px(64.)).flex_none().text_color(pencil).child(label))
            .child(div().flex_1().min_w_0().child(value))
    };
    let mono = |text: String| {
        div()
            .font_family(MONO)
            .text_size(theme.text(Text::Small))
            .child(text)
            .into_any_element()
    };
    let path = s.file_path.clone().unwrap_or_else(|| {
        format!(
            "D:\\Videos\\ytdlpd\\{} [{}].mp4",
            s.title.clone().unwrap_or_default(),
            s.video_id.clone().unwrap_or_default()
        )
    });
    let error = match &s.error_code {
        Some(code) => div()
            .text_color(theme.danger)
            .child(i18n::lookup(err_key(code), None))
            .into_any_element(),
        None => div()
            .text_color(pencil)
            .child(t!("dl-detail-none"))
            .into_any_element(),
    };
    Panel::new("detail-panel", Side::Right, px(360.))
        .limits(px(280.), px(560.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .p(theme.metrics.pad)
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .items_center()
                                .child(div().text_color(pencil).child(t!("dl-detail-no", id = "")))
                                .child(CallNumber::new(s.id.0).selected(true)),
                        )
                        .child(
                            Button::new("detail-close")
                                .ghost()
                                .small()
                                .icon("icons/x.svg")
                                .tooltip("dl-close")
                                .on_click(cx.listener(|this, _, _, cx| this.deselect(cx))),
                        ),
                )
                .child(Thumbnail::new(None, px(328.)))
                .child(
                    div()
                        .text_size(theme.text(Text::Heading))
                        .font_weight(FontWeight::MEDIUM)
                        .child(s.title.clone().unwrap_or_default()),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(div().truncate().child(mono(s.url.clone())))
                        .child(
                            Button::new("detail-copy")
                                .ghost()
                                .small()
                                .label(t!("dl-copy")),
                        ),
                )
                .child(line(t!("dl-detail-file"), mono(path)))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("show-folder")
                                .outline()
                                .small()
                                .label(t!("dl-show-folder")),
                        )
                        .child(
                            Button::new("open-file")
                                .outline()
                                .small()
                                .label(t!("dl-open")),
                        ),
                )
                .child(line(
                    t!("dl-detail-format"),
                    mono(s.format_label.clone().unwrap_or_else(|| "-".into())),
                ))
                .child(line(t!("dl-detail-options"), mono(s.preset.clone())))
                .child(line(
                    t!("dl-detail-attempts"),
                    div()
                        .font_features(ui::tabular())
                        .child(s.attempts.to_string())
                        .into_any_element(),
                ))
                .child(line(t!("dl-detail-error"), error))
                .child(div().text_color(pencil).child(t!("dl-detail-log")))
                .children(
                    data.logs
                        .iter()
                        .filter(|l| l.job == Some(s.id.0))
                        .take(20)
                        .map(|l| {
                            div()
                                .flex()
                                .gap_2()
                                .font_family(MONO)
                                .text_size(theme.text(Text::Tiny))
                                .child(div().text_color(pencil).child(l.time))
                                .child(div().truncate().child(l.text))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("delete-entry")
                                .outline()
                                .label(t!("dl-delete-entry")),
                        )
                        .child(
                            Button::new("delete-file")
                                .destructive()
                                .label(t!("dl-delete-file")),
                        ),
                ),
        )
        .into_any_element()
}
