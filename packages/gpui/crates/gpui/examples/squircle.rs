#![cfg_attr(target_family = "wasm", no_main)]

//! Round corners next to smoothed ones, with the borders, shadows and backdrop blur that follow
//! the corner's shape. Each row keeps one radius and steps the smoothing from 0 to 1.

use gpui::{
    App, Bounds, BoxShadow, Context, Div, Hsla, SharedString, Window, WindowBounds, WindowOptions,
    div, hsla, linear_color_stop, linear_gradient, pattern_slash, prelude::*, px, rgb, size,
};
use gpui_platform::application;

const SMOOTHING: [f32; 4] = [0., 0.4, 0.7, 1.];
const RADII: [f32; 3] = [12., 24., 40.];

struct Squircle;

/// One card of `radius` and `smoothing`, with a caption under it.
fn card(radius: f32, smoothing: f32, style: impl FnOnce(Div) -> Div) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .child(style(
            div()
                .size(px(120.))
                .rounded(px(radius))
                .corner_smoothing(smoothing),
        ))
        .child(
            div()
                .text_xs()
                .text_color(hsla(0., 0., 1., 0.7))
                .child(SharedString::from(format!("{radius} px, {smoothing}"))),
        )
}

/// A titled row of cards that share `style`, one for every radius and smoothing.
fn row(title: &'static str, style: impl Fn(Div) -> Div + Copy) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_sm()
                .text_color(hsla(0., 0., 1., 0.9))
                .child(title),
        )
        .children(RADII.map(|radius| {
            div()
                .flex()
                .gap_6()
                .children(SMOOTHING.map(|smoothing| card(radius, smoothing, style)))
        }))
}

fn filled(div: Div) -> Div {
    div.bg(rgb(0xf4f4f5))
}

fn outlined(div: Div) -> Div {
    div.bg(rgb(0x18181b)).border_2().border_color(rgb(0x8b5cf6))
}

fn shadowed(div: Div) -> Div {
    div.bg(rgb(0x27272a)).shadow(vec![
        BoxShadow::new(px(0.), px(10.), hsla(0., 0., 0., 0.6)).blur_radius(px(18.)),
    ])
}

fn frosted(div: Div) -> Div {
    div.bg(Hsla::white().opacity(0.12))
        .border_1()
        .border_color(Hsla::white().opacity(0.3))
        .backdrop_blur(px(12.))
}

impl Render for Squircle {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("squircle")
            .size_full()
            .overflow_y_scroll()
            .bg(linear_gradient(
                135.,
                linear_color_stop(rgb(0x1e1b4b), 0.),
                linear_color_stop(rgb(0x9d174d), 1.),
            ))
            .p_8()
            .flex()
            .flex_wrap()
            .gap_12()
            .child(row("Fill", filled))
            .child(row("Border", outlined))
            .child(row("Shadow", shadowed))
            // Stripes behind the frosted cards, so the blur and the corners it follows show.
            .child(
                div()
                    .rounded(px(16.))
                    .p_4()
                    .bg(pattern_slash(Hsla::white().opacity(0.25), 6., 10.))
                    .child(row("Backdrop blur", frosted)),
            )
    }
}

fn run_example() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1400.), px(980.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Squircle),
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_example();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_example();
}
