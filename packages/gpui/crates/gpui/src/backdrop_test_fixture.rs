//! Pixel checks shared by the native renderers for backdrops inside transformed layers.

use gpui::{
    Backdrop, BorderStyle, Bounds, ContentMask, Corners, Edges, Filter, Quad, ScaledPixels, Scene,
    point, size,
};

/// Builds a split red and blue background behind one or two scaled backdrop layers.
pub fn scene(depth: usize, blur: f32, background_in_layer: bool) -> Scene {
    let mut scene = Scene::default();
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(64.), ScaledPixels(64.)),
    );
    for level in 0..depth {
        if level == 0 && !background_in_layer {
            background(&mut scene);
        }
        scene.push_filter(
            full,
            full.center(),
            full,
            Filter {
                scale: 0.9,
                ..Filter::default()
            },
        );
        if level == 0 && background_in_layer {
            background(&mut scene);
        }
    }
    if depth == 0 {
        background(&mut scene);
    }
    let bounds = Bounds::new(
        point(ScaledPixels(16.), ScaledPixels(16.)),
        size(ScaledPixels(32.), ScaledPixels(32.)),
    );
    scene.insert_primitive(Backdrop {
        order: 0,
        corner_smoothing: 0.,
        blur,
        opacity: 1.,
        bounds,
        content_mask: ContentMask { bounds: full },
        corner_radii: Corners::default(),
    });
    for _ in 0..depth {
        scene.pop_filter();
    }
    scene.finish();
    scene
}

/// Ensures the backdrop sampled both sides of the ancestor's color boundary.
pub fn check(image: &image::RgbaImage) {
    let pixel = image.get_pixel(31, 32).0;
    assert!(
        pixel[0] > 20 && pixel[2] > 20 && pixel[3] > 245,
        "ancestor backdrop was not blurred: {pixel:?}"
    );
    assert_eq!(image.get_pixel(8, 32).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(56, 32).0, [0, 0, 255, 255]);
}

/// Paints a high contrast boundary so a missing or unblurred backdrop fails visibly.
fn background(scene: &mut Scene) {
    for (x, color) in [(0., gpui::red()), (32., gpui::blue())] {
        let bounds = Bounds::new(
            point(ScaledPixels(x), ScaledPixels(0.)),
            size(ScaledPixels(32.), ScaledPixels(64.)),
        );
        scene.insert_primitive(Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds,
            content_mask: ContentMask { bounds },
            background: color.into(),
            border_color: color,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
            corner_smoothing: 0.,
            pad: 0,
        });
    }
}
