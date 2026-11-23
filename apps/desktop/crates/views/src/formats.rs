//! The Choose formats modal.

use domain::{FormatInfo, ProbeResult};
use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, div, px};
use i18n::t;
use ui::{ActiveTheme as _, Button, MONO, Modal, Text, tabular};

use crate::downloads::Downloads;
use crate::format;

fn table(title: SharedString, formats: Vec<&FormatInfo>, cx: &Context<Downloads>) -> AnyElement {
    let theme = *cx.theme();
    let pencil = theme.muted_foreground;
    let cell = |w: f32, text: String| div().w(px(w)).child(text);
    let mono = |w: f32, text: String| {
        div()
            .w(px(w))
            .font_family(MONO)
            .text_size(theme.text(Text::Small))
            .child(text)
    };
    div()
        .flex()
        .flex_col()
        .child(div().py_1().text_color(pencil).child(title))
        .child(
            div()
                .flex()
                .h(px(32.))
                .items_center()
                .text_color(pencil)
                .text_size(theme.text(Text::Label))
                .child(cell(60., t!("dl-col-id").to_string()))
                .child(cell(60., t!("dl-col-ext").to_string()))
                .child(cell(110., t!("dl-col-resolution").to_string()))
                .child(cell(110., t!("dl-col-codec").to_string()))
                .child(cell(80., t!("dl-col-size").to_string()))
                .child(cell(80., t!("dl-col-note").to_string())),
        )
        .children(formats.into_iter().enumerate().map(|(i, f)| {
            let resolution = match (f.width, f.height) {
                (Some(w), Some(h)) => format!("{w}x{h}"),
                _ => "-".into(),
            };
            let codec = f
                .vcodec
                .clone()
                .or_else(|| f.acodec.clone())
                .unwrap_or_default();
            div()
                .flex()
                .h(px(32.))
                .items_center()
                .when(i == 0, |this| this.bg(theme.table_active))
                .border_b_1()
                .border_color(theme.table_row_border)
                .child(mono(60., f.id.clone()))
                .child(cell(60., f.ext.clone().unwrap_or_default()))
                .child(cell(110., resolution))
                .child(mono(110., codec))
                .child(
                    div().w(px(80.)).font_features(tabular()).child(
                        f.filesize
                            .map(|n| format::bytes(n as i64))
                            .unwrap_or_default(),
                    ),
                )
                .child(cell(80., f.note.clone().unwrap_or_default()))
        }))
        .into_any_element()
}

/// The modal over `probe`'s formats.
pub fn modal(probe: &ProbeResult, cx: &mut Context<Downloads>) -> AnyElement {
    let theme = *cx.theme();
    let video: Vec<_> = probe
        .formats
        .iter()
        .filter(|f| f.vcodec.is_some())
        .collect();
    let audio: Vec<_> = probe
        .formats
        .iter()
        .filter(|f| f.acodec.is_some())
        .collect();
    Modal::new("choose-formats-modal", t!("dl-formats-title"))
        .wide()
        .close_button()
        .on_dismiss(cx.listener(|this, _, _, cx| this.close_formats(cx)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .child(table(t!("dl-formats-video"), video, cx))
                .child(table(t!("dl-formats-audio"), audio, cx))
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(t!("dl-formats-result")),
                        )
                        .child(div().font_family(MONO).child("137+140 -> mp4")),
                ),
        )
        .action(
            Button::new("formats-cancel")
                .ghost()
                .label(t!("dl-cancel"))
                .on_click(cx.listener(|this, _, _, cx| this.close_formats(cx))),
        )
        .action(
            Button::new("formats-download")
                .primary()
                .label(t!("dl-download"))
                .on_click(cx.listener(|this, _, _, cx| this.close_formats(cx))),
        )
        .into_any_element()
}
