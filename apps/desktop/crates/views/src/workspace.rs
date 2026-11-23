use gpui::prelude::*;
use gpui::{
    Context, Entity, FocusHandle, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Window, WindowControlArea, div,
};
use i18n::t;
use input::{OpenSettings, ShowDownloads, ShowLogs, ShowSettings, WORKSPACE_CONTEXT};
use router::{Destination, Navigation, NavigationEvent, SettingsPage};
use ui::{ActiveTheme as _, Button, Vacancy, WindowControls};

/// The shell: a title bar with the page navigation over the current page.
pub struct Workspace {
    navigation: Entity<Navigation>,
    focus: FocusHandle,
    /// Whether the left button went down on the bar and has not moved the window yet.
    grabbed: bool,
}

impl Workspace {
    pub fn new(
        navigation: Entity<Navigation>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&navigation, |_, _, _: &NavigationEvent, cx| cx.notify())
            .detach();
        Self {
            navigation,
            focus: cx.focus_handle(),
            grabbed: false,
        }
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

    fn page(current: Destination) -> Vacancy {
        let (label, icon) = match current {
            Destination::Welcome | Destination::Downloads => {
                (t!("page-downloads-empty"), "icons/download.svg")
            }
            Destination::Logs => (t!("page-logs-empty"), "icons/scroll.svg"),
            Destination::Settings(_) => (t!("page-settings-empty"), "icons/settings.svg"),
        };
        Vacancy::new(label).icon(icon).spoken("page-empty")
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let current = self.navigation.read(cx).current();
        let settings = Destination::Settings(SettingsPage::General);

        div()
            .id("workspace")
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
            .child(div().flex_1().child(Self::page(current).size_full()))
            .when(window.is_maximized(), |this| this)
    }
}
