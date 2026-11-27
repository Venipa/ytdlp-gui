//! The card under the intake: reading, result with the remembered preset, playlist, failure;
//! and the editor that several links open over the list.

use std::sync::Arc;

use domain::{AudioFormat, Preset, ProbeResult, VideoQuality, split_urls};
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ClipboardItem, Context, FontWeight,
    SharedString, deferred, div, px, svg, uniform_list,
};
use i18n::t;
use state::ProbeState;
use ui::{
    ActiveTheme as _, Button, Checkbox, Chip, Dismiss, Kbd, LineNote, MONO, ProgressRule, Rule,
    Skeleton, Spinner, Text, Thumbnail, ease_out_cubic, tabular,
};

use crate::downloads::{Card, Downloads, error_text};
use crate::format;
use crate::toasts::{Kind, toast};

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
/// The width of the result card's thumbnail, and its width in a narrow list.
const CARD_THUMB: f32 = 160.;
const CARD_THUMB_NARROW: f32 = 64.;
/// The width of the Video and Audio labels before the chips.
const LABEL: f32 = 52.;
/// The tallest a one-link card gets, which its unfold opens to.
const CARD_MAX: f32 = 220.;
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

    /// A narrow card's one chip row: the chips of the chosen preset's kind, a switch to the
    /// other kind where the row's name stands (it picks that kind's Best), and `end` after them.
    /// The card loses its second chip row, so the list gets a row back.
    fn preset_row_narrow(&self, end: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        let audio = matches!(self.preset, Preset::Audio(_));
        let chips: Vec<Chip> = match audio {
            true => AUDIO
                .map(|a| {
                    let label = match a {
                        AudioFormat::Best => t!("dl-preset-best"),
                        a => SharedString::from(a.as_str()),
                    };
                    self.preset_chip(Preset::Audio(a), label, cx)
                })
                .into(),
            false => VIDEO
                .map(|q| {
                    let label = match q {
                        VideoQuality::Best => t!("dl-preset-best"),
                        q => SharedString::from(q.as_str()),
                    };
                    self.preset_chip(Preset::Video(q), label, cx)
                })
                .into(),
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                Button::new("card-kind")
                    .ghost()
                    .label(match audio {
                        true => t!("dl-card-audio"),
                        false => t!("dl-card-video"),
                    })
                    .trailing("icons/chevrons-up-down.svg")
                    .tooltip("dl-card-kind-tip")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.preset = match audio {
                            true => Preset::Video(VideoQuality::Best),
                            false => Preset::Audio(AudioFormat::Best),
                        };
                        cx.notify();
                    })),
            )
            .children(chips)
            .child(div().flex_1())
            .child(end)
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
    /// The card; a `narrow` list gets a smaller cover and the title and meta on one line, so
    /// the card gives a list row back.
    pub(crate) fn card_view(&self, narrow: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
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
        let thumb_w = match narrow {
            true => CARD_THUMB_NARROW,
            false => CARD_THUMB,
        };
        let thumb_h = thumb_w * 9. / 16.;
        let card = match self.card(cx) {
            Card::None => return None,
            Card::Probing => shell()
                .child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .child(Skeleton::new().w(px(thumb_w)).h(px(thumb_h)))
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
                // the code and the full message, for a bug report
                let report = format!("{}: {}", error.code, error.message);
                let actions = vec![
                    Button::new("probe-copy-error")
                        .ghost()
                        .icon("icons/copy.svg")
                        .label(t!("dl-copy-error"))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(report.clone()));
                            toast(cx, Kind::Success, t!("dl-copied-error"), None);
                        })
                        .into_any_element(),
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
                // the remembered preset is said once, on the meta line, so the chip rows keep
                // one line each at any width
                let meta = [
                    probe.uploader.clone().unwrap_or_default(),
                    domain::domain_of(&probe.url),
                    match remembered {
                        true => t!("dl-remembered", preset = remembered_label.as_str()).into(),
                        false => String::new(),
                    },
                ]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("  \u{b7}  ");
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
                                    px(thumb_w),
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
                                            .flex()
                                            .when(narrow, |this| this.items_baseline().gap(px(8.)))
                                            .when(!narrow, |this| this.flex_col().gap(px(4.)))
                                            .pr(px(32.))
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(theme.text(Text::Heading))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(probe.title.clone().unwrap_or_default()),
                                            )
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(theme.text(Text::Small))
                                                    .text_color(muted)
                                                    .child(meta),
                                            ),
                                    )
                                    .child(div().mt(px(8.)).child(match narrow {
                                        true => self.preset_row_narrow(actions, cx),
                                        false => self.preset_rows(div(), actions, cx),
                                    })),
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
            // several links show in the editor over the list, so the list never moves for them
            Card::Batch => return None,
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

    /// What one line of the intake says at its end in the editor: the title once read, why it
    /// will not download, or that it is not a link (a wrong line). Nothing before it is read.
    fn line_note(&self, line: &str, cx: &App) -> LineNote {
        let split = split_urls(line);
        let Some(url) = split.urls.first() else {
            return LineNote {
                text: match split.invalid.is_empty() {
                    true => SharedString::default(),
                    false => t!("dl-batch-skipped"),
                },
                bad: !split.invalid.is_empty(),
            };
        };
        let text = match self.app.probes.read(cx).get(url) {
            None => SharedString::default(),
            Some(ProbeState::Loading) => t!("dl-probing"),
            Some(ProbeState::Failed(e)) => error_text(e.code),
            Some(ProbeState::Ready(_)) if self.have(url, cx) => t!("dl-already"),
            Some(ProbeState::Ready(p)) if p.is_playlist() => t!("dl-batch-playlist"),
            Some(ProbeState::Ready(p)) => p.title.clone().unwrap_or_default().into(),
        };
        LineNote { text, bad: false }
    }

    /// How many links of the field a download would add: all but those already in the list,
    /// those that failed to read and playlists (added on their own).
    pub(crate) fn downloadable(&self, cx: &App) -> usize {
        let probes = self.app.probes.read(cx);
        split_urls(self.intake.read(cx).text())
            .urls
            .iter()
            .filter(|url| match probes.get(url) {
                None | Some(ProbeState::Loading) => true,
                Some(ProbeState::Failed(_)) => false,
                Some(ProbeState::Ready(p)) => !p.is_playlist() && !self.have(url, cx),
            })
            .count()
    }

    /// The editor of several links (D-220): it lies over the list at the field's width, so
    /// nothing under it moves. A count head, the lines with what became of each, and the preset
    /// with Download in its foot. Esc or a click outside folds it back into the field.
    pub(crate) fn editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let muted = theme.muted_foreground;
        let text = self.intake.read(cx).text().to_owned();
        // a link listed twice is downloaded once: its second line says so
        let mut seen: Vec<String> = Vec::new();
        let notes: Vec<LineNote> = text
            .split('\n')
            .map(|line| match split_urls(line).urls.into_iter().next() {
                Some(url) if seen.contains(&url) => LineNote {
                    text: t!("dl-intake-twice"),
                    bad: false,
                },
                Some(url) => {
                    seen.push(url);
                    self.line_note(line, cx)
                }
                None => self.line_note(line, cx),
            })
            .collect();
        let skipped = notes.iter().filter(|note| note.bad).count();
        self.intake
            .update(cx, |area, cx| area.set_notes(Some(notes), cx));
        let split = split_urls(&text);
        let count = self.downloadable(cx);
        let bar = || {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(6.))
                .border_color(theme.border)
        };
        let head = bar()
            .border_b_1()
            .text_size(theme.text(Text::Small))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .font_features(tabular())
                    .child(t!("dl-batch-title", count = split.urls.len())),
            )
            .when(skipped > 0, |this| {
                this.child(
                    div()
                        .text_color(muted)
                        .font_features(tabular())
                        .child(t!("dl-intake-skipped", count = skipped)),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(muted)
                    .child(t!("dl-intake-editor-keys")),
            );
        let foot = bar()
            .border_t_1()
            .child(div().text_color(muted).child(t!("dl-batch-preset")))
            .child(
                div()
                    .px(px(8.))
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.table_active_border)
                    .bg(theme.table_active)
                    .text_size(theme.text(Text::Small))
                    .child(format::preset_label(self.preset)),
            )
            .child(div().flex_1())
            .child(Kbd::new("enter"))
            .child(
                Button::new("editor-download")
                    .primary()
                    .icon("icons/download.svg")
                    .label(t!("dl-download-count", count = count))
                    .disabled(count == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            );
        deferred(
            div()
                .id("intake-editor")
                .role(gpui::accesskit::Role::Group)
                .aria_label(t!("dl-intake-editor-name"))
                .absolute()
                .top(theme.metrics.field + px(4.))
                .left_0()
                .right_0()
                .flex()
                .flex_col()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .shadow_lg()
                // a click on the editor never reaches the rows under it
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_editor(cx)))
                .on_action(cx.listener(|this, _: &Dismiss, _, cx| this.close_editor(cx)))
                .child(head)
                .child(div().px(px(12.)).py(px(8.)).child(self.intake.clone()))
                .child(foot),
        )
        .with_priority(1)
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
                // a longer list stops half way into a row, which shows there is more to scroll
                .h(px(32.
                    * match total > PLAYLIST_ROWS {
                        true => PLAYLIST_ROWS as f32 - 0.5,
                        false => total as f32,
                    })),
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
