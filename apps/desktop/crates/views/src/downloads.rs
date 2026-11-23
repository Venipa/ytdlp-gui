//! The Downloads page: intake strip, result card, ledger list, filter chips and detail panel.

use std::ops::Range;
use std::rc::Rc;

use domain::{
    AudioFormat, Chip as StatusChip, DownloadId, DownloadStatus, Filter, MediaKind, Preset,
    ProbeResult, Term, VideoQuality, split_urls,
};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, Hsla, SharedString, UniformListScrollHandle,
    Window, div, px, uniform_list,
};
use i18n::t;
use ui::{
    ActiveTheme as _, Button, CallNumber, Chip, Input, MONO, Notice, ProgressRule, Rule, Skeleton,
    Text, Thumbnail, TokenInput, Vacancy, tabular,
};

use crate::format;
use crate::sample::{Data, Row};

/// What the result card under the intake shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Card {
    None,
    Probing,
    Result,
    Batch,
    Playlist,
    /// A probe failure: the Fluent key of the message.
    Failed(&'static str),
}

/// The Downloads page.
pub struct Downloads {
    data: Rc<Data>,
    intake: Entity<Input>,
    search: Entity<Input>,
    chip: StatusChip,
    selected: Option<DownloadId>,
    card: Card,
    preset: Preset,
    formats_open: bool,
    empty: bool,
    scroll: UniformListScrollHandle,
}

impl Downloads {
    /// Builds the page over `data`.
    pub fn new(data: Rc<Data>, cx: &mut Context<Self>) -> Self {
        let intake = cx.new(|cx| Input::new(t!("dl-intake-hint"), cx).name("dl-intake-name"));
        let search = cx.new(|cx| {
            Input::new(t!("dl-search-hint"), cx)
                .name("dl-search-name")
                .clearable()
        });
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        Self {
            data,
            intake,
            search,
            chip: StatusChip::All,
            selected: None,
            card: Card::None,
            preset: Preset::DEFAULT,
            formats_open: false,
            empty: false,
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// Puts the page into a named demo state for screenshots.
    pub fn demo(&mut self, scene: &str, cx: &mut Context<Self>) {
        let url = |text: &str, this: &mut Self, cx: &mut Context<Self>| {
            this.intake
                .update(cx, |input, cx| input.set_text(text.to_owned(), cx))
        };
        match scene {
            "card" => {
                url("https://www.youtube.com/watch?v=dQw4w9WgXcQ", self, cx);
                self.card = Card::Result;
            }
            "probing" => {
                url("https://www.youtube.com/watch?v=dQw4w9WgXcQ", self, cx);
                self.card = Card::Probing;
            }
            "batch" => {
                url(
                    "https://youtu.be/aaaaaaaaaaa https://youtu.be/bbbbbbbbbbb",
                    self,
                    cx,
                );
                self.card = Card::Batch;
            }
            "playlist" => {
                url("https://www.youtube.com/playlist?list=PL123", self, cx);
                self.card = Card::Playlist;
            }
            "error" => {
                url("https://www.youtube.com/watch?v=private1", self, cx);
                self.card = Card::Failed("dl-err-login");
            }
            "formats" => {
                self.card = Card::Result;
                self.formats_open = true;
            }
            "detail" => self.selected = Some(DownloadId(131)),
            "failed-detail" => self.selected = Some(DownloadId(127)),
            "filtered" => {
                self.search
                    .update(cx, |input, cx| input.set_text("type:audio size:>50mb", cx));
            }
            "empty" => self.empty = true,
            _ => {}
        }
        cx.notify();
    }

    fn rows(&self, cx: &App) -> Vec<Row> {
        if self.empty {
            return Vec::new();
        }
        let filter = Filter::parse(self.search.read(cx).text());
        self.data
            .rows
            .iter()
            .filter(|row| self.chip.matches(row.summary.status) && filter.matches(&row.summary))
            .cloned()
            .collect()
    }

    fn count(&self, chip: StatusChip) -> usize {
        match self.empty {
            true => 0,
            false => self
                .data
                .rows
                .iter()
                .filter(|row| chip.matches(row.summary.status))
                .count(),
        }
    }

    /// Reads the intake and opens the card it calls for.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let text = self.intake.read(cx).text().to_owned();
        let split = split_urls(&text);
        self.card = match (split.urls.len(), split.invalid.is_empty()) {
            (0, true) => Card::None,
            (0, false) => Card::Failed("dl-err-network"),
            (1, true) if text.contains("list=") => Card::Playlist,
            (1, true) => Card::Result,
            _ => Card::Batch,
        };
        cx.notify();
    }

    fn intake_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(40.))
            .child(div().flex_1().child(self.intake.clone()))
            .child(
                Button::new("intake-download")
                    .primary()
                    .label(t!("dl-download"))
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .text_size(theme.text(Text::Body))
    }

    fn preset_chip(&self, preset: Preset, label: SharedString, cx: &mut Context<Self>) -> Chip {
        Chip::new(SharedString::from(format!("preset-{preset}")), label)
            .selected(self.preset == preset)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.preset = preset;
                cx.notify();
            }))
    }

    fn card_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = *cx.theme();
        let shell = || {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.secondary)
        };
        let muted = theme.muted_foreground;
        let probe: &ProbeResult = &self.data.probe;
        Some(match self.card {
            Card::None => return None,
            Card::Probing => shell()
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(Skeleton::new().w(px(320.)).h(px(180.)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(Skeleton::new().w(px(260.)).h(px(18.)))
                                .child(Skeleton::new().w(px(140.)).h(px(12.)))
                                .child(div().text_color(muted).child(t!("dl-probing"))),
                        ),
                )
                .into_any_element(),
            Card::Failed(key) => shell()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    Notice::new(t!("dl-probing"), i18n::lookup(key, None))
                        .failed()
                        .spoken("probe-failed"),
                )
                .child(
                    Button::new("open-settings")
                        .outline()
                        .label(t!("dl-open-settings")),
                )
                .into_any_element(),
            Card::Result => {
                let video = [
                    VideoQuality::Best,
                    VideoQuality::P2160,
                    VideoQuality::P1440,
                    VideoQuality::P1080,
                    VideoQuality::P720,
                    VideoQuality::P480,
                ];
                let audio = [
                    AudioFormat::Best,
                    AudioFormat::Mp3,
                    AudioFormat::M4a,
                    AudioFormat::Opus,
                ];
                let name = |text: &str| -> AnyElement {
                    div()
                        .w(px(48.))
                        .text_color(muted)
                        .child(text.to_owned())
                        .into_any_element()
                };
                shell()
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(Thumbnail::new(None, px(320.)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(theme.text(Text::Large))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(probe.title.clone().unwrap_or_default()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .justify_between()
                                            .text_color(muted)
                                            .child(probe.uploader.clone().unwrap_or_default())
                                            .child(div().font_features(tabular()).child(
                                                format::duration(probe.duration_s.unwrap_or(0.)),
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .mt_2()
                                            .child(name(&t!("dl-card-video")))
                                            .children(video.map(|q| {
                                                let label = match q {
                                                    VideoQuality::Best => t!("dl-preset-best"),
                                                    q => SharedString::from(q.as_str()),
                                                };
                                                self.preset_chip(Preset::Video(q), label, cx)
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(name(&t!("dl-card-audio")))
                                            .children(audio.map(|a| {
                                                let label = match a {
                                                    AudioFormat::Best => t!("dl-preset-best"),
                                                    a => SharedString::from(a.as_str()),
                                                };
                                                self.preset_chip(Preset::Audio(a), label, cx)
                                            }))
                                            .child(div().flex_1())
                                            .child(
                                                Button::new("choose-formats")
                                                    .ghost()
                                                    .label(t!("dl-choose-formats"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.formats_open = true;
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                Button::new("card-cancel")
                                                    .ghost()
                                                    .label(t!("dl-cancel"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.card = Card::None;
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            ),
                    )
                    .into_any_element()
            }
            Card::Batch => {
                let line = |url: &str, note: SharedString, dim: bool| {
                    div()
                        .flex()
                        .gap_3()
                        .h(px(32.))
                        .items_center()
                        .child(
                            div()
                                .w(px(120.))
                                .font_family(MONO)
                                .text_size(theme.text(Text::Small))
                                .child(url.to_owned()),
                        )
                        .child(
                            div()
                                .text_color(if dim { muted } else { theme.foreground })
                                .child(note),
                        )
                };
                shell()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("dl-batch-title", count = 4)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .items_center()
                                    .child(div().text_color(muted).child(t!("dl-batch-preset")))
                                    .child(self.preset_chip(
                                        Preset::Video(VideoQuality::P1080),
                                        "1080".into(),
                                        cx,
                                    ))
                                    .child(
                                        Button::new("batch-download")
                                            .primary()
                                            .label(t!("dl-download-count", count = 3)),
                                    ),
                            ),
                    )
                    .child(line("youtu.be/aaaaaaaaaaa", "Intro to sets".into(), false))
                    .child(line("youtu.be/bbbbbbbbbbb", t!("dl-probing"), true))
                    .child(line("example.com/x", t!("dl-already"), true))
                    .child(line("not a link", t!("dl-batch-skipped"), true))
                    .into_any_element()
            }
            Card::Playlist => {
                let entries = self.data.playlist.entries.clone().unwrap_or_default();
                shell()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(div().font_weight(FontWeight::MEDIUM).child(t!(
                                "dl-playlist-title",
                                count = 48,
                                title = "Lecture series 2024"
                            )))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("sel-all").ghost().label(t!("dl-select-all")),
                                    )
                                    .child(
                                        Button::new("sel-none").ghost().label(t!("dl-select-none")),
                                    ),
                            ),
                    )
                    .children(entries.into_iter().map(|entry| {
                        let have = entry.index == 3;
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .h(px(32.))
                            .child(
                                div()
                                    .w(px(14.))
                                    .h(px(14.))
                                    .rounded(theme.radius)
                                    .border_1()
                                    .border_color(theme.border)
                                    .when(!have, |this| this.bg(theme.table_active_border)),
                            )
                            .child(
                                div()
                                    .w(px(36.))
                                    .font_family(MONO)
                                    .text_color(muted)
                                    .child(format!("{:03}", entry.index)),
                            )
                            .child(div().flex_1().child(entry.title.unwrap_or_default()))
                            .child(
                                div()
                                    .font_features(tabular())
                                    .child(format::duration(entry.duration_s.unwrap_or(0.))),
                            )
                            .when(have, |this| {
                                this.child(div().text_color(muted).child(t!("dl-already")))
                            })
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(div().text_color(muted).child(t!(
                                "dl-playlist-apply",
                                preset = "1080",
                                count = 47
                            )))
                            .child(
                                Button::new("playlist-download")
                                    .primary()
                                    .label(t!("dl-download-count", count = 47)),
                            ),
                    )
                    .into_any_element()
            }
        })
    }

    fn chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let chips = [
            (StatusChip::All, "dl-chip-all"),
            (StatusChip::Active, "dl-chip-active"),
            (StatusChip::Done, "dl-chip-done"),
            (StatusChip::Failed, "dl-chip-failed"),
        ];
        let tokens = Filter::parse(self.search.read(cx).text())
            .terms
            .iter()
            .filter_map(|term| match term {
                Term::Invalid(text) => Some((SharedString::from(text.clone()), false)),
                Term::Type(_) | Term::Status(_) | Term::Size(..) | Term::Site(_) => None,
                Term::Text(_) => None,
            })
            .chain(
                self.search
                    .read(cx)
                    .text()
                    .split_whitespace()
                    .filter(|word| {
                        ["type:", "status:", "size:", "site:"]
                            .iter()
                            .any(|k| word.starts_with(k))
                    })
                    .filter(|word| {
                        !matches!(Filter::parse(word).terms.first(), Some(Term::Invalid(_)))
                    })
                    .map(|word| (SharedString::from(word.to_owned()), true)),
            )
            .collect();
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(36.))
            .text_size(theme.text(Text::Label))
            .child(div().flex().gap_2().children(chips.map(|(chip, key)| {
                Chip::new(key, i18n::lookup(key, None))
                    .count(self.count(chip))
                    .selected(self.chip == chip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.chip = chip;
                        cx.notify();
                    }))
            })))
            .child(TokenInput::new(&self.search, tokens))
    }

    fn row(&self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let s = &row.summary;
        let id = s.id;
        let selected = self.selected == Some(id);
        let (status, tone) = status_text(row, &theme);
        let rule = match s.status {
            DownloadStatus::Downloading | DownloadStatus::Processing => {
                Rule::Fill(s.progress as f32)
            }
            DownloadStatus::Completed => Rule::Solid,
            DownloadStatus::Failed => Rule::Failed,
            _ => Rule::Dotted,
        };
        let failed = s.status == DownloadStatus::Failed;
        let active = matches!(
            s.status,
            DownloadStatus::Downloading | DownloadStatus::Processing
        );
        let size = s
            .file_size
            .or(s.bytes_total)
            .map(format::bytes)
            .unwrap_or_else(|| "-".into());
        let third = match (row.speed, s.status) {
            (Some(speed), _) => format!(
                "{}  {}",
                format::speed(speed),
                format::duration(row.eta.unwrap_or(0.))
            ),
            (None, DownloadStatus::Completed) => "Yesterday".to_owned(),
            _ => String::new(),
        };
        let second: SharedString = match (failed, &s.error_code) {
            (true, Some(code)) => i18n::lookup(err_key(code), None),
            _ => SharedString::from(s.domain.clone()),
        };
        div()
            .id(("row", id.0 as usize))
            .flex()
            .flex_col()
            .flex_none()
            .h(px(54.))
            .w_full()
            .cursor_pointer()
            .when(selected, |this| this.bg(theme.table_active))
            .hover(|style| style.bg(theme.table_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(id);
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap_3()
                    .px(theme.metrics.pad)
                    .child(CallNumber::new(id.0).selected(selected))
                    .child(Thumbnail::new(None, px(80.)))
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
                                    .text_color(if failed {
                                        theme.danger
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .child(second),
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
                            .w(px(96.))
                            .font_features(tabular())
                            .text_color(theme.muted_foreground)
                            .child(third),
                    )
                    .child(
                        div()
                            .w(px(180.))
                            .flex()
                            .justify_end()
                            .gap_2()
                            .text_color(tone)
                            .child(status)
                            .when(active, |this| {
                                this.child(
                                    div()
                                        .w(px(40.))
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
                                        .label(t!("dl-try-again")),
                                )
                            }),
                    ),
            )
            .child(ProgressRule::new(rule))
            .into_any_element()
    }
}

/// The status words of a row and the colour they take.
fn status_text(row: &Row, theme: &ui::Theme) -> (SharedString, Hsla) {
    let s = &row.summary;
    let pencil = theme.muted_foreground;
    match s.status {
        DownloadStatus::Queued => (t!("dl-status-queued"), pencil),
        DownloadStatus::Probing => (t!("dl-status-probing"), pencil),
        DownloadStatus::Downloading => (t!("dl-status-downloading"), theme.progress_bar),
        DownloadStatus::Processing => match s.kind {
            MediaKind::Audio => (
                t!("dl-status-converting", format = "mp3"),
                theme.progress_bar,
            ),
            MediaKind::Video => (t!("dl-status-merging"), theme.progress_bar),
        },
        DownloadStatus::Paused => (t!("dl-status-paused"), pencil),
        DownloadStatus::Completed if s.file_missing => (t!("dl-file-missing"), theme.danger),
        DownloadStatus::Completed => (t!("dl-status-done"), pencil),
        DownloadStatus::Failed => (t!("dl-status-failed"), theme.danger),
        DownloadStatus::Cancelled => (t!("dl-status-cancelled"), pencil),
    }
}

/// The Fluent key of an error code's one-line cause.
pub fn err_key(code: &str) -> &'static str {
    match code {
        "login" | "age_restricted" | "cookies_locked" => "dl-err-login",
        "private" => "dl-err-private",
        "geo" => "dl-err-geo",
        "network" | "ssl" => "dl-err-network",
        "rate_limited" => "dl-err-rate",
        "disk_full" => "dl-err-disk",
        "permission" => "dl-err-permission",
        _ => "dl-err-unknown",
    }
}

impl Render for Downloads {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let rows = Rc::new(self.rows(cx));
        let filter_text = self.search.read(cx).text().to_owned();
        let running = self
            .data
            .rows
            .iter()
            .filter(|r| r.summary.status == DownloadStatus::Downloading)
            .count();
        let paused = self
            .data
            .rows
            .iter()
            .filter(|r| r.summary.status == DownloadStatus::Paused)
            .count();
        let list = match (rows.is_empty(), self.empty) {
            (true, true) => Vacancy::new(t!("dl-empty"))
                .icon("icons/download.svg")
                .detail(t!("dl-empty-detail"))
                .spoken("downloads-empty")
                .size_full()
                .into_any_element(),
            (true, false) => Vacancy::new(t!("dl-filtered-empty", filter = filter_text.as_str()))
                .icon("icons/search.svg")
                .action(
                    Button::new("clear-filter")
                        .outline()
                        .label(t!("dl-clear-filter"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.search.update(cx, |input, cx| input.set_text("", cx));
                            this.chip = StatusChip::All;
                            cx.notify();
                        })),
                )
                .spoken("downloads-filtered")
                .size_full()
                .into_any_element(),
            _ => {
                let items = rows.clone();
                uniform_list(
                    "downloads-rows",
                    rows.len(),
                    cx.processor(move |this, range: Range<usize>, _, cx| {
                        range.map(|at| this.row(&items[at], cx)).collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full()
                .into_any_element()
            }
        };
        let selected = self
            .selected
            .and_then(|id| self.data.rows.iter().find(|r| r.summary.id == id).cloned());
        let card = self.card_view(cx);

        div()
            .size_full()
            .flex()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .p(theme.metrics.pad)
                    .child(self.intake_strip(cx))
                    .children(card)
                    .child(self.chips(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .border_t_1()
                            .border_color(theme.border)
                            .child(list),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .justify_between()
                            .items_center()
                            .h(px(28.))
                            .text_size(theme.text(Text::Small))
                            .text_color(theme.muted_foreground)
                            .child(t!("dl-status-bar", running = running, paused = paused))
                            .child(t!("dl-free-space", size = "412 GB", drive = "D:")),
                    ),
            )
            .children(selected.map(|row| crate::detail::panel(&row, &self.data, cx)))
            .when(self.formats_open, |this| {
                this.child(crate::formats::modal(&self.data.probe, cx))
            })
    }
}

impl Downloads {
    /// Closes the detail panel.
    pub(crate) fn deselect(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        cx.notify();
    }

    /// Closes the formats modal.
    pub(crate) fn close_formats(&mut self, cx: &mut Context<Self>) {
        self.formats_open = false;
        cx.notify();
    }
}
