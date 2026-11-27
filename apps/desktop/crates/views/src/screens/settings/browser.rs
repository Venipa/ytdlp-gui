//! The Browser page: the extension switch, the pairing code and the paired browsers. The
//! server lives in `state::BridgeHandle` (set by the binary); the page only reads and asks it.

use std::time::{Duration, Instant};

use domain::Settings;
use gpui::prelude::*;
use gpui::{AnyElement, ClipboardItem, Context, SharedString, div, px};
use i18n::t;
use state::BridgeHandle;
use ui::{ActiveTheme as _, Button, MONO, OtpSlots, SwitchRow, Text, clock};

use super::SettingsView;
use super::layout::{full_bleed, row, section};
use crate::format;
use crate::toasts::{Kind, toast};

impl SettingsView {
    /// The running bridge, `None` while off, owned by another window or failed to start.
    fn bridge<'a>(&self, cx: &'a gpui::App) -> Option<&'a bridge::Bridge> {
        cx.try_global::<BridgeHandle>()?.0.as_ref()
    }

    /// Asks for a fresh code (the old one stops working) and counts it down once a second.
    fn new_code(&mut self, cx: &mut Context<Self>) {
        let Some(code) = self.bridge(cx).map(|b| b.new_code()) else {
            return;
        };
        self.pair = Some((code.code, code.expires));
        self.pair_tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let live = this
                    .update(cx, |this, cx| {
                        let live = this
                            .pair
                            .as_ref()
                            .is_some_and(|(_, at)| *at > Instant::now());
                        if !live {
                            this.pair = None;
                        }
                        cx.notify();
                        live
                    })
                    .unwrap_or(false);
                if !live {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn copy_code(&mut self, cx: &mut Context<Self>) {
        if let Some((code, _)) = &self.pair {
            cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
            toast(cx, Kind::Success, t!("set-browser-copied"), None);
        }
    }

    fn remove_device(&mut self, hash: &str, cx: &mut Context<Self>) {
        if let Some(b) = self.bridge(cx) {
            b.remove(hash);
        }
        cx.notify();
    }

    pub(super) fn page_browser(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = *cx.theme();
        let on = s.browser_extension;
        let small = |line: SharedString| {
            div()
                .py(px(10.))
                .text_size(theme.text(Text::Small))
                .text_color(theme.muted_foreground)
                .child(line)
                .into_any_element()
        };
        let mut main = vec![
            full_bleed(
                SwitchRow::new("browser-on", t!("set-browser-on"), on)
                    .detail(t!("set-browser-on-desc"))
                    .on_click(self.edit(move |st| st.browser_extension = !on)),
            )
            .into_any_element(),
        ];
        if !on {
            main.push(small(t!("set-browser-install")));
            return vec![section(Some(t!("set-sec-browser")), main, cx)];
        }
        let bridge = self.bridge(cx);
        main.push(small(match bridge {
            Some(b) => t!("set-browser-listening", port = b.port()),
            None => match cx.try_global::<BridgeHandle>().and_then(|h| h.1.clone()) {
                Some(reason) => t!("set-browser-failed", reason = reason),
                None => t!("set-browser-unavailable"),
            },
        }));
        main.push(small(t!("set-browser-install")));
        main.push(small(t!("set-browser-tray")));
        let mut out = vec![section(Some(t!("set-sec-browser")), main, cx)];
        let Some(bridge) = bridge else {
            return out;
        };

        let left = self
            .pair
            .as_ref()
            .map(|(_, at)| at.saturating_duration_since(Instant::now()))
            .filter(|left| !left.is_zero());
        let pair = match (&self.pair, left) {
            (Some((code, _)), Some(left)) => div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .py(px(12.))
                .child(OtpSlots::new(code.clone()).length(8).grouped(4))
                .child(
                    div()
                        .font_family(MONO)
                        .text_size(theme.text(Text::Small))
                        .text_color(theme.muted_foreground)
                        .child(t!("set-browser-valid", left = clock(left).to_string())),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            Button::new("browser-copy")
                                .outline()
                                .label(t!("set-browser-copy"))
                                .on_click(cx.listener(|this, _, _, cx| this.copy_code(cx))),
                        )
                        .child(
                            Button::new("browser-new")
                                .outline()
                                .label(t!("set-browser-new"))
                                .on_click(cx.listener(|this, _, _, cx| this.new_code(cx))),
                        ),
                )
                .into_any_element(),
            _ => row(
                t!("set-browser-pair"),
                Some(t!("set-browser-pair-desc")),
                Button::new("browser-pair")
                    .outline()
                    .label(t!("set-browser-pair-btn"))
                    .on_click(cx.listener(|this, _, _, cx| this.new_code(cx))),
                cx,
            ),
        };
        out.push(section(Some(t!("set-sec-browser-pair")), vec![pair], cx));

        let devices = bridge.devices();
        let rows = if devices.is_empty() {
            vec![small(t!("set-browser-empty"))]
        } else {
            devices
                .into_iter()
                .map(|d| {
                    let line = if d.connected {
                        t!("set-browser-device-connected", origin = d.origin.clone())
                    } else {
                        let (time, date) = format::when(d.last_seen * 1000);
                        let seen =
                            date.map_or_else(|| time.clone(), |date| format!("{date} {time}"));
                        t!(
                            "set-browser-device-line",
                            origin = d.origin.clone(),
                            seen = seen
                        )
                    };
                    let dot = if d.connected {
                        theme.primary
                    } else {
                        theme.muted_foreground.opacity(0.4)
                    };
                    let hash = d.hash.clone();
                    row(
                        SharedString::from(d.name.clone()),
                        Some(line),
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(div().size(px(8.)).rounded_full().bg(dot))
                            .child(
                                Button::new(SharedString::from(format!("browser-rm-{}", d.hash)))
                                    .outline()
                                    .label(t!("set-browser-remove"))
                                    .name(t!("set-browser-remove-name", name = d.name))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.remove_device(&hash, cx)
                                    })),
                            ),
                        cx,
                    )
                })
                .collect()
        };
        out.push(section(Some(t!("set-sec-browser-devices")), rows, cx));
        out
    }
}
