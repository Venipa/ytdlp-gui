use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ElementId, Hsla, PathBuilder, Pixels, StyleRefinement, Window, canvas, div, point,
    px,
};

use crate::motion;
use crate::theme::ActiveTheme as _;

/// The spinner's size, the same as a button's icon.
const SIZE: Pixels = px(16.);
/// How many bars sit round the circle.
const BARS: usize = 12;
/// How long every bar takes to fade from lit to dim.
const CYCLE: f32 = 1.2;
/// How much later each bar starts than the one before it, in seconds.
const STAGGER: f32 = 0.1;
/// How dim a bar gets before it lights up again.
const DIM: f32 = 0.25;
/// Where a bar starts and ends along its radius, and its thickness, as fractions of the size.
const INNER: f32 = 0.2304;
const OUTER: f32 = 0.4704;
const THICKNESS: f32 = 0.08;

/// Twelve bars round a circle that light up in turn, for work that has started and not answered
/// yet. It takes the text colour and holds on one frame when the system asks for less motion.
#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    base: gpui::Div,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            base: div(),
        }
    }
}

impl Styled for Spinner {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self { id, mut base } = self;
        let overrides = std::mem::take(base.style());
        let animated = motion::animates(cx);

        let mut root = div()
            .id(id)
            .role(gpui::accesskit::Role::ProgressIndicator)
            .aria_label(i18n::lookup("spinner-loading", None))
            .size(SIZE)
            .flex_none()
            .text_color(cx.theme().foreground)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let color = window.text_style().color;
                        paint(bounds, color, animated.then(elapsed).unwrap_or(0.), window);
                        if animated {
                            window.request_animation_frame();
                        }
                    },
                )
                .size_full(),
            );
        root.style().refine(&overrides);
        root
    }
}

/// Seconds since the first spinner was drawn, so every spinner on screen pulses in step.
fn elapsed() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// How lit bar `index` is `seconds` in: it fades linearly from 1 to `DIM` over `CYCLE`, then
/// jumps back, each bar `STAGGER` behind the one before it.
fn opacity(index: usize, seconds: f32) -> f32 {
    let delay = (index as f32 - BARS as f32) * STAGGER;
    let along = (seconds - delay).rem_euclid(CYCLE) / CYCLE;
    1. - (1. - DIM) * along
}

/// Draws the bars into `bounds` in `color`, lit as at `seconds`.
fn paint(bounds: Bounds<Pixels>, color: Hsla, seconds: f32, window: &mut Window) {
    let size = bounds.size.width.min(bounds.size.height);
    let center = bounds.center();
    for index in 0..BARS {
        let angle = index as f32 / BARS as f32 * TAU;
        let (sin, cos) = angle.sin_cos();
        let corner = |radial: f32, across: f32| {
            let (radial, across) = (radial * size, across * size);
            point(
                center.x + radial * cos - across * sin,
                center.y + radial * sin + across * cos,
            )
        };
        let half = THICKNESS / 2.;
        let mut path = PathBuilder::fill();
        path.add_polygon(
            &[
                corner(INNER, -half),
                corner(OUTER, -half),
                corner(OUTER, half),
                corner(INNER, half),
            ],
            true,
        );
        if let Ok(path) = path.build() {
            window.paint_path(path, color.opacity(opacity(index, seconds)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lead_bar_is_lit_and_the_next_one_has_just_dimmed() {
        assert!((opacity(0, 0.) - 1.).abs() < 1e-4);
        assert!(opacity(1, 0.) < opacity(11, 0.));
        for index in 0..BARS {
            assert!((DIM..=1.).contains(&opacity(index, 0.37)));
        }
    }

    #[test]
    fn a_bar_repeats_every_cycle() {
        assert!((opacity(3, 0.2) - opacity(3, 0.2 + CYCLE)).abs() < 1e-4);
    }
}
