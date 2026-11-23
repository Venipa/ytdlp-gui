//! The card under the intake: reading, result with the remembered preset, batch, playlist, failure.

use std::sync::Arc;

use domain::{AudioFormat, Preset, ProbeResult, VideoQuality};
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, FontWeight, SharedString, div, px, svg,
    uniform_list,
};
use i18n::t;
use state::ProbeState;
use ui::{
    ActiveTheme as _, Button, Checkbox, Chip, MONO, ProgressRule, Rule, Skeleton, Spinner, Text,
    Thumbnail, ease_out_cubic, tabular,
};

use crate::downloads::{Card, Downloads, error_text};
use crate::format;

/// The video rows of the preset chips.
const VIDEO: [VideoQuality; 6] = [
    VideoQuality::Best,
    VideoQuality::P2160,
    VideoQuality::P1440,
    VideoQuality::P1080,
    VideoQuality::P720,
    VideoQuality::P480,
];
/// The audio rows of the preset chips.
const AUDIO: [AudioFormat; 4] = [
    AudioFormat::Best,
    AudioFormat::Mp3,
    AudioFormat::M4a,
    AudioFormat::Opus,
];
/// The width of the result card's thumbnail.
const CARD_THUMB: f32 = 160.;
/// The width of the Video and Audio labels before the chips.
const LABEL: f32 = 52.;
/// The tallest a one-link card gets, which its unfold opens to.
const CARD_MAX: f32 = 220.;
/// A batch line: the link, what became of it, its state icon (none while reading), dimmed.
type BatchLine = (
    String,
    SharedString,
    Option<(&'static str, gpui::Hsla)>,
    bool,
);
/// Playlist rows shown before the list scrolls.
const PLAYLIST_ROWS: usize = 8;

impl Downloads {
    fn preset_chip(&self, preset: Preset, label: SharedString, cx: &mut Context<Self>) -> Chip {
        Chip::new(SharedString::from(format!("preset-{preset}")), label)
            .selected(self.preset == preset)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.preset = preset;
                cx.notify();
            }))
    }

    /// The two labelled chip rows, Video and Audio, each with something at its end.
    fn preset_rows(
        &self,
        video_end: impl IntoElement,
        audio_end: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let name =
            |text: SharedString| div().w(px(LABEL)).flex_none().text_color(muted).child(text);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(name(t!("dl-card-video")))
                    .children(VIDEO.map(|q| {
                        let label = match q {
                            VideoQuality::Best => t!("dl-preset-best"),
                            q => SharedString::from(q.as_str()),
                        };
                        self.preset_chip(Preset::Video(q), label, cx)
                    }))
                    .child(div().flex_1())
                    .child(video_end),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(name(t!("dl-card-audio")))
                    .children(AUDIO.map(|a| {
                        let label = match a {
                            AudioFormat::Best => t!("dl-preset-best"),
                            a => SharedString::from(a.as_str()),
                        };
                        self.preset_chip(Preset::Audio(a), label, cx)
                    }))
                    .child(div().flex_1())
                    .child(audio_end),
            )
            .into_any_element()
    }

    /// A card that could not go on: an alert icon, what happened, why, the engine's own words
    /// muted under it, and its actions at the end.
    fn problem(
        &self,
        title: SharedString,
        cause: SharedString,
        detail: Option<SharedString>,
        actions: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = *cx.theme();
        div()
            .flex()
            .items_start()
            .gap(px(12.))
            .child(
                svg()
                    .path(icons::path("icons/circle-alert.svg"))
                    .size(px(16.))
                    .mt(px(2.))
                    .flex_none()
                    .text_color(theme.danger),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(div().font_weight(FontWeight::MEDIUM).child(title))
                    .child(
                        div()
                            .text_color(theme.foreground.opacity(0.85))
                            .child(cause),
                    )
                    .children(detail.map(|detail| {
                        div()
                            .truncate()
                            .font_family(MONO)
                            .text_size(theme.text(Text::Small))
                            .text_color(theme.muted_foreground)
                            .child(detail)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .children(actions),
            )
    }

    /// The card, when there is something to show.
    pub(crate) fn card_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = *cx.theme();
        let shell = || {
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .p(px(12.))
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.secondary)
        };
        let muted = theme.muted_foreground;
        let dismiss = |cx: &mut Context<Self>| {
            Button::new("card-cancel")
                .ghost()
                .small()
                .icon("icons/x.svg")
                .tooltip("dl-cancel")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.intake.update(cx, |input, cx| input.set_text("", cx));
                    this.clear_card(cx);
                }))
        };
        let compact = matches!(
            self.card(cx),
            Card::Probing | Card::Result(_) | Card::Failed(_) | Card::NotLink
        );
        let thumb_h = CARD_THUMB * 9. / 16.;
        let card = match self.card(cx) {
            Card::None => return None,
            Card::Probing => shell()
                .child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .child(Skeleton::new().w(px(CARD_THUMB)).h(px(thumb_h)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .gap(px(8.))
                                .child(Skeleton::new().w(px(280.)).h(px(16.)))
                                .child(Skeleton::new().w(px(160.)).h(px(12.)))
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .text_color(muted)
                                        .child(Spinner::new("card-reading"))
                                        .child(t!("dl-probing")),
                                )
                                .child(ProgressRule::new(Rule::Pulse)),
                        ),
                )
                .into_any_element(),
            Card::NotLink => shell()
                .child(self.problem(
                    t!("dl-notlink-title"),
                    t!("dl-notlink"),
                    None,
                    vec![dismiss(cx).into_any_element()],
                    cx,
                ))
                .id("not-a-link")
                .role(gpui::accesskit::Role::Group)
                .aria_label(t!("dl-notlink-title"))
                .into_any_element(),
            Card::Failed(error) => {
                let actions = vec![
                    Button::new("open-settings")
                        .ghost()
                        .label(t!("dl-open-settings"))
                        .on_click(|_, _, cx| router::open_settings(cx))
                        .into_any_element(),
                    Button::new("probe-retry")
                        .outline()
                        .icon("icons/rotate-cw.svg")
                        .label(t!("dl-try-again"))
                        .on_click(cx.listener(|this, _, _, cx| this.submit(cx)))
                        .into_any_element(),
                    dismiss(cx).into_any_element(),
                ];
                shell()
                    .child(self.problem(
                        t!("dl-probe-failed"),
                        error_text(error.code),
                        Some(error.message.into()),
                        actions,
                        cx,
                    ))
                    .id("probe-failed")
                    .role(gpui::accesskit::Role::Group)
                    .aria_label(t!("dl-probe-failed"))
                    .into_any_element()
            }
            Card::Result(probe) => {
                let remembered = self.preset == self.remembered;
                let remembered_label = format::preset_label(self.remembered);
                let meta = [
                    probe.uploader.clone().unwrap_or_default(),
                    domain::domain_of(&probe.url),
                ]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("  \u{b7}  ");
                let note = div()
                    .flex_none()
                    .text_size(theme.text(Text::Small))
                    .text_color(muted)
                    .when(remembered, |this| {
                        this.child(t!("dl-remembered", preset = remembered_label.as_str()))
                    });
                let actions = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("choose-formats")
                            .ghost()
                            .label(t!("dl-choose-formats"))
                            .on_click(cx.listener(|this, _, _, cx| this.open_formats(cx))),
                    )
                    .child(self.download_button("card-download", None, cx));
                shell()
                    .relative()
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child({
                                let thumb = Thumbnail::new(
                                    probe.thumbnail_url.clone().map(SharedString::from),
                                    px(CARD_THUMB),
                                );
                                match probe.duration_s {
                                    Some(d) => thumb.pill(format::duration(d)),
                                    None => thumb,
                                }
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .truncate()
                                            .pr(px(32.))
                                            .text_size(theme.text(Text::Heading))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(probe.title.clone().unwrap_or_default()),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(theme.text(Text::Small))
                                            .text_color(muted)
                                            .child(meta),
                                    )
                                    .child(
                                        div().mt(px(8.)).child(self.preset_rows(note, actions, cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(8.))
                            .right(px(8.))
                            .child(dismiss(cx)),
                    )
                    .into_any_element()
            }
            Card::Batch => self.batch_card(shell(), cx),
            Card::Playlist(probe) => self.playlist_card(shell(), &probe, cx),
        }; // the card unfolds once as it appears: height and opacity over 160 ms
        let span = match ui::motion::animates(cx) {
            true => std::time::Duration::from_millis(160),
            false => std::time::Duration::from_millis(1),
        };
        Some(
            div()
                .overflow_hidden()
                .child(card)
                .with_animation(
                    "card-in",
                    Animation::new(span).with_easing(ease_out_cubic),
                    move |this, delta| match compact {
                        true => this.opacity(delta).max_h(px(CARD_MAX * delta)),
                        false => this.opacity(delta),
                    },
                )
                .into_any_element(),
        )
    }

    fn download_button(
        &self,
        id: &'static str,
        count: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .primary()
            .label(match count {
                Some(n) => t!("dl-download-count", count = n),
                None => t!("dl-download"),
            })
            .disabled(count == Some(0))
            .on_click(cx.listener(|this, _, _, cx| this.download(cx)))
    }

    fn batch_card(&self, shell: gpui::Div, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let muted = theme.muted_foreground;
        let mut ready = 0;
        // each line: the link, what became of it, and a state icon (None = still reading)
        let mut lines: Vec<BatchLine> = Vec::new();
        for url in &self.urls {
            let (note, icon, dim) = match self.app.probes.read(cx).get(url) {
                None | Some(ProbeState::Loading) => (t!("dl-probing"), None, true),
                Some(ProbeState::Failed(e)) => (
                    error_text(e.code),
                    Some(("icons/circle-alert.svg", theme.danger)),
                    true,
                ),
                Some(ProbeState::Ready(_)) if self.have(url, cx) => (
                    t!("dl-already"),
                    Some(("icons/circle-check.svg", muted)),
                    true,
                ),
                Some(ProbeState::Ready(p)) if p.is_playlist() => (
                    t!("dl-batch-playlist"),
                    Some(("icons/list.svg", muted)),
                    true,
                ),
                Some(ProbeState::Ready(p)) => {
                    ready += 1;
                    (
                        p.title.clone().unwrap_or_default().into(),
                        Some(("icons/download.svg", theme.foreground)),
                        false,
                    )
                }
            };
            lines.push((domain::domain_of(url) + short_path(url), note, icon, dim));
        }
        lines.extend(self.skipped.iter().map(|word| {
            (
                word.clone(),
                t!("dl-batch-skipped"),
                Some(("icons/minus.svg", muted)),
                true,
            )
        }));
        shell
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().font_weight(FontWeight::MEDIUM).child(t!(
                        "dl-batch-title",
                        count = self.urls.len() + self.skipped.len()
                    )))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                Button::new("batch-cancel")
                                    .ghost()
                                    .label(t!("dl-cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.intake.update(cx, |i, cx| i.set_text("", cx));
                                        this.clear_card(cx);
                                    })),
                            )
                            .child(self.download_button("batch-download", Some(ready), cx)),
                    ),
            )
            .child(self.preset_rows(div(), div(), cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .border_t_1()
                    .border_color(theme.border)
                    .children(lines.into_iter().enumerate().map(
                        |(at, (label, note, icon, dim))| {
                            div()
                                .flex()
                                .gap(px(12.))
                                .h(px(32.))
                                .items_center()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(
                                    div().w(px(16.)).flex_none().flex().justify_center().child(
                                        match icon {
                                            Some((path, color)) => svg()
                                                .path(icons::path(path))
                                                .size(px(14.))
                                                .text_color(color)
                                                .into_any_element(),
                                            None => Spinner::new(("batch-reading", at))
                                                .into_any_element(),
                                        },
                                    ),
                                )
                                .child(
                                    div()
                                        .w(px(280.))
                                        .flex_none()
                                        .truncate()
                                        .font_family(MONO)
                                        .text_size(theme.text(Text::Small))
                                        .text_color(muted)
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(if dim { muted } else { theme.foreground })
                                        .child(note),
                                )
                        },
                    )),
            )
            .into_any_element()
    }
    fn playlist_card(
        &self,
        shell: gpui::Div,
        probe: &Arc<ProbeResult>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let muted = theme.muted_foreground;
        let entries = Arc::new(probe.entries.clone().unwrap_or_default());
        let total = entries.len();
        let selected = entries
            .iter()
            .filter(|e| !self.playlist_off.contains(&e.index))
            .count();
        let none = entries.clone();
        let list = entries;
        let applied = format::preset_label(self.preset);
        let title = probe.title.clone().unwrap_or_default();
        shell
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().font_weight(FontWeight::MEDIUM).child(t!(
                        "dl-playlist-title",
                        count = total,
                        title = title.as_str()
                    )))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                Button::new("sel-all")
                                    .ghost()
                                    .label(t!("dl-select-all"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.playlist_off.clear();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("sel-none")
                                    .ghost()
                                    .label(t!("dl-select-none"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.playlist_off = none.iter().map(|e| e.index).collect();
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(self.preset_rows(div(), div(), cx))
            .child(
                uniform_list(
                    "playlist-entries",
                    total,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let theme = *cx.theme();
                        range
                            .filter_map(|at| list.get(at).cloned())
                            .map(|entry| {
                                let index = entry.index;
                                let on = !this.playlist_off.contains(&index);
                                let have = this.have(&entry.url, cx);
                                div()
                                    .id(("entry-row", index as usize))
                                    .flex()
                                    .w_full()
                                    .items_center()
                                    .gap(px(12.))
                                    .h(px(32.))
                                    .px(px(4.))
                                    .rounded(theme.radius)
                                    .hover(|style| style.bg(theme.secondary_hover))
                                    .when(!on, |this| this.text_color(theme.muted_foreground))
                                    .child(Checkbox::new(("entry", index as usize), on).on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            if !this.playlist_off.remove(&index) {
                                                this.playlist_off.insert(index);
                                            }
                                            cx.notify();
                                        }),
                                    ))
                                    .child(
                                        div()
                                            .w(px(32.))
                                            .flex_none()
                                            .font_family(MONO)
                                            .text_size(theme.text(Text::Small))
                                            .text_color(theme.muted_foreground)
                                            .child(format!("{index:03}")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .child(entry.title.unwrap_or_default()),
                                    )
                                    .when(have, |this| {
                                        this.child(
                                            div()
                                                .flex_none()
                                                .text_size(theme.text(Text::Small))
                                                .text_color(theme.muted_foreground)
                                                .child(t!("dl-already")),
                                        )
                                    })
                                    .child(
                                        div()
                                            .w(px(48.))
                                            .flex_none()
                                            .text_right()
                                            .font_features(tabular())
                                            .text_color(theme.muted_foreground)
                                            .child(format::duration(
                                                entry.duration_s.unwrap_or(0.),
                                            )),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .w_full()
                .h(px(32. * total.min(PLAYLIST_ROWS) as f32)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_color(muted).child(t!(
                        "dl-playlist-apply",
                        preset = applied.as_str(),
                        count = selected
                    )))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                Button::new("playlist-cancel")
                                    .ghost()
                                    .label(t!("dl-cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.intake.update(cx, |i, cx| i.set_text("", cx));
                                        this.clear_card(cx);
                                    })),
                            )
                            .child(self.download_button("playlist-download", Some(selected), cx)),
                    ),
            )
            .into_any_element()
    }
}

/// The path part of a link, to tell links of one site apart.
fn short_path(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.find(['/', '?']).map_or("", |at| &rest[at..])
}
