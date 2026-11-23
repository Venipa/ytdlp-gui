//! The update line in the sidebar's foot, above the tools block (owner picks 1A 2A 3A 6): what
//! the updater found, its download as a ruled progress line, Relaunch to update with Install
//! when I quit, and failures with Try again. On the icon rail it folds to an icon with a dot
//! that opens Settings > Updates.
use domain::UpdateChannel;
use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{AnyElement, App, ElementId, Entity, SharedString, Window, div, px, svg};
use i18n::t;
use router::{Destination, SettingsPage};
use state::{AppState, Release, UpdateError, UpdateState, Updates};
use ui::{ActiveTheme as _, Button, Checkbox, Perch, ProgressRule, Rule, Tipped as _};

use crate::format;

/// The `--screen update-*` states for screenshots (stand-in release 0.2.0, 31 MB).
pub(crate) fn scene(name: &str) -> Option<UpdateState> {
    let release = Release {
        version: "0.2.0".into(),
        page: format!(
            "https://github.com/{}/releases/tag/v0.2.0",
            state::updates::REPO
        ),
        asset: Some(state::updates::Asset {
            name: "ytdlpd-0.2.0-windows-x64-setup.exe".into(),
            browser_download_url: String::new(),
            size: 31 << 20,
        }),
        sums: None,
    };
    Some(match name {
        "update-available" | "update-page" => UpdateState::Available(release),
        "update-downloading" => UpdateState::Downloading {
            release,
            done: 12 << 20,
            total: Some(31 << 20),
        },
        "update-ready" => UpdateState::Ready(release),
        "update-error" => UpdateState::Failed {
            error: UpdateError::Download { at: 12 << 20 },
            release: Some(release),
        },
        "update-checksum" => UpdateState::Failed {
            error: UpdateError::Checksum,
            release: Some(release),
        },
        "update-offline" => UpdateState::Failed {
            error: UpdateError::Offline,
            release: None,
        },
        _ => return None,
    })
}

/// The title and detail an update state shows, shared with Settings > Updates.
pub(crate) fn words(
    state: &UpdateState,
    channel: UpdateChannel,
    running: usize,
) -> Option<(SharedString, Option<SharedString>)> {
    let size = |r: &Release| r.size().map(|s| format::bytes(s as i64));
    Some(match state {
        UpdateState::Idle | UpdateState::Checking => return None,
        UpdateState::Available(r) => (
            t!("update-available", version = r.version.clone()),
            Some(match (channel, size(r)) {
                (UpdateChannel::Stable, Some(size)) => t!("update-stable-size", size = size),
                (UpdateChannel::Beta, Some(size)) => t!("update-beta-size", size = size),
                (UpdateChannel::Stable, None) => t!("update-stable"),
                (UpdateChannel::Beta, None) => t!("update-beta"),
            }),
        ),
        UpdateState::Downloading {
            release,
            done,
            total,
        } => (
            t!("update-downloading", version = release.version.clone()),
            Some(match total {
                Some(total) => t!(
                    "update-progress",
                    done = format::bytes(*done as i64),
                    total = format::bytes(*total as i64)
                ),
                None => format::bytes(*done as i64).into(),
            }),
        ),
        UpdateState::Ready(r) => (
            t!("update-ready", version = r.version.clone()),
            (running > 0).then(|| t!("update-will-pause", count = running as i64)),
        ),
        UpdateState::Installing(r) => (t!("update-installing", version = r.version.clone()), None),
        UpdateState::Failed { error, .. } => match error {
            UpdateError::Offline => (
                t!("update-err-offline"),
                Some(t!("update-err-offline-detail")),
            ),
            UpdateError::Check => (
                t!("update-err-offline"),
                Some(t!("update-err-check-detail")),
            ),
            UpdateError::Download { at } => (
                t!("update-err-download"),
                Some(t!(
                    "update-err-download-detail",
                    at = format::bytes(*at as i64)
                )),
            ),
            UpdateError::Checksum => (
                t!("update-err-checksum"),
                Some(t!("update-err-checksum-detail")),
            ),
            UpdateError::Install => (
                t!("update-err-install"),
                Some(t!("update-err-install-detail")),
            ),
        },
    })
}

/// The action buttons of a state (shared with Settings > Updates); `prefix` keeps ids unique.
pub(crate) fn actions(
    prefix: &'static str,
    updates: &Entity<Updates>,
    cx: &App,
) -> Vec<AnyElement> {
    let u = updates.read(cx);
    let id = |name: &str| ElementId::from(SharedString::from(format!("{prefix}-{name}")));
    let act =
        |name: &str, label: SharedString, f: fn(&mut Updates, &mut gpui::Context<Updates>)| {
            let updates = updates.clone();
            Button::new(id(name))
                .small()
                .label(label)
                .on_click(move |_, _, cx| updates.update(cx, f))
        };
    let notes = |r: &Release| {
        let page = r.page.clone();
        Button::new(id("notes"))
            .small()
            .ghost()
            .tint(cx.theme().primary)
            // the sidebar is narrow: a short label there, the full one on the page
            .label(match prefix {
                "update-line" => t!("update-notes"),
                _ => t!("update-notes-github"),
            })
            .on_click(move |_, _, cx| cx.open_url(&page))
            .into_any_element()
    };
    match u.state() {
        UpdateState::Available(r) => vec![
            act("download", t!("update-download"), Updates::download)
                .primary()
                .into_any_element(),
            notes(r),
        ],
        UpdateState::Ready(_) => {
            let on = u.install_on_quit();
            let toggle = updates.clone();
            vec![
                act("relaunch", t!("update-relaunch"), Updates::relaunch)
                    .primary()
                    .into_any_element(),
                Checkbox::new(id("on-quit"), on)
                    .label(t!("update-on-quit"))
                    .on_click(move |_, _, cx| {
                        toggle.update(cx, |u, cx| u.toggle_install_on_quit(cx))
                    })
                    .into_any_element(),
            ]
        }
        UpdateState::Failed { .. } => vec![
            act("retry", t!("update-try-again"), Updates::retry)
                .outline()
                .into_any_element(),
        ],
        _ => Vec::new(),
    }
}

/// The line, or `None` while there is nothing to say (up to date, checking).
pub(crate) fn update_line(
    app: &AppState,
    rail: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let theme = *cx.theme();
    let state = app.updates.read(cx).state().clone();
    let channel = app.settings.read(cx).get().update_channel;
    let running = app.downloads.read(cx).count(|s| s.is_active());
    let (title, detail) = words(&state, channel, running)?;
    let failed = matches!(state, UpdateState::Failed { .. });
    if rail {
        let dot = match state {
            UpdateState::Failed { .. } => theme.danger,
            UpdateState::Downloading { .. } | UpdateState::Installing(_) => theme.progress_bar,
            _ => theme.primary,
        };
        let tip = match &detail {
            Some(detail) => format!("{title}\n{detail}"),
            None => title.to_string(),
        };
        let id = ElementId::from("update-line");
        let icon = ui::focusable(div().id(id.clone()), &id, window, cx)
            .role(Role::Button)
            .aria_label(t!("update-line-label", state = title.to_string()))
            .flex()
            .flex_none()
            .h(theme.metrics.control + px(4.))
            .items_center()
            .justify_center()
            .rounded(theme.radius)
            .cursor_pointer()
            .hover(|style| style.bg(theme.table_hover))
            .tip_label(tip, Perch::Trailing)
            .on_click(|_, _, cx| router::navigate(Destination::Settings(SettingsPage::Updates), cx))
            .child(
                div()
                    .relative()
                    .child(
                        svg()
                            .path(icons::path("icons/circle-arrow-down.svg"))
                            .size(px(16.))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(-2.))
                            .right(px(-3.))
                            .size(px(6.))
                            .rounded_full()
                            .bg(dot),
                    ),
            );
        return Some(icon.into_any_element());
    }
    let rule = match &state {
        UpdateState::Downloading { done, total, .. } => Some(match total {
            Some(total) => Rule::Fill(*done as f32 / (*total).max(1) as f32),
            None => Rule::Pulse,
        }),
        UpdateState::Installing(_) => Some(Rule::Pulse),
        _ => None,
    };
    let buttons = actions("update-line", &app.updates, cx);
    Some(
        div()
            .id("update-line")
            .role(Role::Group)
            .aria_label(t!("update-line-label", state = title.to_string()))
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(2.))
            .p(px(8.))
            .mb(px(6.))
            .rounded(theme.radius)
            .border_1()
            .border_color(match failed {
                true => theme.danger,
                false => theme.border,
            })
            .bg(theme.secondary)
            .text_size(theme.text(ui::Text::Small))
            .child(div().font_weight(gpui::FontWeight::MEDIUM).child(title))
            .children(detail.map(|detail| {
                div()
                    .text_size(theme.text(ui::Text::Tiny))
                    .text_color(theme.muted_foreground)
                    .child(detail)
            }))
            .children(rule.map(|rule| div().pt(px(6.)).child(ProgressRule::new(rule))))
            .when(!buttons.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.))
                        .pt(px(6.))
                        .children(buttons),
                )
            })
            .into_any_element(),
    )
}
