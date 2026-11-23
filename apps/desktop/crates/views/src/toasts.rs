//! The toast stack at the bottom right of the window. Any view pushes a toast with `toast(..)`;
//! each one leaves by itself after a few seconds.

use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{App, Context, Entity, Global, SharedString, Window, div, px};
use ui::Toast;

/// How long a toast stays when nobody dismisses it.
const LIFETIME: Duration = Duration::from_secs(6);

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
}

/// The layer entity; one per window, registered as a global so any handler can push.
pub struct ToastLayer {
    next: u64,
    items: Vec<Item>,
}

struct Handle(Entity<ToastLayer>);

impl Global for Handle {}

impl ToastLayer {
    /// Creates the layer and registers it.
    pub fn init(cx: &mut App) -> Entity<Self> {
        let layer = cx.new(|_| Self {
            next: 0,
            items: Vec::new(),
        });
        cx.set_global(Handle(layer.clone()));
        layer
    }

    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.items.retain(|item| item.id != id);
        cx.notify();
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
        layer.items.push(Item {
            id,
            kind,
            message,
            action,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LIFETIME).await;
            this.update(cx, |layer, cx| layer.dismiss(id, cx)).ok();
        })
        .detach();
    });
}

impl Render for ToastLayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layer = cx.entity();
        div()
            .absolute()
            .bottom(px(40.))
            .right(px(16.))
            .w(px(360.))
            .flex()
            .flex_col()
            .gap_2()
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
                card
            }))
    }
}
