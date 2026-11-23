//! The Choose formats modal over a probe's formats.

use std::rc::Rc;

use domain::{FormatInfo, Preset, ProbeResult};
use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, Subscription, div, px};
use i18n::t;
use ui::{ActiveTheme as _, Button, MONO, Modal, Radio, Text, tabular};

use crate::downloads::{Card, Downloads};
use crate::format;
use crate::toasts::{Kind, toast};

/// Keeps the open modal's keyboard listener alive; dropping the draft drops it. Every listener
/// compares equal, so a draft compares by its picks.
#[derive(Clone, Default)]
pub struct Listener(
    #[expect(
        dead_code,
        reason = "held so the listener lives exactly as long as the draft"
    )]
    Option<Rc<Subscription>>,
);

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Listener")
    }
}

impl PartialEq for Listener {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// The ids picked in the modal, and which list the arrow keys move in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub video: Option<String>,
    pub audio: Option<String>,
    /// The arrow keys move the audio pick instead of the video pick.
    pub audio_active: bool,
    keys: Listener,
}

/// The id one step from `current` in `ids` (the first when nothing is picked), kept inside the list.
fn stepped(ids: &[String], current: Option<&String>, delta: i32) -> Option<String> {
    let at = current.and_then(|c| ids.iter().position(|i| i == c));
    let next = match at {
        Some(at) => (at as i32 + delta).clamp(0, ids.len() as i32 - 1) as usize,
        None => 0,
    };
    ids.get(next).cloned()
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
        ..Default::default()
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
            let this = cx.entity().downgrade();
            let keys = cx.intercept_keystrokes(move |event, _, cx| {
                let keystroke = &event.keystroke;
                if keystroke.modifiers.modified() {
                    return;
                }
                let handled = this
                    .update(cx, |this, cx| this.formats_key(&keystroke.key, cx))
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            });
            let mut draft = defaults(&p);
            draft.keys = Listener(Some(Rc::new(keys)));
            self.formats = Some(draft);
            cx.notify();
        }
    }

    /// The ids of the two lists, best first, as the modal shows them.
    fn format_ids(&self, cx: &mut Context<Self>) -> Option<(Vec<String>, Vec<String>)> {
        let Card::Result(probe) = self.card(cx) else {
            return None;
        };
        let ids = |video: bool| {
            probe
                .formats
                .iter()
                .rev()
                .filter(|f| match video {
                    true => has_video(f),
                    false => has_audio(f) && !has_video(f),
                })
                .map(|f| f.id.clone())
                .collect()
        };
        Some((ids(true), ids(false)))
    }

    /// A key while the modal is open; true when the modal took it.
    fn formats_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some((video, audio)) = self.format_ids(cx) else {
            return false;
        };
        let Some(draft) = self.formats.as_mut() else {
            return false;
        };
        match key {
            "down" | "up" => {
                let delta = if key == "down" { 1 } else { -1 };
                let (ids, slot) = match draft.audio_active {
                    false => (video, &mut draft.video),
                    true => (audio, &mut draft.audio),
                };
                *slot = stepped(&ids, slot.as_ref(), delta);
                if let Some(at) = slot.as_ref().and_then(|s| ids.iter().position(|i| i == s)) {
                    self.format_scroll[usize::from(draft.audio_active)].scroll_to_item(at);
                }
            }
            "tab" | "left" | "right" => draft.audio_active = !draft.audio_active,
            "enter" => self.download_formats(cx),
            _ => return false,
        }
        cx.notify();
        true
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
        let muted = theme.muted_foreground;
        let (picked, active) = self
            .formats
            .as_ref()
            .map(|d| match video {
                true => (d.video.clone(), !d.audio_active),
                false => (d.audio.clone(), d.audio_active),
            })
            .unwrap_or_default();
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
        // sizes line up on their right edge
        let size = |text: String| {
            div()
                .w(px(80.))
                .flex_none()
                .text_right()
                .font_features(tabular())
                .child(text)
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .py_1()
                    .text_color(match active {
                        true => theme.foreground,
                        false => muted,
                    })
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .h(px(32.))
                    .items_center()
                    .text_color(muted)
                    .text_size(theme.text(Text::Label))
                    .child(div().w(px(28.)).flex_none())
                    .child(cell(60., t!("dl-col-id").to_string()))
                    .child(cell(60., t!("dl-col-ext").to_string()))
                    .child(cell(110., t!("dl-col-resolution").to_string()))
                    .child(cell(110., t!("dl-col-codec").to_string()))
                    .child(div().flex_1().child(t!("dl-col-note")))
                    .child(size(t!("dl-col-size").to_string())),
            )
            .child(
                div()
                    .id(SharedString::from(format!("fmt-scroll-{video}")))
                    .max_h(px(224.))
                    .overflow_y_scroll()
                    .track_scroll(&self.format_scroll[usize::from(!video)])
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
                        let row_id = SharedString::from(format!("fmt-{video}-{}", f.id));
                        div()
                            .id(row_id.clone())
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
                                    draft.audio_active = !video;
                                }
                                cx.notify();
                            }))
                            .child(
                                div().w(px(28.)).flex_none().flex().justify_center().child(
                                    Radio::new(SharedString::from(format!("{row_id}-radio")), on)
                                        .silent(),
                                ),
                            )
                            .child(mono(60., f.id.clone()))
                            .child(cell(60., f.ext.clone().unwrap_or_default()))
                            .child(cell(110., resolution))
                            .child(mono(110., codec))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(muted)
                                    .child(f.note.clone().unwrap_or_default()),
                            )
                            .child(size(
                                f.filesize
                                    .map(|n| format::bytes(n as i64))
                                    .unwrap_or_default(),
                            ))
                    })),
            )
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
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded(theme.radius)
                                        .bg(theme.secondary)
                                        .font_family(MONO)
                                        .text_size(theme.text(Text::Small))
                                        .child(result.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_size(theme.text(Text::Label))
                                .text_color(theme.muted_foreground)
                                .child(t!("dl-formats-hint")),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(id: &str, v: Option<&str>, a: Option<&str>) -> FormatInfo {
        FormatInfo {
            id: id.into(),
            vcodec: v.map(Into::into),
            acodec: a.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn arrow_steps_stay_inside_the_list() {
        let ids: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        assert_eq!(stepped(&ids, None, 1), Some("a".into()));
        assert_eq!(stepped(&ids, Some(&"a".into()), 1), Some("b".into()));
        assert_eq!(stepped(&ids, Some(&"c".into()), 1), Some("c".into()));
        assert_eq!(stepped(&ids, Some(&"a".into()), -1), Some("a".into()));
        assert_eq!(stepped(&[], None, 1), None);
    }

    #[test]
    fn spec_joins_video_and_audio_unless_the_video_has_sound() {
        let probe = ProbeResult {
            formats: vec![
                fmt("140", Some("none"), Some("mp4a")),
                fmt("137", Some("avc1"), Some("none")),
                fmt("18", Some("avc1"), Some("mp4a")),
            ],
            ..Default::default()
        };
        let draft = defaults(&probe);
        assert_eq!(spec(&draft, &probe), "18");
        let video_only = Draft {
            video: Some("137".into()),
            audio: Some("140".into()),
            ..Default::default()
        };
        assert_eq!(spec(&video_only, &probe), "137+140");
        assert_eq!(spec(&Draft::default(), &probe), "");
    }
}
