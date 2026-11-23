//! The list rows: compact single-line rows (default) and ledger rows with thumbnails, the group
//! headers, the right-click menu and the delete-file confirmation.

use domain::{DownloadId, DownloadStatus, DownloadSummary, MediaKind};
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, FontWeight, Hsla, MouseButton,
    MouseDownEvent, SharedString, div, px,
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
pub(crate) fn status_text(s: &DownloadSummary, theme: &ui::Theme) -> (SharedString, Hsla) {
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

type Act = fn(&mut Downloads, DownloadId, &mut Context<Downloads>);

/// Which columns a row has room for: the list drops the format, then the speed, then the call
/// number as it gets narrower, so the title keeps room to be read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Columns {
    pub call: bool,
    pub format: bool,
    pub speed: bool,
    /// The width of the status column.
    pub status: f32,
}

impl Columns {
    /// The columns for a list `width` wide.
    pub(crate) fn for_width(width: gpui::Pixels) -> Self {
        let w = f32::from(width);
        Self {
            call: w >= 560.,
            format: w >= 900.,
            speed: w >= 720.,
            status: if w >= 720. { 190. } else { 150. },
        }
    }
}

fn rule(s: &DownloadSummary) -> Rule {
    match s.status {
        DownloadStatus::Downloading | DownloadStatus::Processing => Rule::Fill(s.progress as f32),
        DownloadStatus::Completed => Rule::Solid,
        DownloadStatus::Probing => Rule::Pulse,
        DownloadStatus::Failed => Rule::Failed,
        _ => Rule::Dotted,
    }
}

impl Downloads {
    /// One list line.
    pub(crate) fn line(
        &mut self,
        line: Line,
        thumbs: bool,
        ring: bool,
        cols: Columns,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match line {
            Line::Header(key, count) => self.header(key, count, thumbs, cx),
            Line::Row(at) => {
                let Some(s) = self.app.downloads.read(cx).rows().get(at).cloned() else {
                    return div().h(height(thumbs)).into_any_element();
                };
                self.row(&s, thumbs, ring, cols, cx)
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
            .px(theme.metrics.gutter)
            .pb(px(6.))
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
        ring: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = *cx.theme();
        let id = s.id;
        let selected = self.selected == Some(id);
        div()
            .id(("row", id.0 as usize))
            .group(SharedString::from(format!("row-{}", id.0)))
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h(height(thumbs))
            .w_full()
            .cursor_pointer()
            .when(!selected, |this| {
                this.hover(|style| style.bg(theme.table_hover))
                    .active(|style| style.bg(theme.secondary_hover))
            })
            .when(selected, |this| {
                // the selected row: an ink wash and an ink edge at its start
                this.bg(theme.table_active).child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(theme.table_active_border),
                )
            })
            .when(selected && ring, |this| {
                this.border_1().border_color(theme.table_active_border)
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = Some(id);
                window.focus(&this.list_focus, cx);
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

    /// The icon buttons that appear over a hovered row, for what its state allows.
    fn row_actions(&self, s: &DownloadSummary, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let id = s.id;
        let button = |key: &'static str,
                      icon: &'static str,
                      tip: &'static str,
                      act: Act,
                      cx: &mut Context<Self>| {
            Button::new((key, id.0 as usize))
                .ghost()
                .small()
                .icon(icon)
                .tooltip(tip)
                .on_click(cx.listener(move |this, _, _, cx| act(this, id, cx)))
        };
        let has_file = s.file_path.is_some() && !s.file_missing;
        let mut buttons = Vec::new();
        match s.status {
            DownloadStatus::Paused => buttons.push(button(
                "a-resume",
                "icons/play.svg",
                "dl-resume",
                |t, id, cx| t.resume(id, cx),
                cx,
            )),
            DownloadStatus::Failed | DownloadStatus::Cancelled => buttons.push(button(
                "a-retry",
                "icons/rotate-cw.svg",
                "dl-try-again",
                |t, id, cx| t.retry(id, cx),
                cx,
            )),
            DownloadStatus::Completed => {}
            _ => buttons.push(button(
                "a-pause",
                "icons/pause.svg",
                "dl-pause",
                |t, id, cx| t.pause(id, cx),
                cx,
            )),
        }
        if !s.status.is_terminal() {
            buttons.push(button(
                "a-cancel",
                "icons/x.svg",
                "dl-cancel-download",
                |t, id, cx| t.cancel(id, cx),
                cx,
            ));
        }
        if has_file {
            buttons.push(button(
                "a-folder",
                "icons/folder-open.svg",
                "dl-show-folder",
                |t, id, cx| t.show_folder(id, cx),
                cx,
            ));
        }
        div()
            .absolute()
            .right(theme.metrics.gutter - px(4.))
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(4.))
            .bg(theme.background.blend(match self.selected == Some(id) {
                true => theme.table_active,
                false => theme.table_hover,
            }))
            .invisible()
            .group_hover(SharedString::from(format!("row-{}", id.0)), |style| {
                style.visible()
            })
            .children(buttons)
            .into_any_element()
    }

    fn row(
        &self,
        s: &DownloadSummary,
        thumbs: bool,
        ring: bool,
        cols: Columns,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let id = s.id;
        let selected = self.selected == Some(id);
        let (status, tone) = status_text(s, &theme);
        let (size, third) = self.numbers(s, cx);
        let active = matches!(
            s.status,
            DownloadStatus::Downloading | DownloadStatus::Processing
        );
        let title = s.title.clone().unwrap_or_else(|| s.url.clone());
        let actions = self.row_actions(s, cx);
        let thumb = s
            .thumbnail_path
            .clone()
            .or_else(|| s.thumbnail_url.clone())
            .map(SharedString::from);
        let fade = self.fresh.contains_key(&id) && ui::motion::animates(cx);
        let muted = theme.muted_foreground;
        let row = self
            .shell(s, thumbs, ring, cx)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap(px(12.))
                    .px(theme.metrics.gutter)
                    .when(cols.call, |this| {
                        this.child(CallNumber::new(id.0).selected(selected))
                    })
                    .when(thumbs, |this| {
                        this.child(Thumbnail::new(thumb, px(72.)).audio(s.kind == MediaKind::Audio))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .when(thumbs, |this| this.flex_col().justify_center().gap(px(2.)))
                            .when(!thumbs, |this| this.items_baseline().gap(px(8.)))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .when(thumbs, |this| {
                                        this.text_size(theme.text(Text::Heading))
                                            .font_weight(FontWeight::MEDIUM)
                                    })
                                    .child(title),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(160.))
                                    .truncate()
                                    .text_size(theme.text(Text::Small))
                                    .text_color(muted)
                                    .child(s.domain.clone()),
                            ),
                    )
                    .when(cols.format, |this| {
                        this.child(
                            div()
                                .w(px(88.))
                                .flex_none()
                                .truncate()
                                .text_color(muted)
                                .child(s.format_label.clone().unwrap_or_else(|| "-".into())),
                        )
                    })
                    .child(
                        div()
                            .w(px(72.))
                            .flex_none()
                            .whitespace_nowrap()
                            .text_right()
                            .font_features(tabular())
                            .text_color(muted)
                            .child(size),
                    )
                    .when(cols.speed, |this| {
                        this.child(
                            div()
                                .w(px(112.))
                                .flex_none()
                                .truncate()
                                .text_right()
                                .font_features(tabular())
                                .text_color(muted)
                                .child(third),
                        )
                    })
                    .child(
                        div()
                            .w(px(cols.status))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .text_color(tone)
                            .child(div().min_w_0().truncate().child(status))
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
                    )
                    .child(actions),
            )
            .child(
                div()
                    .px(theme.metrics.gutter)
                    .child(ProgressRule::new(rule(s))),
            );
        match fade {
            true => row
                .with_animation(
                    ("row-in", id.0 as usize),
                    Animation::new(crate::downloads::FADE).with_easing(ui::ease_out_cubic),
                    |this, delta| this.opacity(delta),
                )
                .into_any_element(),
            false => row.into_any_element(),
        }
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

#[cfg(test)]
mod tests {
    use super::Columns;
    use gpui::px;

    #[test]
    fn columns_drop_as_the_list_narrows() {
        let wide = Columns::for_width(px(1240.));
        assert!(wide.call && wide.format && wide.speed);
        let mid = Columns::for_width(px(760.));
        assert!(mid.call && !mid.format && mid.speed);
        let narrow = Columns::for_width(px(480.));
        assert!(!narrow.call && !narrow.format && !narrow.speed && narrow.status < mid.status);
    }
}
