//! Welcome, Logs, Settings pages. Plain functions over plain data.

use domain::{ToolKind, ToolSource, ToolState, ToolStatus};
use gpui::prelude::*;
use gpui::{AnyElement, App, Entity, FontWeight, SharedString, Window, div, px};
use i18n::t;
use router::{Destination, SettingsPage};
use ui::{
    ActiveTheme as _, Button, Chip, Input, MONO, Notice, ProgressRule, Rule, SideNav, SideNavItem,
    SwitchRow, Text, tabular,
};

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

fn row(label: SharedString, control: impl IntoElement, cx: &App) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex()
        .items_center()
        .gap_3()
        .min_h(px(32.))
        .child(
            div()
                .w(px(200.))
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(control)
        .into_any_element()
}

/// A read-only value box standing in for a field until settings are wired in.
fn value(text: impl Into<SharedString>, mono: bool, cx: &App) -> AnyElement {
    let theme = *cx.theme();
    div()
        .px_2()
        .h(theme.metrics.field)
        .flex()
        .items_center()
        .min_w(px(200.))
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .when(mono, |this| {
            this.font_family(MONO).text_size(theme.text(Text::Small))
        })
        .child(text.into())
        .into_any_element()
}

fn toggle(id: &'static str, key: &'static str, on: bool) -> AnyElement {
    SwitchRow::new(id, i18n::lookup(key, None), on).into_any_element()
}

/// The settings page `page` beside its side nav.
pub fn settings(
    page: SettingsPage,
    data: &Data,
    cookies: &Entity<Input>,
    _window: &mut Window,
    cx: &App,
) -> AnyElement {
    let theme = *cx.theme();
    let pencil = theme.muted_foreground;
    let nav = SideNav::new(
        SettingsPage::ALL
            .into_iter()
            .map(|p| SideNavItem {
                label: i18n::lookup(p.key(), None),
            })
            .collect(),
        SettingsPage::ALL
            .iter()
            .position(|p| *p == page)
            .unwrap_or(0),
        |index, _, cx| router::navigate(Destination::Settings(SettingsPage::ALL[index]), cx),
    );
    let body: Vec<AnyElement> = match page {
        SettingsPage::General => vec![
            row(
                t!("set-language"),
                value(t!("set-language-auto"), false, cx),
                cx,
            ),
            toggle("s-min", "set-start-minimized", false),
            toggle("s-tray", "set-close-to-tray", false),
            toggle("s-notify", "set-notifications", true),
            toggle("s-clip", "set-clipboard-watch", false),
            toggle("s-clip-auto", "set-clipboard-auto", false),
            toggle("s-resume", "set-resume-on-start", false),
        ],
        SettingsPage::Downloads => vec![
            row(
                t!("set-save-to"),
                div()
                    .flex()
                    .gap_2()
                    .child(value("D:\\Videos\\ytdlpd", true, cx))
                    .child(Button::new("choose-dir").outline().label(t!("set-choose"))),
                cx,
            ),
            row(
                t!("set-file-name"),
                value("%(title).180B [%(id)s].%(ext)s", true, cx),
                cx,
            ),
            row(
                t!("set-preview"),
                div()
                    .font_family(MONO)
                    .text_size(theme.text(Text::Small))
                    .child("Never Gonna Give You Up [dQw4w9WgXcQ].mp4"),
                cx,
            ),
            row(
                t!("set-at-once"),
                div()
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(value("3", false, cx))
                    .child(div().text_color(pencil).child(t!("set-per-site")))
                    .child(value("2", false, cx)),
                cx,
            ),
            row(t!("set-retries"), value("5", false, cx), cx),
        ],
        SettingsPage::Formats => vec![
            row(t!("set-default-preset"), value("video:best", true, cx), cx),
            toggle("f-subs", "set-subtitles-embed", false),
            toggle("f-thumb", "set-embed-thumbnail", true),
            toggle("f-meta", "set-embed-metadata", true),
            toggle("f-chap", "set-embed-chapters", true),
            row(
                t!("set-sponsorblock"),
                div().flex().gap_2().children(
                    [
                        "set-sponsorblock-off",
                        "set-sponsorblock-mark",
                        "set-sponsorblock-remove",
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, k)| Chip::new(k, i18n::lookup(k, None)).selected(i == 0)),
                ),
                cx,
            ),
        ],
        SettingsPage::Network => vec![
            row(t!("set-proxy"), value("", true, cx), cx),
            row(t!("set-rate-limit"), value("", true, cx), cx),
            row(
                t!("set-cookies"),
                div().flex().gap_2().children(
                    [
                        "set-cookies-none",
                        "set-cookies-file",
                        "set-cookies-browser",
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, k)| Chip::new(k, i18n::lookup(k, None)).selected(i == 1)),
                ),
                cx,
            ),
            row(
                SharedString::default(),
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("import-cookies")
                            .outline()
                            .label(t!("set-cookies-import")),
                    )
                    .child(
                        div()
                            .self_center()
                            .text_color(pencil)
                            .child(t!("set-cookies-firefox")),
                    ),
                cx,
            ),
            row(
                t!("set-cookies-paste"),
                div().w(px(360.)).child(cookies.clone()),
                cx,
            ),
            row(
                SharedString::default(),
                Button::new("paste-cookies")
                    .outline()
                    .label(t!("set-cookies-clipboard")),
                cx,
            ),
            Notice::new(t!("set-cookies"), t!("set-cookies-chrome")).into_any_element(),
        ],
        SettingsPage::Tools => {
            let mut rows: Vec<AnyElement> = data
                .tools
                .iter()
                .map(|tool: &ToolStatus| {
                    let note: SharedString = match tool.source {
                        Some(ToolSource::Path) => {
                            t!("set-tool-found", path = "C:\\Tools\\deno.exe")
                        }
                        Some(ToolSource::Managed) if tool.kind == ToolKind::Python => {
                            t!("set-tool-bundled")
                        }
                        _ => t!("set-tool-managed"),
                    };
                    let action = match tool.kind {
                        ToolKind::YtDlp => vec![
                            Button::new("tool-check")
                                .outline()
                                .small()
                                .label(t!("set-tool-check")),
                        ],
                        ToolKind::Ffmpeg | ToolKind::Ffprobe => vec![
                            Button::new(SharedString::from(format!("redo-{}", tool.kind)))
                                .outline()
                                .small()
                                .label(t!("set-tool-redownload")),
                            Button::new(SharedString::from(format!("choose-{}", tool.kind)))
                                .outline()
                                .small()
                                .label(t!("set-tool-choose")),
                        ],
                        ToolKind::Deno => vec![
                            Button::new("choose-deno")
                                .outline()
                                .small()
                                .label(t!("set-tool-choose")),
                        ],
                        _ => vec![],
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .min_h(px(32.))
                        .child(
                            div()
                                .w(px(110.))
                                .child(i18n::lookup(tool_key(tool.kind), None)),
                        )
                        .child(
                            div()
                                .w(px(100.))
                                .font_features(tabular())
                                .text_color(pencil)
                                .child(tool.version.clone().unwrap_or_default()),
                        )
                        .child(div().flex_1().text_color(pencil).child(note))
                        .children(action)
                        .into_any_element()
                })
                .collect();
            rows.push(Notice::new(t!("set-page-tools"), t!("set-long-paths")).into_any_element());
            rows
        }
        SettingsPage::Appearance => vec![
            row(
                t!("set-theme"),
                div().flex().gap_2().children(
                    ["set-theme-system", "set-theme-dark", "set-theme-light"]
                        .into_iter()
                        .enumerate()
                        .map(|(i, k)| Chip::new(k, i18n::lookup(k, None)).selected(i == 1)),
                ),
                cx,
            ),
            toggle("a-compact", "set-compact", false),
            row(t!("set-font-size"), value("13", false, cx), cx),
        ],
        SettingsPage::About => vec![
            row(
                t!("set-about-version"),
                div()
                    .font_features(tabular())
                    .child(env!("CARGO_PKG_VERSION")),
                cx,
            ),
            row(
                SharedString::default(),
                div().text_color(pencil).child(t!("set-about-license")),
                cx,
            ),
        ],
    };
    div()
        .flex()
        .size_full()
        .child(nav)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap_1()
                .p(theme.metrics.pad)
                .child(
                    div()
                        .mb_2()
                        .text_size(theme.text(Text::Title))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(i18n::lookup(page.key(), None)),
                )
                .children(body),
        )
        .into_any_element()
}
