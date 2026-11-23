//! The detail panel: everything known about the selected download, and its actions.

use domain::{DownloadStatus, DownloadSummary};
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, px};
use i18n::t;
use ui::{ActiveTheme as _, Button, CallNumber, MONO, Panel, Side, Text, Thumbnail, tabular};

use crate::downloads::{Downloads, error_text_of};
use crate::format;

impl Downloads {
    /// The 360 px right panel for `s`.
    pub(crate) fn detail_panel(&self, s: &DownloadSummary, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let pencil = theme.muted_foreground;
        let id = s.id;
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
        let error = match (&s.error_code, &s.error_message) {
            (Some(code), message) => div()
                .flex()
                .flex_col()
                .gap_1()
                .text_color(theme.danger)
                .child(error_text_of(code))
                .children(message.clone().map(|m| {
                    div()
                        .font_family(MONO)
                        .text_size(theme.text(Text::Tiny))
                        .text_color(pencil)
                        .child(m)
                }))
                .into_any_element(),
            _ => div()
                .text_color(pencil)
                .child(t!("dl-detail-none"))
                .into_any_element(),
        };
        let logs: Vec<_> = self
            .app
            .logs
            .read(cx)
            .for_job(id)
            .map(|l| (format::clock(l.at), l.message.clone()))
            .collect();
        let skip = logs.len().saturating_sub(40);
        let status = s.status;
        let primary = match status {
            DownloadStatus::Paused => Some(
                Button::new("detail-resume")
                    .outline()
                    .small()
                    .label(t!("dl-resume"))
                    .on_click(cx.listener(move |this, _, _, cx| this.resume(id, cx))),
            ),
            DownloadStatus::Failed | DownloadStatus::Cancelled => Some(
                Button::new("detail-retry")
                    .outline()
                    .small()
                    .label(t!("dl-try-again"))
                    .on_click(cx.listener(move |this, _, _, cx| this.retry(id, cx))),
            ),
            s if !s.is_terminal() => Some(
                Button::new("detail-pause")
                    .outline()
                    .small()
                    .label(t!("dl-pause"))
                    .on_click(cx.listener(move |this, _, _, cx| this.pause(id, cx))),
            ),
            _ => None,
        };
        let has_file = s.file_path.is_some();
        Panel::new("detail-panel", Side::Right, px(360.))
            .limits(px(280.), px(560.))
            .child(
                div()
                    .id("detail-scroll")
                    .size_full()
                    .overflow_y_scroll()
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
                                            .child(
                                                div()
                                                    .text_color(pencil)
                                                    .child(t!("dl-detail-no", id = "")),
                                            )
                                            .child(CallNumber::new(id.0).selected(true)),
                                    )
                                    .child(
                                        Button::new("detail-close")
                                            .ghost()
                                            .small()
                                            .icon("icons/x.svg")
                                            .tooltip("dl-close")
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.deselect(cx)),
                                            ),
                                    ),
                            )
                            .child(Thumbnail::new(
                                s.thumbnail_path
                                    .clone()
                                    .or_else(|| s.thumbnail_url.clone())
                                    .map(SharedString::from),
                                px(328.),
                            ))
                            .child(
                                div()
                                    .text_size(theme.text(Text::Heading))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(s.title.clone().unwrap_or_else(|| s.url.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_2()
                                    .child(div().truncate().child(mono(s.url.clone())))
                                    .child(
                                        Button::new("detail-copy")
                                            .ghost()
                                            .small()
                                            .label(t!("dl-copy"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.copy_link(id, cx)
                                            })),
                                    ),
                            )
                            .children(
                                s.file_path
                                    .clone()
                                    .map(|path| line(t!("dl-detail-file"), mono(path))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .children(primary)
                                    .when(has_file, |this| {
                                        this.child(
                                            Button::new("show-folder")
                                                .outline()
                                                .small()
                                                .label(t!("dl-show-folder"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.show_folder(id, cx)
                                                })),
                                        )
                                    })
                                    .when(has_file && status == DownloadStatus::Completed, |this| {
                                        this.child(
                                            Button::new("open-file")
                                                .outline()
                                                .small()
                                                .label(t!("dl-open"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.open_file(id, cx)
                                                })),
                                        )
                                    })
                                    .when(!status.is_terminal(), |this| {
                                        this.child(
                                            Button::new("detail-cancel")
                                                .ghost()
                                                .small()
                                                .label(t!("dl-cancel-download"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.cancel(id, cx)
                                                })),
                                        )
                                    }),
                            )
                            .child(line(
                                t!("dl-detail-format"),
                                mono(s.format_label.clone().unwrap_or_else(|| "-".into())),
                            ))
                            .child(line(t!("dl-detail-options"), mono(s.preset.clone())))
                            .child(line(
                                t!("dl-detail-attempts"),
                                div()
                                    .font_features(tabular())
                                    .child(s.attempts.to_string())
                                    .into_any_element(),
                            ))
                            .child(line(t!("dl-detail-error"), error))
                            .child(div().text_color(pencil).child(t!("dl-detail-log")))
                            .children(logs.into_iter().skip(skip).map(|(time, text)| {
                                div()
                                    .flex()
                                    .gap_2()
                                    .font_family(MONO)
                                    .text_size(theme.text(Text::Tiny))
                                    .child(div().flex_none().text_color(pencil).child(time))
                                    .child(div().min_w_0().child(text))
                            }))
                            .when(!status.is_active(), |this| {
                                this.child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(
                                            Button::new("delete-entry")
                                                .outline()
                                                .label(t!("dl-delete-entry"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.delete_entry(id, cx)
                                                })),
                                        )
                                        .when(has_file, |this| {
                                            this.child(
                                                Button::new("delete-file")
                                                    .destructive()
                                                    .label(t!("dl-delete-file"))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.ask_delete_file(id, cx)
                                                        },
                                                    )),
                                            )
                                        }),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}
