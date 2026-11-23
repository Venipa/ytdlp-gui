//! The card under the intake: reading, result with the remembered preset, batch, playlist, failure.

use std::sync::Arc;

use domain::{AudioFormat, Preset, ProbeResult, VideoQuality};
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, px, uniform_list};
use i18n::t;
use state::ProbeState;
use ui::{
    ActiveTheme as _, Button, Checkbox, Chip, MONO, Notice, Skeleton, Text, Thumbnail, tabular,
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

    /// The two chip rows, Video and Audio, with `extra` at the end of the audio row.
    fn preset_rows(&self, extra: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let name = |text: SharedString| div().w(px(48.)).text_color(muted).child(text);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(name(t!("dl-card-video")))
                    .children(VIDEO.map(|q| {
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
                    .child(name(t!("dl-card-audio")))
                    .children(AUDIO.map(|a| {
                        let label = match a {
                            AudioFormat::Best => t!("dl-preset-best"),
                            a => SharedString::from(a.as_str()),
                        };
                        self.preset_chip(Preset::Audio(a), label, cx)
                    }))
                    .child(div().flex_1())
                    .child(extra),
            )
            .into_any_element()
    }

    /// The card, when there is something to show.
    pub(crate) fn card_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
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
        let dismiss = |cx: &mut Context<Self>| {
            Button::new("card-cancel")
                .ghost()
                .label(t!("dl-cancel"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.intake.update(cx, |input, cx| input.set_text("", cx));
                    this.clear_card(cx);
                }))
        };
        Some(match self.card(cx) {
            Card::None => return None,
            Card::Probing => shell()
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(Skeleton::new().w(px(240.)).h(px(135.)))
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
            Card::NotLink => shell()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    Notice::new(t!("dl-notlink-title"), t!("dl-notlink"))
                        .failed()
                        .spoken("not-a-link"),
                )
                .child(dismiss(cx))
                .into_any_element(),
            Card::Failed(error) => shell()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            Notice::new(t!("dl-probe-failed"), error_text(error.code))
                                .failed()
                                .spoken("probe-failed"),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(dismiss(cx))
                                .child(
                                    Button::new("probe-retry")
                                        .outline()
                                        .label(t!("dl-try-again"))
                                        .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                                )
                                .child(
                                    Button::new("open-settings")
                                        .outline()
                                        .label(t!("dl-open-settings"))
                                        .on_click(|_, _, cx| router::open_settings(cx)),
                                ),
                        ),
                )
                .child(
                    div()
                        .truncate()
                        .font_family(MONO)
                        .text_size(theme.text(Text::Tiny))
                        .text_color(muted)
                        .child(error.message),
                )
                .into_any_element(),
            Card::Result(probe) => {
                let remembered = self.preset == self.remembered;
                let remembered_label = format::preset_label(self.remembered);
                shell()
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(Thumbnail::new(
                                probe.thumbnail_url.clone().map(SharedString::from),
                                px(240.),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
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
                                        div().mt_2().child(
                                            self.preset_rows(
                                                Button::new("choose-formats")
                                                    .ghost()
                                                    .label(t!("dl-choose-formats"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.open_formats(cx)
                                                    })),
                                                cx,
                                            ),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_color(muted).when(remembered, |this| {
                                this.child(t!("dl-remembered", preset = remembered_label.as_str()))
                            }))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(dismiss(cx))
                                    .child(self.download_button("card-download", None, cx)),
                            ),
                    )
                    .into_any_element()
            }
            Card::Batch => self.batch_card(shell(), cx),
            Card::Playlist(probe) => self.playlist_card(shell(), &probe, cx),
        })
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
        let mut lines = Vec::new();
        for url in &self.urls {
            let (note, dim): (SharedString, bool) = match self.app.probes.read(cx).get(url) {
                None | Some(ProbeState::Loading) => (t!("dl-probing"), true),
                Some(ProbeState::Failed(e)) => (error_text(e.code), true),
                Some(ProbeState::Ready(_)) if self.have(url, cx) => (t!("dl-already"), true),
                Some(ProbeState::Ready(p)) if p.is_playlist() => (t!("dl-batch-playlist"), true),
                Some(ProbeState::Ready(p)) => {
                    ready += 1;
                    (p.title.clone().unwrap_or_default().into(), false)
                }
            };
            lines.push((domain::domain_of(url) + short_path(url), note, dim));
        }
        lines.extend(
            self.skipped
                .iter()
                .map(|word| (word.clone(), t!("dl-batch-skipped"), true)),
        );
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
                            .gap_2()
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
            .child(self.preset_rows(div(), cx))
            .children(lines.into_iter().map(|(label, note, dim)| {
                div()
                    .flex()
                    .gap_3()
                    .h(px(28.))
                    .items_center()
                    .child(
                        div()
                            .w(px(240.))
                            .flex_none()
                            .truncate()
                            .font_family(MONO)
                            .text_size(theme.text(Text::Small))
                            .child(label),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_color(if dim { muted } else { theme.foreground })
                            .child(note),
                    )
            }))
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
                            .gap_2()
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
            .child(self.preset_rows(div(), cx))
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
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .h(px(32.))
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
                                            .w(px(36.))
                                            .font_family(MONO)
                                            .text_color(theme.muted_foreground)
                                            .child(format!("{index:03}")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .child(entry.title.unwrap_or_default()),
                                    )
                                    .child(
                                        div().font_features(tabular()).child(format::duration(
                                            entry.duration_s.unwrap_or(0.),
                                        )),
                                    )
                                    .when(have, |this| {
                                        this.child(
                                            div()
                                                .text_color(theme.muted_foreground)
                                                .child(t!("dl-already")),
                                        )
                                    })
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
                            .gap_2()
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
