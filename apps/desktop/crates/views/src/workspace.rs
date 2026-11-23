use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Window, WindowControlArea, div,
};
use input::{OpenSettings, ShowDownloads, ShowLogs, ShowSettings, WORKSPACE_CONTEXT};
use router::{Destination, Navigation, NavigationEvent, SettingsPage};
use ui::{ActiveTheme as _, Button, WindowControls};

use crate::downloads::Downloads;
use crate::pages;
use crate::sample::Data;
use crate::screens::settings::SettingsView;

/// The shell: a title bar with the page navigation over the current page.
pub struct Workspace {
    navigation: Entity<Navigation>,
    focus: FocusHandle,
    data: Rc<Data>,
    downloads: Entity<Downloads>,
    settings: Entity<SettingsView>,
    /// Whether the left button went down on the bar and has not moved the window yet.
    grabbed: bool,
}

impl Workspace {
    /// Builds the shell. `scene` names a screenshot state such as `card` or `settings-network`.
    pub fn new(
        navigation: Entity<Navigation>,
        scene: Option<&str>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&navigation, |_, _, _: &NavigationEvent, cx| cx.notify())
            .detach();
        let data = Rc::new(Data::sample());
        let downloads = cx.new(|cx| Downloads::new(data.clone(), cx));
        let settings = cx.new(SettingsView::new);
        match scene.unwrap_or("downloads") {
            "welcome" => router::navigate(Destination::Welcome, cx),
            "logs" => router::navigate(Destination::Logs, cx),
            other => match other.strip_prefix("settings-") {
                Some(id) => router::navigate(
                    Destination::Settings(
                        SettingsPage::from_id(id).unwrap_or(SettingsPage::General),
                    ),
                    cx,
                ),
                None => downloads.update(cx, |d, cx| d.demo(other, cx)),
            },
        }
        Self {
            navigation,
            focus: cx.focus_handle(),
            data,
            downloads,
            settings,
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

    fn page(&self, current: Destination, cx: &mut Context<Self>) -> AnyElement {
        match current {
            Destination::Welcome => pages::welcome(&self.data, cx),
            Destination::Downloads => self.downloads.clone().into_any_element(),
            Destination::Logs => pages::logs(&self.data, cx),
            Destination::Settings(_) => self.settings.clone().into_any_element(),
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
            .child(div().flex_1().min_h_0().child(self.page(current, cx)))
    }
}
