#![cfg_attr(target_family = "wasm", no_main)]

use std::{fs, path::PathBuf};

use anyhow::Result;
use gpui::{
    App, AssetSource, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div, img,
    prelude::*, px, rgb, size, svg,
};
use gpui_platform::application;

struct Assets {
    base: PathBuf,
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        fs::read(self.base.join(path))
            .map(|data| Some(std::borrow::Cow::Owned(data)))
            .map_err(Into::into)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        fs::read_dir(self.base.join(path))
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        entry
                            .ok()
                            .and_then(|entry| entry.file_name().into_string().ok())
                            .map(SharedString::from)
                    })
                    .collect()
            })
            .map_err(Into::into)
    }
}

struct LayerScaleExample;

impl LayerScaleExample {
    fn card(label: &'static str, scale: f32) -> impl IntoElement {
        div()
            .w(px(320.0))
            .h(px(220.0))
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .rounded_lg()
            .border_2()
            .border_color(rgb(0x4d66cc))
            .bg(rgb(0xf7f8ff))
            .blur(px(1.5))
            .layer_scale(scale)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(img("image/app-icon.png").size_12())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(label)
                            .child(format!("compositor scale {scale:.4}")),
                    )
                    .child(
                        svg()
                            .path("image/arrow_circle.svg")
                            .text_color(rgb(0x4d66cc))
                            .size_8(),
                    ),
            )
            .child(
                div()
                    .text_lg()
                    .child("The entire subtree is sampled as one image."),
            )
            .child(
                div()
                    .text_sm()
                    .child("Text wrapping, image size, border width, and flex layout stay fixed."),
            )
    }
}

impl gpui::Render for LayerScaleExample {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_8()
            .bg(rgb(0xe9ebf5))
            .child(Self::card("Reference", 1.0))
            .child(Self::card("Scaled", 0.99))
    }
}

fn run_example() {
    application()
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples"),
        })
        .run(|cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(760.0), px(360.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| LayerScaleExample),
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
