//! Maps the appearance settings (theme, background, text size) onto `ui::Look` and the windows.

use domain::Settings;
use gpui::{App, Window};
use state::AppState;
use ui::{ActiveTheme as _, Look, Rounding, Theme, ThemeKind, ThemeOverrides, WindowBlur};

/// The text sizes offered, in px.
pub const FONT_SIZES: [f32; 5] = [11., 12., 13., 14., 16.];

/// The look `s` asks for; `theme` overrides the stored theme id (the `--theme` flag).
pub fn look_of(s: &Settings, theme: Option<&str>) -> Look {
    let (transparent, blur_window) = match s.background.as_str() {
        "translucent" => (true, false),
        "blur" => (true, true),
        _ => (false, false),
    };
    Look {
        kind: ThemeKind::from_id(theme.unwrap_or(&s.theme)),
        rounding: Rounding::Subtle,
        font: s.font_size,
        transparent,
        transparency: if transparent {
            s.transparency.clamp(0., ui::MAX_TRANSPARENCY)
        } else {
            1.
        },
        blur: s.blur_ui,
        blur_window,
        tint: None,
        tint_secondary: None,
    }
}

/// Sets text size and background of `window` from the current theme.
pub fn apply_window(window: &mut Window, cx: &mut App) {
    WindowBlur::update(window, cx);
    let theme = *cx.theme();
    window.set_rem_size(theme.font_size);
    window.set_background_appearance(ui::backdrop(theme.blur_window, theme.transparent));
}

/// The colours the theme id (or the `--theme` flag) lays over the Ledger palette.
pub fn overrides_of(s: &Settings, theme: Option<&str>) -> ThemeOverrides {
    super::themes::overrides(theme.unwrap_or(&s.theme))
}

/// Applies the stored look and theme colours to the theme and every open window.
pub fn apply_look(look: Look, overrides: &ThemeOverrides, cx: &mut App) {
    Theme::set(look, overrides, cx);
    for handle in cx.windows() {
        // a window busy rendering is skipped, the Settings page applies it on its next frame
        handle
            .update(cx, |_, window, cx| apply_window(window, cx))
            .ok();
    }
}

/// Applies the settings stored in `AppState` at startup; `theme` is an explicit `--theme`.
pub fn apply_stored(theme: Option<&str>, cx: &mut App) {
    let s = AppState::global(cx).settings.read(cx).get().clone();
    apply_look(look_of(&s, theme), &overrides_of(&s, theme), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_modes() {
        let mut s = Settings::default();
        let look = look_of(&s, None);
        assert!(!look.transparent && !look.blur_window && look.font == 12.);
        s.background = "blur".into();
        let look = look_of(&s, Some("light"));
        assert!(look.transparent && look.blur_window && look.kind == ThemeKind::Light);
        assert!(!look.blur && look.transparency == 0.15);
        s.background = "translucent".into();
        (s.transparency, s.blur_ui) = (0.6, true);
        let look = look_of(&s, None);
        assert!(look.transparent && !look.blur_window && look.blur && look.transparency == 0.6);
        // a theme file is laid over the dark palette
        s.theme = "nord".into();
        assert_eq!(look_of(&s, None).kind, ThemeKind::Dark);
        assert!(overrides_of(&s, None).background.is_some());
        assert_eq!(overrides_of(&s, Some("light")), ThemeOverrides::default());
    }
}
