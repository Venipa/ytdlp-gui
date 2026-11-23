//! The Getting ready page over `state::Tools`. It continues by itself when every tool is ready.

use domain::{ToolKind, ToolSource, ToolState};
use gpui::prelude::*;
use gpui::{AnyElement, App, FontWeight, PathPromptOptions, SharedString, div, px};
use i18n::t;
use router::Destination;
use state::AppState;
use ui::{ActiveTheme as _, Button, ProgressRule, Rule, Text, tabular};

use crate::downloads::error_text;
use crate::format;

pub(crate) fn tool_key(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::YtDlp => "welcome-tool-yt-dlp",
        ToolKind::YtDlpEjs => "welcome-tool-yt-dlp-ejs",
        ToolKind::Ffmpeg => "welcome-tool-ffmpeg",
        ToolKind::Ffprobe => "welcome-tool-ffprobe",
        ToolKind::Deno => "welcome-tool-deno",
        ToolKind::Python => "welcome-tool-python",
    }
}

/// The name a tool goes by in the interface.
pub fn tool_name(kind: ToolKind) -> SharedString {
    i18n::lookup(tool_key(kind), None)
}

/// Remembers the first run is over and shows Downloads.
pub fn leave(cx: &mut App) {
    let state = AppState::global(cx);
    state.store.write(|db| async move {
        db.state_set("first_run_done", "1").await
    });
    router::navigate(Destination::Downloads, cx);
}

/// Whether Continue may be pressed: the downloader and ffmpeg are usable.
pub fn can_continue(cx: &App) -> bool {
    let tools = AppState::global(cx).tools;
    let tools = tools.read(cx);
    [ToolKind::YtDlp, ToolKind::Ffmpeg].iter().all(|kind| {
        tools
            .get(*kind)
            .is_none_or(|s| s.state == ToolState::Ready)
    }) && tools.detected()
}

/// Asks for an ffmpeg binary and uses it.
fn choose_ffmpeg(cx: &mut App) {
    let tools = AppState::global(cx).tools;
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = chosen.await
            && let Some(path) = paths.first()
        {
            let path = path.to_string_lossy().into_owned();
            tools
                .update(cx, |t, cx| t.use_custom(ToolKind::Ffmpeg, path, cx));
        }
    })
    .detach();
}

/// The Getting ready screen.
pub fn welcome(cx: &App) -> AnyElement {
    let theme = *cx.theme();
    let pencil = theme.muted_foreground;
    let tools = AppState::global(cx).tools;
    let statuses = tools.read(cx).all().to_vec();
    let failed = statuses
        .iter()
        .any(|s| matches!(s.state, ToolState::Failed(_)));
    let go = can_continue(cx);
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
            statuses
                .into_iter()
                .filter(|tool| tool.kind != ToolKind::Python)
                .map(|tool| {
                    let (state, rule, extra): (SharedString, Rule, Option<String>) =
                        match &tool.state {
                            ToolState::Ready if tool.source == Some(ToolSource::Path) => {
                                (t!("welcome-found"), Rule::Solid, None)
                            }
                            ToolState::Ready => (t!("welcome-ready"), Rule::Solid, None),
                            ToolState::Checking => (t!("welcome-checking"), Rule::Dotted, None),
                            ToolState::Missing => (t!("welcome-waiting"), Rule::Dotted, None),
                            ToolState::Failed(error) => {
                                (error_text(error.code), Rule::Failed, None)
                            }
                            ToolState::Downloading { done, total } => (
                                t!("welcome-downloading"),
                                Rule::Fill(total.map_or(0., |t| *done as f32 / t.max(1) as f32)),
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
                                .child(div().w(px(120.)).child(tool_name(tool.kind)))
                                .child(
                                    div()
                                        .w(px(120.))
                                        .font_features(tabular())
                                        .text_color(pencil)
                                        .child(tool.version.clone().unwrap_or_default()),
                                )
                                .child(div().w(px(240.)).truncate().child(state))
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
                        .label(t!("welcome-own-ffmpeg"))
                        .on_click(|_, _, cx| choose_ffmpeg(cx)),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .when(failed, |this| {
                            let tools = tools.clone();
                            this.child(
                                Button::new("welcome-retry")
                                    .outline()
                                    .label(t!("dl-try-again"))
                                    .on_click(move |_, _, cx| {
                                        tools.update(cx, |t, cx| t.ensure_missing(cx))
                                    }),
                            )
                        })
                        .child(
                            Button::new("welcome-continue")
                                .primary()
                                .disabled(!go)
                                .label(t!("welcome-continue"))
                                .on_click(|_, _, cx| leave(cx)),
                        ),
                ),
        )
        .into_any_element()
}
