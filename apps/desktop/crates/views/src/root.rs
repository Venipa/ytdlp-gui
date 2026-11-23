use gpui::prelude::*;
use gpui::{Context, Entity, Window, div};
use router::Navigation;

use crate::workspace::Workspace;

/// The bundled typeface every window text uses.
pub const UI_FONT: &str = "IBM Plex Sans";

/// The window's root view: sets the typeface and hosts the Workspace.
pub struct Root {
    workspace: Entity<Workspace>,
}

impl Root {
    /// Builds the root over the current navigation.
    pub fn new(scene: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let navigation: Entity<Navigation> = router::trail(cx);
        Self {
            workspace: cx.new(|cx| Workspace::new(navigation, scene, window, cx)),
        }
    }
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .font_family(UI_FONT)
            .child(self.workspace.clone())
    }
}
