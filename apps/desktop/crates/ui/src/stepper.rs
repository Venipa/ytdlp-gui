//! A number field between a minus and a plus button, drawn as one bordered group of `control`
//! height so the three parts share one top, one bottom and one focus ring.

use std::rc::Rc;

use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{App, BoxShadow, ElementId, Entity, Pixels, Window, div, point, px, svg};

use crate::input::Input;
use crate::theme::ActiveTheme as _;

type Step = Rc<dyn Fn(i32, &mut Window, &mut App)>;

/// `[−][ 3 ][+]`: the field is the caller's `Input` (built with `.joined().align_center()`),
/// the buttons call `on_step` with -1 or +1.
#[derive(IntoElement)]
pub struct Stepper {
    id: ElementId,
    input: Entity<Input>,
    width: Pixels,
    invalid: bool,
    on_step: Option<Step>,
}

impl Stepper {
    /// A stepper over `input`, the number part `width` wide.
    pub fn new(id: impl Into<ElementId>, input: &Entity<Input>, width: Pixels) -> Self {
        Self {
            id: id.into(),
            input: input.clone(),
            width,
            invalid: false,
            on_step: None,
        }
    }

    /// Draws the group's edge in the danger colour while the field holds a refused value.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Runs `step` with -1 for minus and +1 for plus.
    pub fn on_step(mut self, step: impl Fn(i32, &mut Window, &mut App) + 'static) -> Self {
        self.on_step = Some(Rc::new(step));
        self
    }
}

impl RenderOnce for Stepper {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let height = theme.metrics.control;
        let focused = self.input.read(cx).is_focused(window);
        let edge = match (self.invalid, focused) {
            (true, _) => theme.danger,
            (false, true) => theme.table_active_border,
            (false, false) => theme.border,
        };
        let button = |delta: i32, window: &mut Window, cx: &mut App| {
            let (icon, key) = match delta < 0 {
                true => ("icons/minus.svg", "stepper-less"),
                false => ("icons/plus.svg", "stepper-more"),
            };
            let id = ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), key.into());
            let step = self.on_step.clone();
            crate::focus::focusable(div().id(id.clone()), &id, window, cx)
                .role(Role::Button)
                .aria_label(i18n::lookup(key, None))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(height - px(2.))
                .cursor_pointer()
                .text_color(theme.muted_foreground)
                .hover(|style| style.bg(theme.secondary_hover))
                .active(|style| style.bg(theme.secondary_active))
                .map(|this| match delta < 0 {
                    true => this.border_r_1(),
                    false => this.border_l_1(),
                })
                .border_color(theme.border)
                .group(key)
                .child(
                    svg()
                        .path(icons::path(icon))
                        .size(px(14.))
                        .text_color(theme.muted_foreground)
                        .group_hover(key, |style| style.text_color(theme.foreground)),
                )
                .when_some(step, |this, step| {
                    this.on_click(move |_, window, cx| step(delta, window, cx))
                })
                .map(|this| {
                    crate::motion::pressing(this, &id, crate::motion::PRESS_SCALE, window, cx)
                })
        };
        let less = button(-1, window, cx);
        let more = button(1, window, cx);
        div()
            .id(self.id.clone())
            .flex()
            .flex_none()
            .items_center()
            .h(height)
            .rounded(theme.radius)
            .border_1()
            .border_color(edge)
            .bg(theme.foreground.opacity(0.045))
            .overflow_hidden()
            .when(focused && !self.invalid, |this| {
                this.shadow(vec![BoxShadow {
                    color: theme.table_active_border.opacity(0.4),
                    offset: point(Pixels::ZERO, Pixels::ZERO),
                    blur_radius: Pixels::ZERO,
                    spread_radius: px(2.),
                    inset: false,
                }])
            })
            .child(less)
            .child(
                div()
                    .w(self.width)
                    .h_full()
                    .flex()
                    .items_center()
                    .child(self.input),
            )
            .child(more)
    }
}
