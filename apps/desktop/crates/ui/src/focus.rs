//! The keyboard focus ring every focusable part shares: an ink-blue edge with a 2px halo, the
//! same ring a focused field wears, shown only when focus arrived by keyboard.

use gpui::prelude::*;
use gpui::{App, BoxShadow, Div, ElementId, FocusHandle, Window, div, point, px};

use crate::theme::ActiveTheme as _;

/// Makes `element` a Tab stop that draws the ring while it holds focus from the keyboard. Enter
/// and Space then click it. `id` keys the focus handle, so it must be stable and unique.
pub fn focusable<E>(element: E, id: &ElementId, window: &mut Window, cx: &mut App) -> E
where
    E: InteractiveElement + StatefulInteractiveElement,
{
    let ring = cx.theme().table_active_border;
    let handle = handle(id, window, cx);
    // drawn inside the part with no border width of its own, so focus never moves the layout
    // and a card that clips its rows still shows it; a part with a border inks that border
    let rings = vec![edge(ring), halo(ring, true)];
    element
        .track_focus(&handle)
        .focus_visible(move |style| style.border_color(ring).shadow(rings))
}

/// The 1px ink edge drawn inside a part that has no border of its own.
fn edge(ring: gpui::Hsla) -> BoxShadow {
    BoxShadow {
        color: ring,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(1.),
        inset: true,
    }
}

/// The same ring laid over a part that fills its room edge to edge (a list, a row in it), where
/// an outer halo would be clipped: the ink edge with the halo inside it. Add it as the last
/// child of a `relative` parent while that part holds keyboard focus.
pub fn ring(cx: &App) -> Div {
    let ring = cx.theme().table_active_border;
    div()
        .absolute()
        .inset_0()
        .border_1()
        .border_color(ring)
        .shadow(vec![halo(ring, true)])
}

fn halo(ring: gpui::Hsla, inset: bool) -> BoxShadow {
    BoxShadow {
        color: ring.opacity(0.4),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(3.),
        inset,
    }
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
