//! The list rows: compact single-line rows (default) and ledger rows with thumbnails, the group
//! headers, the right-click menu and the delete-file confirmation.

use domain::{DownloadId, DownloadStatus, DownloadSummary, MediaKind};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, MouseButton, MouseDownEvent, SharedString, div, px,
};
use i18n::t;
use ui::{
    ActiveTheme as _, Button, CallNumber, Menu, MenuItem, Modal, Popup, ProgressRule, Rule, Text,
    Thumbnail, tabular,
};

use crate::downloads::{Downloads, Line, error_text_of};
use crate::format;

/// The row height of the list (headers included, the list is uniform).
fn height(thumbs: bool) -> gpui::Pixels {
    match thumbs {
        true => px(54.),
        false => px(32.),
    }
}

/// The status words of a row and the colour they take.
fn status_text(s: &DownloadSummary, theme: &ui::Theme) -> (SharedString, Hsla) {
    let pencil = theme.muted_foreground;
    match s.status {
        DownloadStatus::Queued => (t!("dl-status-queued"), pencil),
        DownloadStatus::Probing => (t!("dl-status-probing"), pencil),
        DownloadStatus::Downloading => (t!("dl-status-downloading"), theme.progress_bar),
        DownloadStatus::Processing => match s.kind {
            MediaKind::Audio => (
                t!(
                    "dl-status-converting",
                    format = s.preset.split(':').nth(1).unwrap_or("mp3")
                ),
                theme.progress_bar,
            ),
            MediaKind::Video => (t!("dl-status-merging"), theme.progress_bar),
        },
        DownloadStatus::Paused if s.status_reason.as_deref() == Some("interrupted") => {
            (t!("dl-status-interrupted"), pencil)
        }
        DownloadStatus::Paused => (t!("dl-status-paused"), pencil),
        DownloadStatus::Completed if s.file_missing => (t!("dl-file-missing"), theme.danger),
        DownloadStatus::Completed => (t!("dl-status-done"), pencil),
        DownloadStatus::Failed => {
            let why = s
                .error_code
                .as_deref()
                .map(error_text_of)
                .unwrap_or_else(|| t!("dl-err-unknown"));
            (
                t!("dl-status-failed-why", reason = why.as_ref()),
                theme.danger,
            )
        }
        DownloadStatus::Cancelled => (t!("dl-status-cancelled"), pencil),
    }
}

fn rule(s: &DownloadSummary) -> Rule {
    match s.status {
        DownloadStatus::Downloading | DownloadStatus::Processing => Rule::Fill(s.progress as f32),
        DownloadStatus::Completed => Rule::Solid,
        DownloadStatus::Failed => Rule::Failed,
        _ => Rule::Dotted,
    }
}

impl Downloads {
    /// One list line.
    pub(crate) fn line(&mut self, line: Line, thumbs: bool, cx: &mut Context<Self>) -> AnyElement {
        match line {
            Line::Header(key, count) => self.header(key, count, thumbs, cx),
            Line::Row(at) => {
                let Some(s) = self.app.downloads.read(cx).rows().get(at).cloned() else {
                    return div().h(height(thumbs)).into_any_element();
                };
                match thumbs {
                    true => self.ledger_row(&s, cx),
                    false => self.compact_row(&s, cx),
                }
            }
        }
    }

    fn header(
        &self,
        key: &'static str,
        count: usize,
        thumbs: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        div()
            .flex()
            .flex_none()
            .items_end()
            .gap_2()
            .h(height(thumbs))
            .w_full()
            .px(theme.metrics.pad)
            .pb_1()
            .text_size(theme.text(Text::Label))
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(i18n::lookup(key, None)),
            )
            .child(div().font_features(tabular()).child(count.to_string()))
            .into_any_element()
    }

    /// What the row is when clicked, right-clicked, hovered.
    fn shell(
        &self,
        s: &DownloadSummary,
        thumbs: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = *cx.theme();
        let id = s.id;
        let selected = self.selected == Some(id);
        div()
            .id(("row", id.0 as usize))
            .flex()
            .flex_col()
            .flex_none()
            .h(height(thumbs))
            .w_full()
            .cursor_pointer()
            .when(selected, |this| this.bg(theme.table_active))
            .hover(|style| style.bg(theme.table_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(id);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.selected = Some(id);
                    this.menu = Some((event.position, id));
                    cx.notify();
                }),
            )
    }

    fn numbers(&self, s: &DownloadSummary, cx: &mut Context<Self>) -> (String, String) {
        let size = s
            .file_size
            .or(s.bytes_total)
            .map(format::bytes)
            .unwrap_or_else(|| "-".into());
        let live = self.app.downloads.read(cx).live(s.id).copied();
        let third = match (live.and_then(|l| l.speed), s.status) {
            (Some(speed), DownloadStatus::Downloading) => {
                let eta = live
                    .and_then(|l| l.eta)
                    .map(|e| format!("  {}", format::duration(e)))
                    .unwrap_or_default();
                format!("{}{eta}", format::speed(speed))
            }
            _ => String::new(),
        };
        (size, third)
    }

    fn compact_row(&self, s: &DownloadSummary, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let selected = self.selected == Some(s.id);
        let (status, tone) = status_text(s, &theme);
        let (size, third) = self.numbers(s, cx);
        let active = matches!(
            s.status,
            DownloadStatus::Downloading | DownloadStatus::Processing
        );
        self.shell(s, false, cx)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap_3()
                    .px(theme.metrics.pad)
                    .child(CallNumber::new(s.id.0).selected(selected))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .truncate()
                                    .child(s.title.clone().unwrap_or_else(|| s.url.clone())),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(140.))
                                    .truncate()
                                    .text_size(theme.text(Text::Small))
                                    .text_color(theme.muted_foreground)
                                    .child(s.domain.clone()),
                            ),
                    )
                    .child(
                        div()
                            .w(px(88.))
                            .flex_none()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(s.format_label.clone().unwrap_or_else(|| "-".into())),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .flex_none()
                            .font_features(tabular())
                            .child(size),
                    )
                    .child(
                        div()
                            .w(px(108.))
                            .flex_none()
                            .truncate()
                            .font_features(tabular())
                            .text_color(theme.muted_foreground)
                            .child(third),
                    )
                    .child(
                        div()
                            .w(px(190.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .text_color(tone)
                            .child(div().truncate().child(status))
                            .when(active, |this| {
                                this.child(
                                    div()
                                        .w(px(36.))
                                        .flex_none()
                                        .text_right()
                                        .font_features(tabular())
                                        .child(format!("{:.0}%", s.progress * 100.)),
                                )
                            }),
                    ),
            )
            .child(ProgressRule::new(rule(s)))
            .into_any_element()
    }

    fn ledger_row(&self, s: &DownloadSummary, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let id = s.id;
        let selected = self.selected == Some(id);
        let (status, tone) = status_text(s, &theme);
        let (size, third) = self.numbers(s, cx);
        let active = matches!(
            s.status,
            DownloadStatus::Downloading | DownloadStatus::Processing
        );
        let failed = s.status == DownloadStatus::Failed;
        self.shell(s, true, cx)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap_3()
                    .px(theme.metrics.pad)
                    .child(CallNumber::new(id.0).selected(selected))
                    .child(Thumbnail::new(
                        s.thumbnail_path
                            .clone()
                            .or_else(|| s.thumbnail_url.clone())
                            .map(SharedString::from),
                        px(80.),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(theme.text(Text::Heading))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(s.title.clone().unwrap_or_else(|| s.url.clone())),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(theme.text(Text::Small))
                                    .text_color(theme.muted_foreground)
                                    .child(s.domain.clone()),
                            ),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .text_color(theme.muted_foreground)
                            .child(s.format_label.clone().unwrap_or_else(|| "-".into())),
                    )
                    .child(div().w(px(72.)).font_features(tabular()).child(size))
                    .child(
                        div()
                            .w(px(104.))
                            .font_features(tabular())
                            .text_color(theme.muted_foreground)
                            .child(third),
                    )
                    .child(
                        div()
                            .w(px(200.))
                            .flex()
                            .justify_end()
                            .gap_2()
                            .text_color(tone)
                            .child(div().truncate().child(status))
                            .when(active, |this| {
                                this.child(
                                    div()
                                        .w(px(36.))
                                        .text_right()
                                        .font_features(tabular())
                                        .child(format!("{:.0}%", s.progress * 100.)),
                                )
                            })
                            .when(failed, |this| {
                                this.child(
                                    Button::new(("retry", id.0 as usize))
                                        .small()
                                        .outline()
                                        .label(t!("dl-try-again"))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| this.retry(id, cx)),
                                        ),
                                )
                            }),
                    ),
            )
            .child(ProgressRule::new(rule(s)))
            .into_any_element()
    }

    /// The right-click menu of a row.
    pub(crate) fn row_menu(&self, cx: &mut Context<Self>) -> Option<Popup> {
        let (at, id) = self.menu?;
        let s = self.summary(id, cx)?;
        let this = cx.entity();
        let act = |f: fn(&mut Downloads, DownloadId, &mut Context<Downloads>)| {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut gpui::Window, cx: &mut gpui::App| {
                this.update(cx, |this, cx| {
                    this.menu = None;
                    f(this, id, cx)
                })
            }
        };
        let status = s.status;
        let has_file = s.file_path.is_some();
        let mut items = Vec::new();
        let item = |key: &'static str, label: SharedString, icon: &'static str| {
            MenuItem::new(key, label).icon(icon)
        };
        match status {
            DownloadStatus::Downloading | DownloadStatus::Processing | DownloadStatus::Probing => {
                items.push(
                    item("pause", t!("dl-pause"), "icons/pause.svg")
                        .on_click(act(|t, id, cx| t.pause(id, cx))),
                );
            }
            DownloadStatus::Queued => items.push(
                item("pause", t!("dl-pause"), "icons/pause.svg")
                    .on_click(act(|t, id, cx| t.pause(id, cx))),
            ),
            DownloadStatus::Paused => items.push(
                item("resume", t!("dl-resume"), "icons/play.svg")
                    .on_click(act(|t, id, cx| t.resume(id, cx))),
            ),
            DownloadStatus::Failed | DownloadStatus::Cancelled => items.push(
                item("retry", t!("dl-try-again"), "icons/rotate-cw.svg")
                    .on_click(act(|t, id, cx| t.retry(id, cx))),
            ),
            DownloadStatus::Completed => {}
        }
        if !status.is_terminal() {
            items.push(
                item("cancel", t!("dl-cancel-download"), "icons/x.svg")
                    .on_click(act(|t, id, cx| t.cancel(id, cx))),
            );
        }
        if has_file {
            items.push(
                item("folder", t!("dl-show-folder"), "icons/folder-open.svg")
                    .on_click(act(|t, id, cx| t.show_folder(id, cx))),
            );
        }
        if status == DownloadStatus::Completed && has_file && !s.file_missing {
            items.push(
                item("open", t!("dl-open"), "icons/external-link.svg")
                    .on_click(act(|t, id, cx| t.open_file(id, cx))),
            );
        }
        items.push(MenuItem::separator("sep-copy"));
        items.push(
            item("copy-title", t!("dl-copy-title"), "icons/copy.svg")
                .on_click(act(|t, id, cx| t.copy_title(id, cx))),
        );
        items.push(
            item("copy-link", t!("dl-copy-link"), "icons/link.svg")
                .on_click(act(|t, id, cx| t.copy_link(id, cx))),
        );
        if !status.is_active() {
            items.push(MenuItem::separator("sep-delete"));
            items.push(
                item("delete-entry", t!("dl-delete-entry"), "icons/trash-2.svg")
                    .on_click(act(|t, id, cx| t.delete_entry(id, cx))),
            );
            if has_file {
                items.push(
                    item("delete-file", t!("dl-delete-file"), "icons/trash-2.svg")
                        .on_click(act(|t, id, cx| t.ask_delete_file(id, cx))),
                );
            }
        }
        Some(
            Popup::new(at, Menu::new("row-menu").items(items)).on_close(cx.listener(
                |this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                },
            )),
        )
    }

    /// The confirmation before a file is deleted, naming the file.
    pub(crate) fn confirm_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.confirm?;
        let s = self.summary(id, cx)?;
        let path = s.file_path?;
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(path);
        Some(
            Modal::new("confirm-delete-file", t!("dl-confirm-title"))
                .detail(t!("dl-confirm-detail", name = name.as_str()))
                .on_dismiss(cx.listener(|this, _, _, cx| {
                    this.confirm = None;
                    cx.notify();
                }))
                .action(
                    Button::new("confirm-keep")
                        .ghost()
                        .label(t!("dl-keep-file"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm = None;
                            cx.notify();
                        })),
                )
                .action(
                    Button::new("confirm-delete")
                        .destructive()
                        .label(t!("dl-delete-file"))
                        .on_click(cx.listener(move |this, _, _, cx| this.delete_file(id, cx))),
                )
                .into_any_element(),
        )
    }
}
