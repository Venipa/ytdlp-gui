//! The screens of ytdlpd: the root, the Workspace shell with its title-bar navigation and the
//! pages it switches between.

mod card;
mod detail;
mod downloads;
mod format;
mod formats;
mod logs;
mod os;
mod root;
mod rows;
mod screens;
mod ticks;
mod toasts;
mod update_line;
mod welcome;
mod workspace;

pub use root::{Root, UI_FONT};
pub use screens::settings::{apply_stored, apply_window, look_of, overrides_of};
/// Shows a toast (with an optional button); the binary uses it for browser-extension adds.
pub use toasts::{Kind as ToastKind, toast};

/// The ink-blue used for selection, focus and links (a solid field of the theme).
pub fn ink(cx: &gpui::App) -> gpui::Hsla {
    use ui::ActiveTheme as _;
    cx.theme().table_active_border
}
