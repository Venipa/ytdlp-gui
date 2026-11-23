use std::time::Duration;

use domain::{ToolKind, ToolSource, ToolState, ToolStatus};
use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, ElementId, Entity, FocusHandle, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString, Window, WindowControlArea, div, px,
    svg,
};
use i18n::t;
use input::{
    DeleteEntry, DeleteFile, FirstRow, FocusIntake, FocusSearch, LastRow, OpenSettings,
    PasteIntake, ShowDownloads, ShowLogs, ShowSettings, TogglePause, WORKSPACE_CONTEXT,
};
use router::{Destination, Navigation, NavigationEvent, SettingsPage};
use state::AppState;
use ui::{
    Activate, ActiveTheme as _, Deselect, NextField, Perch, PreviousField, ProgressRule, Rule,
    SelectNext, SelectPrevious, Tipped as _, WindowControls,
};

use crate::downloads::{Downloads, Key};
use crate::logs::LogsView;
use crate::screens::settings::{SettingsView, page_icon, tool_key};
use crate::toasts::ToastLayer;
use crate::welcome;

/// The sidebar's width, and the icon rail's it folds to in a narrow window.
const SIDEBAR: f32 = 180.;
const RAIL: f32 = 48.;
/// Below this window width the sidebar folds to the rail.
const NARROW: f32 = 900.;
/// The tools the sidebar's foot reports on, in order.
const TOOLS: [ToolKind; 4] = [
    ToolKind::YtDlp,
    ToolKind::Ffmpeg,
    ToolKind::Ffprobe,
    ToolKind::Deno,
];

/// What the sidebar says about one tool.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mark {
    Ready,
    Missing,
    /// Downloading or updating, with how far along when that is known.
    Busy(Option<f32>),
    Checking,
}

/// Reads a tool's mark from its state; `updating` is an update of yt-dlp under way.
fn mark(tool: &ToolStatus, updating: bool) -> Mark {
    match &tool.state {
        ToolState::Downloading { done, total } => {
            Mark::Busy(total.map(|total| (*done as f32 / total.max(1) as f32).min(1.)))
        }
        _ if updating && tool.kind == ToolKind::YtDlp => Mark::Busy(None),
        ToolState::Ready => Mark::Ready,
        ToolState::Missing | ToolState::Failed(_) => Mark::Missing,
        ToolState::Checking => Mark::Checking,
    }
}

/// The shell: a title bar with the app mark and the window controls over the sidebar and the
/// current page.
pub struct Workspace {
    navigation: Entity<Navigation>,
    focus: FocusHandle,
    downloads: Entity<Downloads>,
    logs: Entity<LogsView>,
    toasts: Entity<ToastLayer>,
    settings: Entity<SettingsView>,
    app: AppState,
    /// Whether the first run was finished in an earlier session; `None` until read.
    first_run_done: Option<bool>,
    /// A `--screen` debug scene is open: first run does not route away from it.
    pinned: bool,
    /// Whether the start-up tool check has been acted on.
    routed: bool,
    /// Whether the left button went down on the bar and has not moved the window yet.
    grabbed: bool,
    _focus_lost: gpui::Subscription,
}

impl Workspace {
    /// Builds the shell. `scene` names a pure UI state for screenshots: `welcome`, `logs` or
    /// `settings-network`; data scenes come from `--demo`.
    pub fn new(
        navigation: Entity<Navigation>,
        scene: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let app = AppState::global(cx);
        cx.subscribe(&navigation, |_, _, _: &NavigationEvent, cx| cx.notify())
            .detach();
        let toasts = ToastLayer::init(cx);
        let downloads = cx.new(|cx| Downloads::new(window, cx));
        let logs = cx.new(LogsView::new);
        let settings = cx.new(SettingsView::new);
        match scene.unwrap_or("downloads") {
            "welcome" => router::navigate(Destination::Welcome, cx),
            "logs" => router::navigate(Destination::Logs, cx),
            other if other.starts_with("update-") => {
                if let Some(state) = crate::update_line::scene(other) {
                    app.updates.update(cx, |u, cx| u.show(state, cx));
                }
                if other == "update-page" {
                    router::navigate(Destination::Settings(SettingsPage::Updates), cx);
                }
            }
            other if !other.starts_with("settings-") => {
                downloads.update(cx, |d, cx| d.show_scene(other, cx))
            }
            other => {
                if let Some(id) = other.strip_prefix("settings-") {
                    router::navigate(
                        Destination::Settings(
                            SettingsPage::from_id(id).unwrap_or(SettingsPage::General),
                        ),
                        cx,
                    );
                }
            }
        }
        cx.observe(&app.tools, |this, _, cx| this.route(cx))
            .detach();
        cx.observe(&app.downloads, |_, _, cx| cx.notify()).detach();
        // the sidebar's update line follows the updater
        cx.observe(&app.updates, |_, _, cx| cx.notify()).detach();
        // the sidebar's tools block follows every tool state, not only what routing acts on
        cx.observe(&app.tools, |_, _, cx| cx.notify()).detach();
        // and the yt-dlp version Settings asks the engine for
        cx.observe(&settings, |_, _, cx| cx.notify()).detach();
        let read = app
            .store
            .read(|db| async move { db.state_get("first_run_done").await });
        cx.spawn(async move |this, cx| {
            let done = read.await.ok().flatten().is_some();
            this.update(cx, |this, cx| {
                this.first_run_done = Some(done);
                this.route(cx);
            })
            .ok();
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // a focused part that leaves (a toast timing out under the keyboard) leaves nothing
        // focused, and keys then reach no handler: focus falls back to the closest part still
        // shown, else the workspace, so Tab carries on
        let focus_lost = cx.on_focus_lost(window, |this: &mut Self, window, cx| {
            let to = window
                .focus_lost_restore_target(cx)
                .unwrap_or_else(|| this.focus.clone());
            window.focus(&to, cx);
        });
        Self {
            _focus_lost: focus_lost,
            navigation,
            focus,
            downloads,
            logs,
            toasts,
            settings,
            app,
            first_run_done: None,
            pinned: scene.is_some(),
            routed: false,
            grabbed: false,
        }
    }

    /// Getting ready: the first run shows it and installs what is missing; it leaves by itself when
    /// every tool is ready. Later runs show the banner on Downloads instead.
    fn route(&mut self, cx: &mut Context<Self>) {
        let Some(first_run_done) = self.first_run_done else {
            return;
        };
        let tools = self.app.tools.clone();
        let (detected, ready) = {
            let t = tools.read(cx);
            (t.detected(), t.all_ready())
        };
        if !detected {
            return;
        }
        if !self.routed {
            log::info!(
                "views: tools detected, ready={ready}, first run done={first_run_done}, tools={:?}",
                tools
                    .read(cx)
                    .all()
                    .iter()
                    .map(|s| (s.kind, s.state.clone()))
                    .collect::<Vec<_>>()
            );
        }
        if !std::mem::replace(&mut self.routed, true) {
            match (ready, first_run_done) {
                (false, false) => {
                    if !self.pinned {
                        router::navigate(Destination::Welcome, cx);
                    }
                    tools.update(cx, |t, cx| t.ensure_missing(cx));
                }
                (true, false) => self
                    .app
                    .store
                    .write(|db| async move { db.state_set("first_run_done", "1").await }),
                _ => {}
            }
        }
        if ready && !self.pinned && self.navigation.read(cx).current() == Destination::Welcome {
            welcome::leave(cx);
        }
        cx.notify();
    }

    /// The left sidebar: Downloads with its count of unfinished jobs, Logs, and Settings with its
    /// pages nested under it while Settings is open. Below `NARROW` it folds to an icon rail
    /// whose items name themselves in tooltips, and the open Settings group lists its pages there
    /// as icons too.
    fn sidebar(
        &self,
        current: Destination,
        unfinished: usize,
        rail: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        // Getting ready is the first state of Downloads, so its entry stays lit there
        let lit = |to: Destination| {
            to.same_section(&current)
                || (current == Destination::Welcome && to == Destination::Downloads)
        };
        let entry = |key: &'static str, icon: &'static str, tip: &'static str, to: Destination| {
            ui::nav_item(key, i18n::lookup(key, None), icon, tip, lit(to), rail, cx)
        };
        let label = i18n::lookup("nav-downloads", None);
        let downloads = entry(
            "nav-downloads",
            "icons/download.svg",
            "nav-downloads-tip",
            Destination::Downloads,
        )
        .when(unfinished > 0, |row| match rail {
            true => row.name(format!("{label} {unfinished}")),
            false => row.detail(ui::grouped(unfinished)),
        })
        .on_click(|_, _, cx| router::navigate(Destination::Downloads, cx));
        let logs = entry(
            "nav-logs",
            "icons/list.svg",
            "nav-logs-tip",
            Destination::Logs,
        )
        .on_click(|_, _, cx| router::navigate(Destination::Logs, cx));
        let open = match current {
            Destination::Settings(page) => Some(page),
            _ => None,
        };
        // while Settings is open its page carries the pill, so the group row only brightens
        let settings = ui::nav_item(
            "nav-settings",
            i18n::lookup("nav-settings", None),
            "icons/settings.svg",
            "nav-settings-tip",
            false,
            rail,
            cx,
        )
        .when(open.is_some(), |row| row.tint(theme.foreground))
        .when(!rail, |row| {
            row.trailing(match open {
                Some(_) => "icons/chevron-down.svg",
                None => "icons/chevron-right.svg",
            })
            .expanded(open.is_some())
        })
        .on_click(|_, _, cx| router::open_settings(cx));
        let pages = open.map(|open| {
            let rows = SettingsPage::ALL.iter().enumerate().map(|(index, page)| {
                let page = *page;
                let key = page.key();
                ui::nav_item(
                    ElementId::from(SharedString::from(format!("nav-settings-{}", page.id()))),
                    i18n::lookup(key, None),
                    page_icon(page),
                    key,
                    page == open,
                    rail,
                    cx,
                )
                .aria_position_in_set(index + 1)
                .aria_size_of_set(SettingsPage::ALL.len())
                .on_click(move |_, _, cx| router::navigate(Destination::Settings(page), cx))
            });
            div()
                .id("nav-settings-pages")
                .role(Role::Group)
                .aria_label(i18n::lookup("nav-settings", None))
                .flex()
                .flex_col()
                .gap(px(2.))
                .map(|this| match rail {
                    // on the rail a hairline sets the pages off from the entries above
                    true => this.pt(px(4.)).border_t_1().border_color(theme.border),
                    // a guide line under the Settings icon carries the pages, as in maple
                    false => this
                        .ml(theme.metrics.pad + px(7.))
                        .pl(px(6.))
                        .border_l_1()
                        .border_color(theme.border),
                })
                .children(rows)
        });
        div()
            .id("sidebar")
            .role(Role::Navigation)
            .aria_label(i18n::lookup("nav-sidebar", None))
            .flex()
            .flex_col()
            .flex_none()
            .w(px(if rail { RAIL } else { SIDEBAR }))
            .gap(px(2.))
            .p(theme.metrics.pad)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(downloads)
            .child(logs)
            .child(settings)
            .children(pages)
            .child(div().flex_1())
            .children(crate::update_line::update_line(&self.app, rail, window, cx))
            .child(self.tools_block(rail, window, cx))
            .into_any_element()
    }

    /// The tools at the sidebar's foot: one line per tool with its short version in mono and a
    /// status glyph, a hairline of progress while one downloads, and the full versions and
    /// sources in a tooltip. The whole block is one control that opens Settings > Tools; on the
    /// rail it is a wrench with a dot, amber while a tool is busy and red while one is missing.
    fn tools_block(&self, rail: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let tools = self.app.tools.read(cx);
        let updating = tools.updating();
        let ytdlp = self.settings.read(cx).ytdlp_version().map(str::to_string);
        let shown: Vec<(ToolStatus, Mark)> = TOOLS
            .iter()
            .filter_map(|kind| match (tools.get(*kind), kind) {
                (Some(tool), _) => Some(tool.clone()),
                // the bundled yt-dlp is not detected: the engine reports its version to Settings
                (None, ToolKind::YtDlp) => Some(ToolStatus {
                    kind: ToolKind::YtDlp,
                    state: match ytdlp {
                        Some(_) => ToolState::Ready,
                        None => ToolState::Checking,
                    },
                    source: None,
                    path: None,
                    version: ytdlp.clone(),
                }),
                (None, _) => None,
            })
            .map(|tool| {
                let mark = mark(&tool, updating);
                (tool, mark)
            })
            .collect();
        let name = |tool: &ToolStatus| i18n::lookup(tool_key(tool.kind), None);
        let state = |mark: Mark| {
            i18n::lookup(
                match mark {
                    Mark::Ready => "tools-ready",
                    Mark::Missing => "tools-missing",
                    Mark::Busy(_) => "tools-updating",
                    Mark::Checking => "tools-checking",
                },
                None,
            )
        };
        let tip = shown
            .iter()
            .map(|(tool, mark)| {
                let source = match (tool.source, tool.kind) {
                    (Some(ToolSource::Path), _) => "tools-source-path",
                    (Some(ToolSource::Custom), _) => "tools-source-custom",
                    (_, ToolKind::YtDlp) => "set-tool-bundled",
                    _ => "tools-source-managed",
                };
                match (mark, tool.version.as_deref()) {
                    (Mark::Ready, Some(version)) => format!(
                        "{}  {version}  \u{b7}  {}",
                        name(tool),
                        i18n::lookup(source, None)
                    ),
                    // a tool that is not there has no version or source to name
                    _ => format!("{}  {}", name(tool), state(*mark)),
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let summary = shown
            .iter()
            .map(|(tool, mark)| match tool.version.as_deref() {
                Some(version) => format!(
                    "{} {} {}",
                    name(tool),
                    welcome::short_version(version),
                    state(*mark)
                ),
                None => format!("{} {}", name(tool), state(*mark)),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let dot = match (
            shown.iter().any(|(_, m)| *m == Mark::Missing),
            shown.iter().any(|(_, m)| matches!(m, Mark::Busy(_))),
        ) {
            (true, _) => Some(theme.danger),
            (false, true) => Some(theme.progress_bar),
            _ => None,
        };
        let lines: Vec<AnyElement> = match rail {
            true => Vec::new(),
            false => shown
                .iter()
                .map(|(tool, mark)| {
                    let (glyph, color) = match mark {
                        Mark::Ready => ("icons/check.svg", theme.muted_foreground),
                        Mark::Missing => ("icons/circle-alert.svg", theme.danger),
                        Mark::Busy(_) => ("icons/refresh-cw.svg", theme.progress_bar),
                        Mark::Checking => ("icons/loader-circle.svg", theme.muted_foreground),
                    };
                    let version = match (mark, tool.version.as_deref()) {
                        (Mark::Ready, Some(version)) => {
                            SharedString::from(welcome::short_version(version).to_string())
                        }
                        _ => state(*mark),
                    };
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .h(px(18.))
                                .child(div().flex_1().min_w_0().truncate().child(name(tool)))
                                .child(
                                    div()
                                        .flex_none()
                                        .max_w(px(72.))
                                        .truncate()
                                        .font_family(ui::MONO)
                                        .text_size(theme.text(ui::Text::Tiny))
                                        .child(version),
                                )
                                .child(
                                    svg()
                                        .path(icons::path(glyph))
                                        .size(px(12.))
                                        .flex_none()
                                        .text_color(color),
                                ),
                        )
                        .children(match mark {
                            Mark::Busy(fraction) => Some(ProgressRule::new(match fraction {
                                Some(fraction) => Rule::Fill(*fraction),
                                None => Rule::Pulse,
                            })),
                            _ => None,
                        })
                        .into_any_element()
                })
                .collect(),
        };
        let id = ElementId::from("tools-status");
        let block = ui::focusable(div().id(id.clone()), &id, window, cx)
            .role(Role::Button)
            .aria_label(t!("tools-block-label", summary = summary))
            .flex()
            .flex_col()
            .flex_none()
            .rounded(theme.radius)
            .cursor_pointer()
            .hover(|style| style.bg(theme.table_hover))
            .active(|style| style.bg(theme.secondary_hover))
            .tip_label(tip, Perch::Trailing)
            .on_click(|_, _, cx| router::navigate(Destination::Settings(SettingsPage::Tools), cx))
            .map(|this| match rail {
                true => this
                    .h(theme.metrics.control + px(4.))
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .relative()
                            .child(
                                svg()
                                    .path(icons::path("icons/wrench.svg"))
                                    .size(px(16.))
                                    .text_color(theme.muted_foreground),
                            )
                            .children(dot.map(|color| {
                                div()
                                    .absolute()
                                    .top(px(-2.))
                                    .right(px(-3.))
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(color)
                            })),
                    ),
                false => this
                    .gap(px(2.))
                    .px(theme.metrics.pad)
                    .py(px(6.))
                    .text_size(theme.text(ui::Text::Small))
                    .text_color(theme.muted_foreground)
                    .children(lines),
            });
        ui::motion::pressing(block, &id, ui::motion::PRESS_SCALE, window, cx).into_any_element()
    }

    fn page(&self, current: Destination, cx: &mut Context<Self>) -> AnyElement {
        let (n, page): (usize, AnyElement) = match current {
            Destination::Welcome => (0, welcome::welcome(cx)),
            Destination::Downloads => (1, self.downloads.clone().into_any_element()),
            Destination::Logs => (2, self.logs.clone().into_any_element()),
            Destination::Settings(_) => (3, self.settings.clone().into_any_element()),
        };
        // a page fades in over 120 ms when the section changes; a new id restarts the run
        let span = match ui::motion::animates(cx) {
            true => Duration::from_millis(120),
            false => Duration::from_millis(1),
        };
        div()
            .size_full()
            .child(page)
            .with_animation(
                ("page", n),
                Animation::new(span).with_easing(ui::ease_out_cubic),
                |this, delta| this.opacity(delta),
            )
            .into_any_element()
    }

    /// Forwards a key of the Downloads page.
    fn key(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation.read(cx).current() == Destination::Downloads {
            self.downloads.update(cx, |d, cx| d.key(key, window, cx));
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let current = self.navigation.read(cx).current();
        let rail = window.viewport_size().width < px(NARROW);
        let unfinished = self
            .app
            .downloads
            .read(cx)
            .count(|status| !status.is_terminal());

        div()
            .id("workspace")
            .relative()
            .key_context(WORKSPACE_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &ShowDownloads, _, cx| {
                router::navigate(Destination::Downloads, cx)
            }))
            .on_action(
                cx.listener(|_, _: &ShowLogs, _, cx| router::navigate(Destination::Logs, cx)),
            )
            .on_action(cx.listener(|_, _: &ShowSettings, _, cx| router::open_settings(cx)))
            .on_action(cx.listener(|_, _: &OpenSettings, _, cx| router::open_settings(cx)))
            .on_action(cx.listener(|this, _: &SelectNext, w, cx| this.key(Key::Next, w, cx)))
            .on_action(
                cx.listener(|this, _: &SelectPrevious, w, cx| this.key(Key::Previous, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &TogglePause, w, cx| this.key(Key::TogglePause, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &DeleteEntry, w, cx| this.key(Key::DeleteEntry, w, cx)),
            )
            .on_action(cx.listener(|this, _: &DeleteFile, w, cx| this.key(Key::DeleteFile, w, cx)))
            .on_action(cx.listener(|this, _: &Deselect, w, cx| this.key(Key::Deselect, w, cx)))
            .on_action(cx.listener(|_, _: &NextField, w, cx| w.focus_next(cx)))
            .on_action(cx.listener(|_, _: &PreviousField, w, cx| w.focus_prev(cx)))
            .on_action(cx.listener(|this, _: &FirstRow, w, cx| this.key(Key::First, w, cx)))
            .on_action(cx.listener(|this, _: &LastRow, w, cx| this.key(Key::Last, w, cx)))
            .on_action(cx.listener(|this, _: &Activate, w, cx| this.key(Key::Open, w, cx)))
            .on_action(cx.listener(|this, _: &PasteIntake, w, cx| this.key(Key::Paste, w, cx)))
            .on_action(
                cx.listener(|this, _: &FocusIntake, w, cx| this.key(Key::FocusIntake, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FocusSearch, w, cx| this.key(Key::FocusSearch, w, cx)),
            )
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(theme.text(ui::Text::Body))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .relative()
                    .items_center()
                    .justify_end()
                    .h(theme.metrics.title_bar)
                    .border_b_1()
                    .border_color(theme.title_bar_border)
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, _| {
                            match event.click_count {
                                1 => this.grabbed = true,
                                2 => window.zoom_window(),
                                _ => {}
                            }
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, _| this.grabbed = false),
                    )
                    .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, window, _| {
                        if std::mem::take(&mut this.grabbed) {
                            window.start_window_move();
                        }
                    }))
                    .child(
                        // centred over the whole bar, not the space left of the window controls
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .size(px(20.))
                                            .rounded(theme.radius)
                                            .bg(theme.primary)
                                            .text_color(theme.primary_foreground)
                                            .child(
                                                svg()
                                                    .path(icons::path("icons/download.svg"))
                                                    .size(px(12.))
                                                    .text_color(theme.primary_foreground),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .font_family(ui::MONO)
                                            .text_size(theme.text(ui::Text::Label))
                                            .text_color(theme.muted_foreground)
                                            .child(i18n::lookup("app-wordmark", None)),
                                    ),
                            ),
                    )
                    // macOS draws its own traffic lights (placed in main.rs); ours would be a second set
                    .when(!cfg!(target_os = "macos"), |bar| {
                        bar.child(
                            div()
                                .flex_none()
                                .h_full()
                                .self_stretch()
                                .child(WindowControls::new(false)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(current, unfinished, rail, window, cx))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .child(self.page(current, cx))
                            // toasts lie over the page's bottom right; Downloads hangs them above
                            // its status bar itself
                            .when(current != Destination::Downloads, |this| {
                                this.child(self.toasts.clone())
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use domain::{EngineError, ErrorCode, ToolKind, ToolState, ToolStatus};

    use super::{Mark, mark};

    fn tool(kind: ToolKind, state: ToolState) -> ToolStatus {
        ToolStatus {
            kind,
            state,
            source: None,
            path: None,
            version: None,
        }
    }

    #[test]
    fn a_tool_reads_as_ready_missing_or_busy() {
        assert_eq!(
            mark(&tool(ToolKind::Ffmpeg, ToolState::Ready), false),
            Mark::Ready
        );
        let failed = ToolState::Failed(EngineError::new(ErrorCode::Network, "reset"));
        assert_eq!(mark(&tool(ToolKind::Deno, failed), false), Mark::Missing);
        let half = ToolState::Downloading {
            done: 50,
            total: Some(100),
        };
        assert_eq!(
            mark(&tool(ToolKind::Ffprobe, half), false),
            Mark::Busy(Some(0.5))
        );
        // an update of yt-dlp marks yt-dlp busy and leaves the others be
        assert_eq!(
            mark(&tool(ToolKind::YtDlp, ToolState::Ready), true),
            Mark::Busy(None)
        );
        assert_eq!(
            mark(&tool(ToolKind::Ffmpeg, ToolState::Ready), true),
            Mark::Ready
        );
    }
}
