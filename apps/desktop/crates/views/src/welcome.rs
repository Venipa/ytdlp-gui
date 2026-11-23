//! The Getting ready page over `state::Tools`. It continues by itself when every tool is ready.

use domain::{ToolKind, ToolSource, ToolState};
use gpui::prelude::*;
use gpui::{AnyElement, App, FontWeight, PathPromptOptions, SharedString, div, px, svg};
use i18n::t;
use router::Destination;
use state::AppState;
use ui::{ActiveTheme as _, Button, Perch, ProgressRule, Rule, Spinner, Text, Tipped as _, tabular};

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
    state
        .store
        .write(|db| async move { db.state_set("first_run_done", "1").await });
    router::navigate(Destination::Downloads, cx);
}

/// Whether Continue may be pressed: the downloader and ffmpeg are usable.
pub fn can_continue(cx: &App) -> bool {
    let tools = AppState::global(cx).tools;
    let tools = tools.read(cx);
    [ToolKind::YtDlp, ToolKind::Ffmpeg]
        .iter()
        .all(|kind| tools.get(*kind).is_none_or(|s| s.state == ToolState::Ready))
        && tools.detected()
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
            tools.update(cx, |t, cx| t.use_custom(ToolKind::Ffmpeg, path, cx));
        }
    })
    .detach();
}

/// The part of a version string that people read: `7.0.1-essentials_build-www.gyan.dev` is `7.0.1`.
fn short_version(version: &str) -> &str {
    version
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
        .next()
        .unwrap_or(version)
}

fn tool_icon(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::YtDlp => "icons/download.svg",
        ToolKind::YtDlpEjs => "icons/link.svg",
        ToolKind::Ffmpeg => "icons/film.svg",
        ToolKind::Ffprobe => "icons/info.svg",
        ToolKind::Deno => "icons/settings.svg",
        ToolKind::Python => "icons/list.svg",
    }
}

/// The Getting ready screen: one centred column, a line per tool; the rule under it is its progress.
pub fn welcome(cx: &App) -> AnyElement {
    let theme = *cx.theme();
    let pencil = theme.muted_foreground;
    let tools = AppState::global(cx).tools;
    let statuses = tools.read(cx).all().to_vec();
    let failed = statuses
        .iter()
        .any(|s| matches!(s.state, ToolState::Failed(_)));
    let go = can_continue(cx);
    let rows = statuses
        .into_iter()
        .filter(|tool| tool.kind != ToolKind::Python)
        .map(|tool| {
            let mut danger = false;
            let (state, rule): (SharedString, Rule) = match &tool.state {
                ToolState::Ready if tool.source == Some(ToolSource::Path) => {
                    (t!("welcome-found"), Rule::Solid)
                }
                ToolState::Ready => (t!("welcome-ready"), Rule::Solid),
                ToolState::Checking => (t!("welcome-checking"), Rule::Dotted),
                ToolState::Missing => (t!("welcome-waiting"), Rule::Dotted),
                ToolState::Failed(error) => {
                    danger = true;
                    (error_text(error.code), Rule::Failed)
                }
                ToolState::Downloading { done, total } => (
                    match total {
                        Some(total) => t!(
                            "welcome-progress",
                            done = format::bytes(*done as i64),
                            total = format::bytes(*total as i64)
                        ),
                        None => t!("welcome-downloading"),
                    },
                    Rule::Fill(total.map_or(0., |t| *done as f32 / t.max(1) as f32)),
                ),
            };
            let key = tool.kind as usize;
            let lead: AnyElement = match &tool.state {
                ToolState::Downloading { .. } | ToolState::Checking => {
                    Spinner::new(("tool-spin", key)).into_any_element()
                }
                ToolState::Ready => svg()
                    .path(icons::path("icons/circle-check.svg"))
                    .size(px(16.))
                    .text_color(pencil)
                    .into_any_element(),
                ToolState::Failed(_) => svg()
                    .path(icons::path("icons/circle-alert.svg"))
                    .size(px(16.))
                    .text_color(theme.danger)
                    .into_any_element(),
                ToolState::Missing => div().size(px(16.)).into_any_element(),
            };
            let version = tool.version.clone().unwrap_or_default();
            let short = short_version(&version).to_string();
            let long = version != short;
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .h(px(40.))
                        .items_center()
                        .gap_3()
                        .child(
                            svg()
                                .path(icons::path(tool_icon(tool.kind)))
                                .size(px(16.))
                                .flex_none()
                                .text_color(pencil),
                        )
                        .child(
                            div()
                                .w(px(112.))
                                .flex_none()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tool_name(tool.kind)),
                        )
                        .child(
                            div()
                                .id(("tool-version", key))
                                .w(px(80.))
                                .flex_none()
                                .truncate()
                                .font_features(tabular())
                                .text_color(pencil)
                                .child(short)
                                .when(long, |this| this.tip_label(version.clone(), Perch::Below)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_right()
                                .font_features(tabular())
                                .text_color(if danger { theme.danger } else { pencil })
                                .child(state),
                        )
                        .child(div().flex_none().w(px(16.)).child(lead)),
                )
                .child(ProgressRule::new(rule))
        });
    div()
        .size_full()
        .flex()
        .justify_center()
        .pt(px(72.))
        .child(
            div()
                .w_full()
                .max_w(px(560.))
                .px_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_size(theme.text(Text::Title))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t!("welcome-title")),
                )
                .child(div().text_color(pencil).child(t!("welcome-detail")))
                .child(div().flex().flex_col().mt_2().children(rows))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .mt_3()
                        .child(
                            Button::new("own-ffmpeg")
                                .ghost()
                                .small()
                                .label(t!("welcome-own-ffmpeg"))
                                .on_click(|_, _, cx| choose_ffmpeg(cx)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .when(!go && !failed, |this| {
                                    this.child(
                                        div().text_color(pencil).child(t!("welcome-continue-wait")),
                                    )
                                })
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
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    #[test]
    fn short_version_drops_build_suffix() {
        assert_eq!(
            super::short_version("7.0.1-essentials_build-www.gyan.dev"),
            "7.0.1"
        );
        assert_eq!(super::short_version("2026.03.13"), "2026.03.13");
    }
}
