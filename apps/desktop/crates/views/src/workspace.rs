use std::time::Duration;

use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, ElementId, Entity, FocusHandle, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Window, WindowControlArea, div, px, svg,
};
use input::{
    DeleteEntry, DeleteFile, FirstRow, FocusIntake, FocusSearch, LastRow, OpenSettings,
    PasteIntake, ShowDownloads, ShowLogs, ShowSettings, TogglePause, WORKSPACE_CONTEXT,
};
use router::{Destination, Navigation, NavigationEvent, SettingsPage};
use state::AppState;
use ui::{
    Activate, ActiveTheme as _, Deselect, NextField, Perch, PreviousField, SelectNext,
    SelectPrevious, Tipped as _, WindowControls,
};

use crate::downloads::{Downloads, Key};
use crate::logs::LogsView;
use crate::screens::settings::SettingsView;
use crate::toasts::ToastLayer;
use crate::welcome;

/// The shell: a title bar with the page navigation over the current page.
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
        Self {
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

    /// One page tab of the bar: icon and label, muted until hovered or open; the open page has an
    /// ink rule on the bar's bottom edge, and Downloads carries the amber count of unfinished jobs.
    #[allow(clippy::too_many_arguments)]
    fn entry(
        &self,
        key: &'static str,
        icon: &'static str,
        to: Destination,
        current: Destination,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let on = to.same_section(&current);
        let id = ElementId::from(key);
        let label = i18n::lookup(key, None);
        let tip = match key {
            "nav-downloads" => "nav-downloads-tip",
            "nav-logs" => "nav-logs-tip",
            _ => "nav-settings-tip",
        };
        let tint = match on {
            true => theme.foreground,
            false => theme.muted_foreground,
        };
        let tab = ui::focusable(div().id(id.clone()), &id, window, cx)
            .role(Role::Tab)
            .aria_label(match count {
                0 => label.clone(),
                n => format!("{label} {n}").into(),
            })
            .aria_selected(on)
            .group(key)
            .flex()
            .items_center()
            .gap(px(6.))
            .h(theme.metrics.control)
            .px(px(10.))
            .rounded(theme.radius)
            .cursor_pointer()
            .text_color(tint)
            .hover(|style| style.bg(theme.secondary_hover).text_color(theme.foreground))
            .active(|style| style.bg(theme.secondary_active))
            .tip(tip, Perch::Below)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| router::navigate(to, cx))
            .child(
                svg()
                    .path(icons::path(icon))
                    .size(px(16.))
                    .flex_none()
                    .text_color(tint)
                    .group_hover(key, |style| style.text_color(theme.foreground)),
            )
            .child(label)
            .when(count > 0, |this| {
                this.child(
                    div()
                        .h(px(16.))
                        .min_w(px(16.))
                        .px(px(5.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(theme.progress_bar.opacity(0.16))
                        .text_color(theme.progress_bar)
                        .text_size(theme.text(ui::Text::Tiny))
                        .font_features(ui::tabular())
                        .child(count.to_string()),
                )
            });
        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .h_full()
            .child(tab)
            .when(on, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .right(px(10.))
                        .bottom(px(-1.))
                        .h(px(2.))
                        .bg(theme.table_active_border),
                )
            })
            .into_any_element()
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
        let settings = Destination::Settings(SettingsPage::General);
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
                    .items_center()
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
                        div()
                            .flex()
                            .flex_1()
                            .h_full()
                            .items_center()
                            .gap(px(2.))
                            .px(theme.metrics.gutter)
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .gap(px(8.))
                                    .mr(theme.metrics.gutter)
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
                            )
                            .child(self.entry(
                                "nav-downloads",
                                "icons/download.svg",
                                Destination::Downloads,
                                current,
                                unfinished,
                                window,
                                cx,
                            ))
                            .child(self.entry(
                                "nav-logs",
                                "icons/list.svg",
                                Destination::Logs,
                                current,
                                0,
                                window,
                                cx,
                            ))
                            .child(self.entry(
                                "nav-settings",
                                "icons/settings.svg",
                                settings,
                                current,
                                0,
                                window,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .h_full()
                            .self_stretch()
                            .child(WindowControls::new(false)),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.page(current, cx)))
            .child(self.toasts.clone())
    }
}
