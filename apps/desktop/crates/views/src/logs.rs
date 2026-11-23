//! The Logs page over `state::Logs`: level chips, a job picker, copy all.

use std::ops::Range;
use std::rc::Rc;

use domain::DownloadId;
use gpui::prelude::*;
use gpui::{
    ClipboardItem, Context, Entity, ScrollStrategy, ScrollWheelEvent, Subscription,
    UniformListScrollHandle, Window, div, px, uniform_list,
};
use i18n::t;
use state::{AppState, LogEntry};
use ui::{
    ActiveTheme as _, Button, Chip, Input, MONO, Menu, MenuItem, Popup, Scrollbar, Scroller, Text,
    Vacancy,
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
    bar: gpui::Entity<Scrollbar>,
    seen: usize,
    search: Entity<Input>,
    /// Whether new lines scroll into view; a wheel turn up pauses it until the end is reached again.
    follow: bool,
    resume_at_end: bool,
    _subs: Vec<Subscription>,
}

impl LogsView {
    /// Builds the page over the registered state.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let scroll = UniformListScrollHandle::new();
        let bar = cx.new(|cx| {
            Scrollbar::new(scroll.0.borrow().base_handle.clone()).watching(cx.entity_id())
        });
        let search = cx.new(|cx| {
            Input::new(t!("dl-logs-search-hint"), cx)
                .name("dl-logs-search-name")
                .icon("icons/search.svg")
                .clearable()
        });
        let subs = vec![
            cx.observe(&app.logs, |_, _, cx| cx.notify()),
            cx.observe(&search, |_, _, cx| cx.notify()),
        ];
        Self {
            app,
            least: 0,
            job: None,
            picker: None,
            bar,
            scroll,
            seen: 0,
            search,
            follow: true,
            resume_at_end: false,
            _subs: subs,
        }
    }

    fn shown(&self, cx: &gpui::App) -> Vec<LogEntry> {
        let needle = self.search.read(cx).text().trim().to_lowercase();
        self.app
            .logs
            .read(cx)
            .entries()
            .filter(|e| {
                rank(&e.level) >= self.least
                    && (self.job.is_none() || e.job == self.job)
                    && (needle.is_empty()
                        || format!("{} {:04} {}", e.level, e.job.map_or(0, |j| j.0), e.message)
                            .to_lowercase()
                            .contains(&needle))
            })
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
        let entries = Rc::new(self.shown(cx));
        if !self.follow && self.resume_at_end && self.scroll.is_scrolled_to_end() == Some(true) {
            self.follow = true;
            self.resume_at_end = false;
        }
        if entries.len() != self.seen {
            self.seen = entries.len();
            if self.follow && !entries.is_empty() {
                self.scroll
                    .scroll_to_item(entries.len() - 1, ScrollStrategy::Bottom);
            }
        }
        let job_label = match self.job {
            Some(id) => format!("{} {:04}", t!("dl-logs-job"), id.0),
            None => t!("dl-logs-all-jobs").to_string(),
        };
        let chips = [
            ("dl-logs-all", 0u8),
            ("dl-logs-info", 1),
            ("dl-logs-warn", 2),
            ("dl-logs-error", 3),
        ];
        let body = match entries.is_empty() {
            true => Vacancy::new(match self.search.read(cx).text().is_empty() {
                true => t!("dl-logs-empty"),
                false => t!("dl-logs-no-match"),
            })
            .spoken("logs-empty")
            .size_full()
            .into_any_element(),
            false => {
                let items = entries.clone();
                Scroller::listing("logs-scroll", &self.bar)
                    .child(
                        uniform_list(
                            "log-lines",
                            entries.len(),
                            cx.processor(move |_, range: Range<usize>, window, cx| {
                                let theme = *cx.theme();
                                // a narrow window keeps the level word and drops its dot
                                let narrow = window.viewport_size().width < px(900.);
                                range
                                    .filter_map(|at| items.get(at).map(|l| (at, l)))
                                    .map(|(at, l)| {
                                        let tone = match rank(&l.level) {
                                            3 => theme.danger,
                                            2 => theme.progress_bar,
                                            _ => theme.muted_foreground,
                                        };
                                        let line = format!(
                                            "{} {} {}",
                                            format::clock(l.at),
                                            l.level,
                                            l.message
                                        );
                                        div()
                                            .id(("log-line", at))
                                            .flex()
                                            .gap(px(12.))
                                            .h(px(24.))
                                            .px(theme.metrics.gutter)
                                            .items_center()
                                            .cursor_pointer()
                                            // warnings and errors carry a faint wash of their tone
                                            .when(rank(&l.level) >= 2, |this| {
                                                this.bg(tone.opacity(0.08))
                                            })
                                            .hover(|style| style.bg(theme.table_hover))
                                            .on_click(move |_, _, cx| {
                                                cx.write_to_clipboard(
                                                    gpui::ClipboardItem::new_string(line.clone()),
                                                )
                                            })
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
                                                div()
                                                    .w(px(if narrow { 40. } else { 56. }))
                                                    .flex_none()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.))
                                                    .text_color(tone)
                                                    .when(!narrow, |this| {
                                                        this.child(
                                                            div()
                                                                .size(px(6.))
                                                                .rounded_full()
                                                                .bg(tone),
                                                        )
                                                    })
                                                    .child(l.level.clone()),
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
                                            .child(
                                                div().flex_1().truncate().child(l.message.clone()),
                                            )
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .size_full(),
                    )
                    .into_any_element()
            }
        };
        let picker = self.picker_menu(cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .gap(theme.metrics.pad)
                    .px(theme.metrics.gutter)
                    .py(px(12.))
                    .child(div().flex().gap(px(6.)).children(chips.map(|(key, least)| {
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
                            .gap(px(8.))
                            .items_center()
                            .child(div().w(px(220.)).child(self.search.clone()))
                            .child(
                                Button::new("logs-job")
                                    .outline()
                                    .icon("icons/chevrons-up-down.svg")
                                    .label(job_label)
                                    .on_click(cx.listener(
                                        |this, event: &gpui::ClickEvent, _, cx| {
                                            this.picker = Some(event.position());
                                            cx.notify();
                                        },
                                    )),
                            )
                            .child(
                                Button::new("logs-follow")
                                    .ghost()
                                    .icon("icons/arrow-up-down.svg")
                                    .label(t!("dl-logs-follow"))
                                    .toggled(self.follow)
                                    .selected(self.follow)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.follow = !this.follow;
                                        this.resume_at_end = false;
                                        if this.follow {
                                            this.seen = 0;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("logs-copy")
                                    .ghost()
                                    .icon("icons/copy.svg")
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
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        if event.delta.pixel_delta(px(20.)).y > px(0.) && this.follow {
                            this.follow = false;
                            this.resume_at_end = true;
                            cx.notify();
                        }
                    }))
                    .child(body),
            )
            .children(picker)
    }
}
