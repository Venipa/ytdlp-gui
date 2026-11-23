use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Window, WindowControlArea, div,
};
use input::{
    DeleteEntry, DeleteFile, FocusIntake, FocusSearch, OpenSettings, PasteIntake, ShowDownloads,
    ShowLogs, ShowSettings, TogglePause, WORKSPACE_CONTEXT,
};
use router::{Destination, Navigation, NavigationEvent, SettingsPage};
use state::AppState;
use ui::{ActiveTheme as _, Button, Deselect, SelectNext, SelectPrevious, WindowControls};

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
                    router::navigate(Destination::Welcome, cx);
                    tools.update(cx, |t, cx| t.ensure_missing(cx));
                }
                (true, false) => self
                    .app
                    .store
                    .write(|db| async move { db.state_set("first_run_done", "1").await }),
                _ => {}
            }
        }
        if ready && self.navigation.read(cx).current() == Destination::Welcome {
            welcome::leave(cx);
        }
        cx.notify();
    }

    fn entry(
        &self,
        id: &'static str,
        key: &'static str,
        to: Destination,
        current: Destination,
    ) -> Button {
        Button::new(id)
            .ghost()
            .label(i18n::lookup(key, None))
            .selected(to.same_section(&current))
            .on_click(move |_, _, cx| router::navigate(to, cx))
    }

    fn page(&self, current: Destination, cx: &mut Context<Self>) -> AnyElement {
        match current {
            Destination::Welcome => welcome::welcome(cx),
            Destination::Downloads => self.downloads.clone().into_any_element(),
            Destination::Logs => self.logs.clone().into_any_element(),
            Destination::Settings(_) => self.settings.clone().into_any_element(),
        }
    }

    /// Forwards a key of the Downloads page.
    fn key(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation.read(cx).current() == Destination::Downloads {
            self.downloads.update(cx, |d, cx| d.key(key, window, cx));
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let current = self.navigation.read(cx).current();
        let settings = Destination::Settings(SettingsPage::General);

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
                            .items_center()
                            .gap_1()
                            .px_3()
                            .child(self.entry(
                                "nav-downloads",
                                "nav-downloads",
                                Destination::Downloads,
                                current,
                            ))
                            .child(self.entry("nav-logs", "nav-logs", Destination::Logs, current))
                            .child(self.entry("nav-settings", "nav-settings", settings, current)),
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
