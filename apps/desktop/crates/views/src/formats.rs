//! The Choose formats modal over a probe's formats.

use domain::{FormatInfo, Preset, ProbeResult};
use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, div, px};
use i18n::t;
use ui::{ActiveTheme as _, Button, MONO, Modal, Text, tabular};

use crate::downloads::{Card, Downloads};
use crate::format;
use crate::toasts::{Kind, toast};

/// The ids picked in the modal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub video: Option<String>,
    pub audio: Option<String>,
}

fn present(codec: &Option<String>) -> bool {
    codec.as_deref().is_some_and(|c| c != "none")
}

fn has_video(f: &FormatInfo) -> bool {
    present(&f.vcodec)
}

fn has_audio(f: &FormatInfo) -> bool {
    present(&f.acodec)
}

/// The format string the draft stands for: `137+140`, or the one id.
pub fn spec(draft: &Draft, probe: &ProbeResult) -> String {
    let video = draft
        .video
        .as_ref()
        .and_then(|id| probe.formats.iter().find(|f| &f.id == id));
    match (video, &draft.audio) {
        (Some(v), Some(a)) if !has_audio(v) => format!("{}+{a}", v.id),
        (Some(v), _) => v.id.clone(),
        (None, Some(a)) => a.clone(),
        (None, None) => String::new(),
    }
}

/// The best video and the best audio-only format, the modal's starting pick.
pub fn defaults(probe: &ProbeResult) -> Draft {
    Draft {
        video: probe
            .formats
            .iter()
            .rev()
            .find(|f| has_video(f))
            .map(|f| f.id.clone()),
        audio: probe
            .formats
            .iter()
            .rev()
            .find(|f| has_audio(f) && !has_video(f))
            .map(|f| f.id.clone()),
    }
}

impl Downloads {
    /// The format-column label of a custom pick: `1080p` from the video, else the spec itself.
    pub(crate) fn formats_label(&self, probe: Option<&ProbeResult>, spec: &str) -> String {
        let first = spec.split('+').next().unwrap_or(spec);
        probe
            .and_then(|p| p.formats.iter().find(|f| f.id == first))
            .map(|f| match (f.height, &f.ext) {
                (Some(h), _) => format!("{h}p"),
                (None, Some(ext)) => ext.clone(),
                _ => spec.to_owned(),
            })
            .unwrap_or_else(|| spec.to_owned())
    }

    /// Opens the modal on the card's video.
    pub(crate) fn open_formats(&mut self, cx: &mut Context<Self>) {
        if let Card::Result(p) = self.card(cx) {
            self.formats = Some(defaults(&p));
            cx.notify();
        }
    }

    /// Download with the picked formats.
    fn download_formats(&mut self, cx: &mut Context<Self>) {
        let (Card::Result(p), Some(draft)) = (self.card(cx), self.formats.take()) else {
            return;
        };
        let spec = spec(&draft, &p);
        if spec.is_empty() {
            return;
        }
        self.enqueue(&p.url, Preset::Custom, &spec, None, Some(&p), cx);
        toast(cx, Kind::Success, t!("dl-toast-added-one"), None);
        self.intake.update(cx, |input, cx| input.set_text("", cx));
        self.clear_card(cx);
    }

    fn format_table(
        &self,
        title: SharedString,
        formats: Vec<&FormatInfo>,
        video: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let pencil = theme.muted_foreground;
        let picked = self.formats.as_ref().and_then(|d| match video {
            true => d.video.clone(),
            false => d.audio.clone(),
        });
        let cell = |w: f32, text: String| div().w(px(w)).flex_none().truncate().child(text);
        let mono = |w: f32, text: String| {
            div()
                .w(px(w))
                .flex_none()
                .truncate()
                .font_family(MONO)
                .text_size(theme.text(Text::Small))
                .child(text)
        };
        div()
            .flex()
            .flex_col()
            .child(div().py_1().text_color(pencil).child(title))
            .child(
                div()
                    .flex()
                    .h(px(32.))
                    .items_center()
                    .text_color(pencil)
                    .text_size(theme.text(Text::Label))
                    .child(cell(60., t!("dl-col-id").to_string()))
                    .child(cell(60., t!("dl-col-ext").to_string()))
                    .child(cell(110., t!("dl-col-resolution").to_string()))
                    .child(cell(110., t!("dl-col-codec").to_string()))
                    .child(cell(80., t!("dl-col-size").to_string()))
                    .child(cell(120., t!("dl-col-note").to_string())),
            )
            .children(formats.into_iter().map(|f| {
                let resolution = match (f.width, f.height) {
                    (Some(w), Some(h)) => format!("{w}x{h}"),
                    _ => f.abr.map(|a| format!("{a:.0} kbps")).unwrap_or_default(),
                };
                let codec = match video {
                    true => f.vcodec.clone(),
                    false => f.acodec.clone(),
                }
                .unwrap_or_default();
                let id = f.id.clone();
                let on = picked.as_deref() == Some(f.id.as_str());
                div()
                    .id(SharedString::from(format!("fmt-{video}-{}", f.id)))
                    .flex()
                    .h(px(32.))
                    .items_center()
                    .cursor_pointer()
                    .when(on, |this| this.bg(theme.table_active))
                    .hover(|style| style.bg(theme.table_hover))
                    .border_b_1()
                    .border_color(theme.table_row_border)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(draft) = this.formats.as_mut() {
                            let slot = match video {
                                true => &mut draft.video,
                                false => &mut draft.audio,
                            };
                            *slot = Some(id.clone());
                        }
                        cx.notify();
                    }))
                    .child(mono(60., f.id.clone()))
                    .child(cell(60., f.ext.clone().unwrap_or_default()))
                    .child(cell(110., resolution))
                    .child(mono(110., codec))
                    .child(
                        div().w(px(80.)).flex_none().font_features(tabular()).child(
                            f.filesize
                                .map(|n| format::bytes(n as i64))
                                .unwrap_or_default(),
                        ),
                    )
                    .child(cell(120., f.note.clone().unwrap_or_default()))
            }))
            .into_any_element()
    }

    /// The modal, when open.
    pub(crate) fn formats_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let draft = self.formats.as_ref()?;
        let Card::Result(probe) = self.card(cx) else {
            return None;
        };
        let theme = *cx.theme();
        let video: Vec<_> = probe
            .formats
            .iter()
            .rev()
            .filter(|f| has_video(f))
            .collect();
        let audio: Vec<_> = probe
            .formats
            .iter()
            .rev()
            .filter(|f| has_audio(f) && !has_video(f))
            .collect();
        let result = spec(draft, &probe);
        Some(
            Modal::new("choose-formats-modal", t!("dl-formats-title"))
                .wide()
                .close_button()
                .on_dismiss(cx.listener(|this, _, _, cx| {
                    this.formats = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .w_full()
                        .child(self.format_table(t!("dl-formats-video"), video, true, cx))
                        .child(self.format_table(t!("dl-formats-audio"), audio, false, cx))
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .items_center()
                                .child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .child(t!("dl-formats-result")),
                                )
                                .child(div().font_family(MONO).child(result.clone())),
                        ),
                )
                .action(
                    Button::new("formats-cancel")
                        .ghost()
                        .label(t!("dl-cancel"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.formats = None;
                            cx.notify();
                        })),
                )
                .action(
                    Button::new("formats-download")
                        .primary()
                        .disabled(result.is_empty())
                        .label(t!("dl-download"))
                        .on_click(cx.listener(|this, _, _, cx| this.download_formats(cx))),
                )
                .into_any_element(),
        )
    }
}
