//! Welcome, Logs, Settings pages. Plain functions over plain data.

use domain::{ToolKind, ToolSource, ToolState};
use gpui::prelude::*;
use gpui::{AnyElement, App, FontWeight, SharedString, div, px};
use i18n::t;
use ui::{ActiveTheme as _, Button, Chip, MONO, ProgressRule, Rule, Text, tabular};

use crate::format;
use crate::sample::Data;

fn tool_key(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::YtDlp => "welcome-tool-yt-dlp",
        ToolKind::YtDlpEjs => "welcome-tool-yt-dlp-ejs",
        ToolKind::Ffmpeg => "welcome-tool-ffmpeg",
        ToolKind::Ffprobe => "welcome-tool-ffprobe",
        ToolKind::Deno => "welcome-tool-deno",
        ToolKind::Python => "welcome-tool-python",
    }
}

/// The Getting ready screen.
pub fn welcome(data: &Data, cx: &App) -> AnyElement {
    let theme = *cx.theme();
    let pencil = theme.muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p(theme.metrics.pad * 2.)
        .max_w(px(640.))
        .child(
            div()
                .text_size(theme.text(Text::Title))
                .font_weight(FontWeight::SEMIBOLD)
                .child(t!("welcome-title")),
        )
        .child(div().text_color(pencil).child(t!("welcome-detail")))
        .children(
            data.tools
                .iter()
                .filter(|tool| tool.kind != ToolKind::Python)
                .map(|tool| {
                    let (state, rule, extra): (SharedString, Rule, Option<String>) =
                        match &tool.state {
                            ToolState::Ready if tool.source == Some(ToolSource::Path) => {
                                (t!("welcome-found"), Rule::Solid, None)
                            }
                            ToolState::Ready => (t!("welcome-ready"), Rule::Solid, None),
                            ToolState::Checking => (t!("welcome-waiting"), Rule::Dotted, None),
                            ToolState::Missing => (t!("welcome-missing"), Rule::Dotted, None),
                            ToolState::Failed(_) => (t!("welcome-failed"), Rule::Failed, None),
                            ToolState::Downloading { done, total } => (
                                t!("welcome-downloading"),
                                Rule::Fill(total.map_or(0., |t| *done as f32 / t as f32)),
                                total.map(|t| {
                                    t!(
                                        "welcome-progress",
                                        done = format::bytes(*done as i64),
                                        total = format::bytes(t as i64)
                                    )
                                    .to_string()
                                }),
                            ),
                        };
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .h(px(32.))
                                .items_center()
                                .child(
                                    div()
                                        .w(px(120.))
                                        .child(i18n::lookup(tool_key(tool.kind), None)),
                                )
                                .child(
                                    div()
                                        .w(px(120.))
                                        .font_features(tabular())
                                        .text_color(pencil)
                                        .child(tool.version.clone().unwrap_or_default()),
                                )
                                .child(div().w(px(200.)).child(state))
                                .child(
                                    div()
                                        .font_features(tabular())
                                        .text_color(pencil)
                                        .child(extra.unwrap_or_default()),
                                ),
                        )
                        .child(ProgressRule::new(rule))
                }),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .mt_3()
                .child(
                    Button::new("own-ffmpeg")
                        .ghost()
                        .label(t!("welcome-own-ffmpeg")),
                )
                .child(
                    Button::new("welcome-continue")
                        .primary()
                        .disabled(true)
                        .label(t!("welcome-continue")),
                ),
        )
        .into_any_element()
}

/// The Logs page.
pub fn logs(data: &Data, cx: &App) -> AnyElement {
    let theme = *cx.theme();
    let pencil = theme.muted_foreground;
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
                .child(
                    div().flex().gap_2().children(
                        [
                            "dl-logs-all",
                            "dl-logs-info",
                            "dl-logs-warn",
                            "dl-logs-error",
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(i, key)| Chip::new(key, i18n::lookup(key, None)).selected(i == 0)),
                    ),
                )
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
                                .label(t!("dl-logs-all-jobs")),
                        )
                        .child(Button::new("logs-copy").ghost().label(t!("dl-logs-copy"))),
                ),
        )
        .children(data.logs.iter().map(|l| {
            let tone = match l.level {
                "error" => theme.danger,
                "warn" => theme.progress_bar,
                _ => pencil,
            };
            div()
                .flex()
                .gap_3()
                .h(px(32.))
                .items_center()
                .font_family(MONO)
                .text_size(theme.text(Text::Small))
                .child(div().w(px(64.)).text_color(pencil).child(l.time))
                .child(div().w(px(44.)).text_color(tone).child(l.level))
                .child(
                    div()
                        .w(px(40.))
                        .text_color(pencil)
                        .child(l.job.map(|j| format!("{j:04}")).unwrap_or_default()),
                )
                .child(div().flex_1().truncate().child(l.text))
        }))
        .into_any_element()
}
