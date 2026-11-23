//! Clipboard watcher adapter: reads when the window is activated, never writes.
use cli::clipboard::{ClipboardRead, Watcher};
use gpui::{App, AppContext as _, Context, Entity, Window};

struct Gpui<'a>(&'a App);

impl ClipboardRead for Gpui<'_> {
    fn text(&self) -> Option<String> {
        self.0.read_from_clipboard()?.text()
    }
}

/// Watch state held by the entity that observes window activation.
pub struct Watch {
    watcher: Watcher,
}

/// Calls `on_found` with URL candidates each time `window` becomes active and the clipboard text changed.
/// The returned entity must be kept alive.
pub fn attach(
    window: &mut Window,
    cx: &mut App,
    on_found: impl Fn(Vec<String>, &mut App) + 'static,
) -> Entity<Watch> {
    cx.new(|cx: &mut Context<Watch>| {
        cx.observe_window_activation(window, move |this, window, cx| {
            if !window.is_window_active() {
                return;
            }
            let found = this.watcher.check(&Gpui(cx));
            if !found.is_empty() {
                on_found(found, cx);
            }
        })
        .detach();
        Watch {
            watcher: Watcher::default(),
        }
    })
}
