//! The Logs page over `state::Logs`: level chips, a job picker, copy all.

use std::collections::HashSet;
use std::rc::Rc;

use domain::DownloadId;
use gpui::prelude::*;
use gpui::{
    AnyElement, ClipboardItem, Context, Entity, FollowMode, ListAlignment, ListState, Subscription,
    Window, accesskit::Role, div, list, px,
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
    /// Rows of different heights: a line opens to its whole message.
    lines: ListState,
    bar: gpui::Entity<Scrollbar>,
    /// The filter and count the list was last laid out for, so new lines are appended.
    laid_out: (u8, Option<DownloadId>, String, usize),
    search: Entity<Input>,
    /// Whether new lines scroll into view; scrolling up pauses it until the end is reached again.
    follow: bool,
    /// The open lines, by time and message.
    open: HashSet<(i64, String)>,
    _subs: Vec<Subscription>,
}

impl LogsView {
    /// Builds the page over the registered state.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let lines = ListState::new(0, ListAlignment::Top, px(200.));
        lines.set_follow_mode(FollowMode::Tail);
        let bar = cx.new(|cx| Scrollbar::list(lines.clone()).watching(cx.entity_id()));
        let search = cx.new(|cx| {
            Input::new("dl-logs-search-hint", cx)
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
            lines,
            laid_out: (0, None, String::new(), 0),
            search,
            follow: true,
            open: HashSet::new(),
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
        let filter = (
            self.least,
            self.job,
            self.search.read(cx).text().trim().to_owned(),
        );
        let (least, job, needle, count) = &self.laid_out;
        let count = *count;
        if (*least, *job, needle) != (filter.0, filter.1, &filter.2) || entries.len() < count {
            self.lines.reset(entries.len());
            if self.follow {
                self.lines.set_follow_mode(FollowMode::Tail);
            }
        } else if entries.len() > count {
            self.lines.splice(count..count, entries.len() - count);
        }
        self.laid_out = (filter.0, filter.1, filter.2, entries.len());
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
                let view = cx.entity().downgrade();
                let open = self.open.clone();
                Scroller::listing("logs-scroll", &self.bar)
                    .child(
                        list(self.lines.clone(), move |at, window, cx| {
                            let Some(entry) = entries.get(at) else {
                                return div().into_any_element();
                            };
                            let key = (entry.at, entry.message.clone());
                            let is_open = open.contains(&key);
                            let view = view.clone();
                            row(at, entry, is_open, window, cx, move |cx| {
                                view.update(cx, |this, cx| {
                                    if !this.open.remove(&key) {
                                        this.open.insert(key.clone());
                                    }
                                    cx.notify();
                                })
                                .ok();
                            })
                        })
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
                // a narrow page wraps the search and buttons onto lines of their own rather
                // than running them off the edge
                div()
                    .flex()
                    .flex_none()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(theme.metrics.pad)
                    .px(theme.metrics.gutter)
                    .py(px(12.))
                    .child(div().flex().flex_none().gap(px(6.)).children(chips.map(
                        |(key, least)| {
                            Chip::new(key, i18n::lookup(key, None))
                                .selected(self.least == least)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.least = least;
                                    cx.notify();
                                }))
                        },
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .items_center()
                            .child(div().w(px(220.)).flex_none().child(self.search.clone()))
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
                                        this.lines.set_follow_mode(match this.follow {
                                            true => FollowMode::Tail,
                                            false => FollowMode::Normal,
                                        });
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
                    .child(body),
            )
            .children(picker)
    }
}

/// One log line: time, level, job and the first line of the message cut to the width; open, the
/// whole message wraps below, with a button that copies it.
fn row(
    at: usize,
    entry: &LogEntry,
    open: bool,
    window: &Window,
    cx: &gpui::App,
    toggle: impl Fn(&mut gpui::App) + 'static,
) -> AnyElement {
    let theme = *cx.theme();
    // a narrow window keeps the level word and drops its dot
    let narrow = window.viewport_size().width < px(900.);
    let tone = match rank(&entry.level) {
        3 => theme.danger,
        2 => theme.progress_bar,
        _ => theme.muted_foreground,
    };
    let first = entry.message.lines().next().unwrap_or_default().to_owned();
    let more = entry.message.lines().count().saturating_sub(1);
    let copied = format!(
        "{} {} {}",
        format::clock(entry.at),
        entry.level,
        entry.message
    );
    let message = match open {
        true => div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(6.))
            .child(div().w_full().child(entry.message.clone()))
            .child(
                Button::new(("log-copy", at))
                    .small()
                    .ghost()
                    .icon("icons/copy.svg")
                    .label(t!("dl-logs-copy-line"))
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                        toast(cx, Kind::Success, t!("dl-logs-copied"), None);
                    }),
            )
            .into_any_element(),
        false => div()
            .flex_1()
            .min_w_0()
            .flex()
            .gap(px(8.))
            .child(div().min_w_0().truncate().child(first))
            .when(more > 0, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(t!("dl-logs-more-lines", count = more)),
                )
            })
            .into_any_element(),
    };
    div()
        .id(("log-line", at))
        .role(Role::Button)
        .aria_label(match open {
            true => t!("dl-logs-line-close"),
            false => t!("dl-logs-line-open"),
        })
        .w_full()
        .flex()
        .items_start()
        .gap(px(12.))
        .min_h(px(24.))
        .py(px(4.))
        .px(theme.metrics.gutter)
        .cursor_pointer()
        // warnings and errors carry a faint wash of their tone
        .when(rank(&entry.level) >= 2, |this| this.bg(tone.opacity(0.08)))
        .when(open && rank(&entry.level) < 2, |this| {
            this.bg(theme.secondary_hover)
        })
        .hover(|style| style.bg(theme.table_hover))
        .on_click(move |_, _, cx| toggle(cx))
        .font_family(MONO)
        .text_size(theme.text(Text::Small))
        .child(
            div()
                .w(px(64.))
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(format::clock(entry.at)),
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
                    this.child(div().size(px(6.)).rounded_full().bg(tone))
                })
                .child(entry.level.clone()),
        )
        .child(
            div()
                .w(px(40.))
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(entry.job.map(|j| format!("{:04}", j.0)).unwrap_or_default()),
        )
        .child(message)
        .into_any_element()
}
