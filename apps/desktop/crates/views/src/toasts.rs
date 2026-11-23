//! The toast stack at the bottom right of the window. Any view pushes a toast with `toast(..)`;
//! each one leaves by itself after a few seconds.

use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, App, Context, Entity, Global, SharedString, Window, div, px,
};
use ui::{Toast, motion};

/// How long a toast stays when nobody dismisses it; a pointer over the stack holds the clock.
const LIFETIME: Duration = Duration::from_secs(4);
/// A toast that carries a button stays longer, so there is time to reach it.
const LIFETIME_ACTION: Duration = Duration::from_secs(8);
/// The most toasts on screen; a new one pushes the oldest out.
const MAX: usize = 3;
const TICK: Duration = Duration::from_millis(100);
/// How long a leaving toast fades.
const LEAVE: Duration = Duration::from_millis(120);

/// What a toast reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Info,
    Success,
    Error,
}

type Act = Rc<dyn Fn(&mut App)>;

struct Item {
    id: u64,
    kind: Kind,
    message: SharedString,
    action: Option<(SharedString, Act)>,
    left: Duration,
    leaving: bool,
}

/// The layer entity; one per window, registered as a global so any handler can push.
pub struct ToastLayer {
    next: u64,
    items: Vec<Item>,
    hovered: bool,
    ticking: bool,
}

struct Handle(Entity<ToastLayer>);

impl Global for Handle {}

impl ToastLayer {
    /// Creates the layer and registers it.
    pub fn init(cx: &mut App) -> Entity<Self> {
        let layer = cx.new(|_| Self {
            next: 0,
            items: Vec::new(),
            hovered: false,
            ticking: false,
        });
        cx.set_global(Handle(layer.clone()));
        layer
    }

    /// Fades the toast out, then drops it; with reduced motion it goes at once.
    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return;
        };
        if item.leaving {
            return;
        }
        if !motion::animates(cx) {
            self.items.retain(|item| item.id != id);
            cx.notify();
            return;
        }
        item.leaving = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LEAVE).await;
            this.update(cx, |layer, cx| {
                layer.items.retain(|item| item.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Counts every toast down while the pointer is away, and dismisses the ones that ran out.
    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let go = this
                    .update(cx, |layer, cx| {
                        if !layer.hovered {
                            let due: Vec<u64> = layer
                                .items
                                .iter_mut()
                                .filter(|item| !item.leaving)
                                .filter_map(|item| {
                                    item.left = item.left.saturating_sub(TICK);
                                    item.left.is_zero().then_some(item.id)
                                })
                                .collect();
                            for id in due {
                                layer.dismiss(id, cx);
                            }
                        }
                        let live = !layer.items.is_empty();
                        layer.ticking = live;
                        live
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        })
        .detach();
    }
}

/// Shows a toast, with an optional button (`Undo`, `Show in folder`).
pub fn toast(
    cx: &mut App,
    kind: Kind,
    message: impl Into<SharedString>,
    action: Option<(SharedString, Act)>,
) {
    let Some(Handle(layer)) = cx.try_global::<Handle>().map(|h| Handle(h.0.clone())) else {
        return;
    };
    let message = message.into();
    layer.update(cx, |layer, cx| {
        layer.next += 1;
        let id = layer.next;
        let left = if action.is_some() {
            LIFETIME_ACTION
        } else {
            LIFETIME
        };
        layer.items.push(Item {
            id,
            kind,
            message,
            action,
            left,
            leaving: false,
        });
        while layer.items.len() > MAX {
            layer.items.remove(0);
        }
        layer.tick(cx);
        cx.notify();
    });
}

impl Render for ToastLayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layer = cx.entity();
        div()
            .id("toast-stack")
            .absolute()
            .bottom(px(40.))
            .right(px(16.))
            .w(px(360.))
            .flex()
            .flex_col()
            .gap_2()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered = *hovered;
                cx.notify();
            }))
            .children(self.items.iter().map(|item| {
                let id = item.id;
                let icon = match item.kind {
                    Kind::Info => "icons/info.svg",
                    Kind::Success => "icons/circle-check.svg",
                    Kind::Error => "icons/circle-alert.svg",
                };
                let dismissed = layer.clone();
                let mut card = Toast::new(("toast", id as usize), item.message.clone())
                    .icon(icon)
                    .single_line()
                    .when(item.leaving, |card| card.inert())
                    .on_dismiss(move |_, _, cx| {
                        dismissed.update(cx, |layer, cx| layer.dismiss(id, cx))
                    });
                if let Some((label, act)) = item.action.clone() {
                    let acted = layer.clone();
                    card = card.action(label, move |_, _, cx| {
                        act(cx);
                        acted.update(cx, |layer, cx| layer.dismiss(id, cx));
                    });
                }
                let fade = item.leaving;
                div().child(card).with_animation(
                    ("toast-fade", id as usize * 2 + usize::from(fade)),
                    Animation::new(if fade { LEAVE } else { motion::dim_span() }),
                    move |el, delta| el.opacity(if fade { 1. - delta } else { delta }),
                )
            }))
    }
}
