//! The Logs page over `state::Logs`: level chips, a job picker, copy all.

use std::ops::Range;
use std::rc::Rc;

use domain::DownloadId;
use gpui::prelude::*;
use gpui::{
    ClipboardItem, Context, ScrollStrategy, Subscription, UniformListScrollHandle, Window,
    div, px, uniform_list,
};
use i18n::t;
use state::{AppState, LogEntry};
use ui::{
    ActiveTheme as _, Button, Chip, MONO, Menu, MenuItem, Popup, Text, Vacancy,
};

use crate::format;
use crate::toasts::{Kind, toast};

/// The lowest rank a chip shows: debug 0, info 1, warn 2, error 3.
fn rank(level: &str) -> u8 {
    match level {
        "error" => 3,
        "warn" | "warning" => 2,
        "info" => 1,
        _ => 0,
    }
}

/// The Logs page.
pub struct LogsView {
    app: AppState,
    /// The chip: the least rank shown.
    least: u8,
    job: Option<DownloadId>,
    picker: Option<gpui::Point<gpui::Pixels>>,
    scroll: UniformListScrollHandle,
    seen: usize,
    _sub: Subscription,
}

impl LogsView {
    /// Builds the page over the registered state.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let sub = cx.observe(&app.logs, |_, _, cx| cx.notify());
        Self {
            app,
            least: 0,
            job: None,
            picker: None,
            scroll: UniformListScrollHandle::new(),
            seen: 0,
            _sub: sub,
        }
    }

    fn shown(&self, cx: &gpui::App) -> Vec<LogEntry> {
        self.app
            .logs
            .read(cx)
            .entries()
            .filter(|e| rank(&e.level) >= self.least && (self.job.is_none() || e.job == self.job))
            .cloned()
            .collect()
    }

    fn copy_all(&self, cx: &mut Context<Self>) {
        let text = self
            .shown(cx)
            .iter()
            .map(|e| {
                format!(
                    "{} {:<5} {} {}",
                    format::clock(e.at),
                    e.level,
                    e.job.map(|j| format!("{:04}", j.0)).unwrap_or_default(),
                    e.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        toast(cx, Kind::Success, t!("dl-logs-copied"), None);
    }

    /// The jobs that have log lines, for the picker.
    fn jobs(&self, cx: &gpui::App) -> Vec<(DownloadId, String)> {
        let mut ids: Vec<DownloadId> = self
            .app
            .logs
            .read(cx)
            .entries()
            .filter_map(|e| e.job)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let downloads = self.app.downloads.read(cx);
        ids.into_iter()
            .rev()
            .take(50)
            .map(|id| {
                let title = downloads
                    .get(id)
                    .and_then(|s| s.title.clone())
                    .unwrap_or_default();
                (id, format!("{:04}  {title}", id.0))
            })
            .collect()
    }

    fn picker_menu(&self, cx: &mut Context<Self>) -> Option<Popup> {
        let at = self.picker?;
        let this = cx.entity();
        let pick = |job: Option<DownloadId>| {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                this.update(cx, |this, cx| {
                    this.job = job;
                    this.picker = None;
                    cx.notify();
                })
            }
        };
        let mut items = vec![
            MenuItem::new("job-all", t!("dl-logs-all-jobs"))
                .checked(self.job.is_none())
                .on_click(pick(None)),
        ];
        items.extend(self.jobs(cx).into_iter().map(|(id, label)| {
            MenuItem::new(("job", id.0 as usize), label)
                .checked(self.job == Some(id))
                .on_click(pick(Some(id)))
        }));
        Some(
            Popup::new(at, Menu::new("logs-jobs").items(items)).on_close(cx.listener(
                |this, _, _, cx| {
                    this.picker = None;
                    cx.notify();
                },
            )),
        )
    }
}

impl Render for LogsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let pencil = theme.muted_foreground;
        let entries = Rc::new(self.shown(cx));
        if entries.len() != self.seen {
            self.seen = entries.len();
            if !entries.is_empty() {
                self.scroll
                    .scroll_to_item(entries.len() - 1, ScrollStrategy::Bottom);
            }
        }
        let job_label = match self.job {
            Some(id) => format!("{:04}", id.0),
            None => t!("dl-logs-all-jobs").to_string(),
        };
        let chips = [
            ("dl-logs-all", 0u8),
            ("dl-logs-info", 1),
            ("dl-logs-warn", 2),
            ("dl-logs-error", 3),
        ];
        let body = match entries.is_empty() {
            true => Vacancy::new(t!("dl-logs-empty"))
                .spoken("logs-empty")
                .size_full()
                .into_any_element(),
            false => {
                let items = entries.clone();
                uniform_list(
                    "log-lines",
                    entries.len(),
                    cx.processor(move |_, range: Range<usize>, _, cx| {
                        let theme = *cx.theme();
                        range
                            .filter_map(|at| items.get(at))
                            .map(|l| {
                                let tone = match rank(&l.level) {
                                    3 => theme.danger,
                                    2 => theme.progress_bar,
                                    _ => theme.muted_foreground,
                                };
                                div()
                                    .flex()
                                    .gap_3()
                                    .h(px(24.))
                                    .items_center()
                                    .font_family(MONO)
                                    .text_size(theme.text(Text::Small))
                                    .child(
                                        div()
                                            .w(px(64.))
                                            .flex_none()
                                            .text_color(theme.muted_foreground)
                                            .child(format::clock(l.at)),
                                    )
                                    .child(
                                        div().w(px(44.)).flex_none().text_color(tone).child(
                                            l.level.clone(),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .w(px(40.))
                                            .flex_none()
                                            .text_color(theme.muted_foreground)
                                            .child(
                                                l.job
                                                    .map(|j| format!("{:04}", j.0))
                                                    .unwrap_or_default(),
                                            ),
                                    )
                                    .child(div().flex_1().truncate().child(l.message.clone()))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full()
                .into_any_element()
            }
        };
        let picker = self.picker_menu(cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_2()
            .p(theme.metrics.pad)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().flex().gap_2().children(chips.map(|(key, least)| {
                        Chip::new(key, i18n::lookup(key, None))
                            .selected(self.least == least)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.least = least;
                                cx.notify();
                            }))
                    })))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(div().text_color(pencil).child(t!("dl-logs-job")))
                            .child(
                                Button::new("logs-job")
                                    .outline()
                                    .small()
                                    .label(job_label)
                                    .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                                        this.picker = Some(event.position());
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("logs-copy")
                                    .ghost()
                                    .label(t!("dl-logs-copy"))
                                    .on_click(cx.listener(|this, _, _, cx| this.copy_all(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(body),
            )
            .children(picker)
    }
}
