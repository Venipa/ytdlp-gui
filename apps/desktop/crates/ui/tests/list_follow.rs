//! A followed list (the Logs screen) under a wheel: it stays at its end, and scrolling back
//! down to the end never lands it at the top.

use gpui::{
    AppContext as _, Context, Element as _, Entity, FollowMode, IntoElement, ListAlignment,
    ListState, ParentElement as _, Render, ScrollDelta, ScrollWheelEvent, Styled as _,
    TestAppContext, VisualTestContext, Window, div, list, point, px,
};
use ui::{Scrollbar, Scroller, Theme};

struct Logs {
    lines: ListState,
    bar: Entity<Scrollbar>,
}

impl Render for Logs {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Scroller::listing("logs", &self.bar).child(
            list(self.lines.clone(), |at, _, _| {
                // uneven rows, as wrapped log lines are
                div().w_full().h(px(18. + (at % 3) as f32 * 6.)).into_any()
            })
            .size_full(),
        )
    }
}

fn setup(cx: &mut TestAppContext, count: usize) -> (VisualTestContext, Entity<Logs>, ListState) {
    cx.update(|cx| cx.set_global(Theme::dark()));
    let lines = ListState::new(count, ListAlignment::Top, px(200.));
    lines.set_follow_mode(FollowMode::Tail);
    let state = lines.clone();
    let window = cx.add_window(|_, cx| Logs {
        lines: state.clone(),
        bar: cx.new(|_| Scrollbar::list(state)),
    });
    let view = window.root(cx).unwrap();
    let vcx = VisualTestContext::from_window(window.into(), cx);
    (vcx, view, lines)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..60 {
        cx.update(|window, _| window.refresh());
        cx.simulate_frame();
        cx.run_until_parked();
    }
}

fn wheel(cx: &mut VisualTestContext, lines: f32) {
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(50.), px(150.)),
        delta: ScrollDelta::Lines(point(0., lines)),
        ..Default::default()
    });
    draw(cx);
}

fn near_end(lines: &ListState, _count: usize) -> bool {
    -lines.scroll_px_offset_for_scrollbar().y >= lines.max_offset_for_scrollbar().y - px(2.)
}

#[gpui::test]
fn wheel_down_at_the_end_keeps_following(cx: &mut TestAppContext) {
    let (mut cx, _view, lines) = setup(cx, 500);
    let cx = &mut cx;
    draw(cx);
    assert!(near_end(&lines, 500), "{:?}", lines.logical_scroll_top());
    for _ in 0..5 {
        wheel(cx, -3.);
        assert!(near_end(&lines, 500), "{:?}", lines.logical_scroll_top());
    }
    // a new line still shows
    lines.splice(500..500, 1);
    draw(cx);
    assert!(near_end(&lines, 501), "{:?}", lines.logical_scroll_top());
}

#[gpui::test]
fn back_down_to_the_end_does_not_jump_to_the_top(cx: &mut TestAppContext) {
    let (mut cx, _view, lines) = setup(cx, 500);
    let cx = &mut cx;
    draw(cx);
    for _ in 0..4 {
        wheel(cx, 3.);
    }
    let up = lines.logical_scroll_top().item_ix;
    assert!(up < 495, "did not scroll up: {up}");
    for step in 0..12 {
        wheel(cx, -3.);
        let at = lines.logical_scroll_top().item_ix;
        assert!(at + 40 >= up, "step {step}: jumped from about {up} to {at}");
    }
    assert!(near_end(&lines, 500), "{:?}", lines.logical_scroll_top());
    lines.splice(500..500, 3);
    draw(cx);
    assert!(near_end(&lines, 503), "{:?}", lines.logical_scroll_top());
}

#[gpui::test]
fn touchpad_back_down_to_the_end_does_not_jump_to_the_top(cx: &mut TestAppContext) {
    let (mut cx, _view, lines) = setup(cx, 500);
    let cx = &mut cx;
    draw(cx);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(50.), px(150.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(300.))),
        ..Default::default()
    });
    draw(cx);
    let up = lines.logical_scroll_top().item_ix;
    for step in 0..12 {
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(150.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-60.))),
            ..Default::default()
        });
        draw(cx);
        let at = lines.logical_scroll_top().item_ix;
        assert!(at + 40 >= up, "step {step}: jumped from about {up} to {at}");
    }
    assert!(near_end(&lines, 500), "{:?}", lines.logical_scroll_top());
}
