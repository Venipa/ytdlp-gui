//! The keyboard focus ring every focusable part shares: a 1px ink-blue edge that shows only when
//! focus arrived by keyboard.

use gpui::prelude::*;
use gpui::{App, ElementId, FocusHandle, Window};

use crate::theme::ActiveTheme as _;

/// Makes `element` a Tab stop that draws the ring while it holds focus from the keyboard. Enter
/// and Space then click it. `id` keys the focus handle, so it must be stable and unique.
pub fn focusable<E>(element: E, id: &ElementId, window: &mut Window, cx: &mut App) -> E
where
    E: InteractiveElement + StatefulInteractiveElement,
{
    let ring = cx.theme().table_active_border;
    let handle = handle(id, window, cx);
    element
        .track_focus(&handle)
        .focus_visible(move |style| style.border_1().border_color(ring))
}

/// The focus handle `focusable` keeps for `id`, for callers that move focus to it.
pub fn handle(id: &ElementId, window: &mut Window, cx: &mut App) -> FocusHandle {
    window
        .use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(id.clone()), "focus".into()),
            cx,
            |_, cx| cx.focus_handle().tab_stop(true),
        )
        .read(cx)
        .clone()
}
