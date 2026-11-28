//! The detail panel: the selected download as a header, an action row, collapsible details and a
//! log tail. Deleting goes through the "..." menu and a confirmation, never a red button.

use std::time::Duration;

use domain::{DownloadStatus, DownloadSummary, MediaKind};
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, FontWeight, SharedString, div, px, svg,
};
use i18n::t;
use ui::{
    ActiveTheme as _, Button, CallNumber, MONO, Panel, Perch, Scroller, Side, Text, Thumbnail,
    Tipped as _, ease_out_cubic, tabular,
};

use crate::downloads::{Downloads, error_text_of};
use crate::format;

/// How long the panel takes to open.
const SLIDE: Duration = Duration::from_millis(160);
/// The panel's width.
pub(crate) const PANEL: f32 = 360.;
/// The widest the panel may be dragged, which is also what its slide opens to.
const WIDEST: f32 = 560.;
/// The log lines the tail keeps.
const TAIL: usize = 60;

impl Downloads {
    /// A labelled line of the Details section: the value is cut with an ellipsis and shows whole
    /// on hover. A `copy` value (link, file) runs to two lines and copies itself on click.
    fn fact(
        &self,
        key: &'static str,
        label: SharedString,
        value: String,
        mono: bool,
        copy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        // the label sits on the value's first line, also when a link runs to two
        div()
            .flex()
            .items_start()
            .gap(px(12.))
            .min_h(px(20.))
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .text_size(theme.text(Text::Small))
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .id(key)
                    .flex_1()
                    .min_w_0()
                    .when(!copy, |this| this.truncate())
                    .when(mono, |this| {
                        this.font_family(MONO).text_size(theme.text(Text::Small))
                    })
                    .when(copy, |this| {
                        let text = value.clone();
                        this.line_clamp(2)
                            .cursor_pointer()
                            .hover(|style| style.text_color(theme.table_active_border))
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    text.clone(),
                                ));
                                crate::toasts::toast(
                                    cx,
                                    crate::toasts::Kind::Success,
                                    t!("dl-copied"),
                                    None,
                                );
                            })
                    })
                    .tip_label(value.clone(), Perch::Above)
                    .child(value),
            )
            .into_any_element()
    }

    /// The 360 px right panel for `s`, a sheet over the list pinned to the right edge.
    /// What `Fixes` says about the finished file: a fix running, a fix that failed, or a file
    /// that may not play everywhere, with the fix as a button.
    fn fix_note(&self, id: domain::DownloadId, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = *cx.theme();
        let state = self.app.fixes.read(cx).state(id)?.clone();
        let fixes = self.app.fixes.clone();
        let (tone, text, action): (_, SharedString, Option<(SharedString, domain::FixChain)>) =
            match state {
                state::FixState::Queued(fix) => (
                    theme.muted_foreground,
                    t!("dl-fix-queued", fix = crate::format::chain_name(fix)),
                    None,
                ),
                state::FixState::Running(fix) => (
                    theme.muted_foreground,
                    match self.app.fixes.read(cx).progress(id) {
                        Some(share) => t!(
                            "dl-fix-running-at",
                            fix = crate::format::chain_name(fix),
                            percent = format!("{:.0}", share * 100.)
                        ),
                        None => t!("dl-fix-running", fix = crate::format::chain_name(fix)),
                    },
                    None,
                ),
                state::FixState::Failed(fix, error) => (
                    theme.danger,
                    t!(
                        "dl-fix-failed",
                        fix = crate::format::chain_name(fix),
                        error = error
                    ),
                    Some((t!("dl-fix-retry"), fix)),
                ),
                state::FixState::Unplayable(why) => (
                    theme.progress_bar,
                    t!("dl-fix-unplayable", parts = why.parts.join(", ")),
                    Some((t!("dl-fix-make-compatible"), domain::Fix::Compatible.into())),
                ),
            };
        let dismiss = fixes.clone();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(8.))
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.secondary)
                .child(div().size(px(6.)).flex_none().rounded_full().bg(tone))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(theme.text(Text::Small))
                        .child(text),
                )
                .children(action.map(|(label, fix)| {
                    Button::new("fix-action")
                        .outline()
                        .small()
                        .label(label)
                        .on_click(move |_, _, cx| fixes.update(cx, |f, cx| f.run(id, fix, cx)))
                }))
                .child(
                    Button::new("fix-dismiss")
                        .ghost()
                        .small()
                        .icon("icons/x.svg")
                        .name(t!("dl-fix-dismiss"))
                        .on_click(move |_, _, cx| dismiss.update(cx, |f, cx| f.dismiss(id, cx))),
                )
                .into_any_element(),
        )
    }

    /// The details of `s`, beside the list under the link field and filters. `fill`: the page is
    /// too narrow for both, so the panel takes the list's place until it is closed.
    pub(crate) fn detail_panel(
        &self,
        s: &DownloadSummary,
        fill: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let pencil = theme.muted_foreground;
        let id = s.id;
        let status = s.status;
        let has_file = s.file_path.is_some();
        // which set gave which domains when the download last started
        let cookies = self.app.downloads.read(cx).cookies_used(id).map(|used| {
            used.iter()
                .map(|(set, domains)| format!("{set}: {}", domains.join(", ")))
                .collect::<Vec<_>>()
                .join("; ")
        });
        let title = s.title.clone().unwrap_or_else(|| s.url.clone());
        let thumb = s
            .thumbnail_path
            .clone()
            .or_else(|| s.thumbnail_url.clone())
            .map(SharedString::from);

        let action = |key: &'static str, icon: &'static str, label: SharedString| {
            Button::new(key).outline().icon(icon).label(label)
        };
        let primary = match status {
            DownloadStatus::Paused => Some(
                action("detail-resume", "icons/play.svg", t!("dl-resume"))
                    .on_click(cx.listener(move |this, _, _, cx| this.resume(id, cx))),
            ),
            DownloadStatus::Failed | DownloadStatus::Cancelled => Some(
                action("detail-retry", "icons/rotate-cw.svg", t!("dl-try-again"))
                    .on_click(cx.listener(move |this, _, _, cx| this.retry(id, cx))),
            ),
            s if !s.is_terminal() => Some(
                action("detail-pause", "icons/pause.svg", t!("dl-pause"))
                    .on_click(cx.listener(move |this, _, _, cx| this.pause(id, cx))),
            ),
            _ => None,
        };
        let actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .children(primary)
            .when(has_file && status == DownloadStatus::Completed, |this| {
                this.child(
                    action("open-file", "icons/external-link.svg", t!("dl-open"))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_file(id, cx))),
                )
            })
            .when(has_file, |this| {
                this.child(
                    action("show-folder", "icons/folder-open.svg", t!("dl-show-folder"))
                        .on_click(cx.listener(move |this, _, _, cx| this.show_folder(id, cx))),
                )
            })
            .when(!status.is_terminal(), |this| {
                this.child(
                    Button::new("detail-cancel")
                        .ghost()
                        .icon("icons/x.svg")
                        .label(t!("dl-cancel-download"))
                        .on_click(cx.listener(move |this, _, _, cx| this.cancel(id, cx))),
                )
            })
            .child(
                Button::new("detail-copy-link")
                    .ghost()
                    .icon("icons/link.svg")
                    .tooltip("dl-copy-link")
                    .on_click(cx.listener(move |this, _, _, cx| this.copy_link(id, cx))),
            )
            // delete and the rest live in the same menu a right-click opens
            .child(
                Button::new("detail-more")
                    .ghost()
                    .icon("icons/ellipsis.svg")
                    .tooltip("dl-more")
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                        this.menu = Some((event.position(), id));
                        cx.notify();
                    })),
            );

        let fixing = {
            let fixes = self.app.fixes.read(cx);
            fixes
                .running(s.id)
                .map(|fix| (fix, false))
                .or_else(|| fixes.queued(s.id).map(|fix| (fix, true)))
        };
        let (status_label, tone) = crate::rows::status_text(s, fixing, &theme);
        // the error box under the actions carries the reason, the header only the state
        let status_label = match status {
            DownloadStatus::Failed => t!("dl-status-failed"),
            _ => status_label,
        };
        let header = div()
            .flex()
            .gap(px(12.))
            .items_start()
            .child(
                Thumbnail::new(thumb, px(112.))
                    .alt("detail-cover", t!("dl-cover-alt"))
                    .audio(s.kind == MediaKind::Audio),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("detail-title")
                            .line_clamp(2)
                            .text_size(theme.text(Text::Heading))
                            .font_weight(FontWeight::MEDIUM)
                            .tip_label(title.clone(), Perch::Below)
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(theme.text(Text::Small))
                            .child(CallNumber::new(id.0).selected(true))
                            .child(div().truncate().text_color(tone).child(status_label)),
                    ),
            )
            .child(
                Button::new("detail-close")
                    .ghost()
                    .small()
                    .icon("icons/x.svg")
                    .tooltip("dl-close")
                    .on_click(cx.listener(|this, _, _, cx| this.deselect(cx))),
            );

        let open = self.details_open;
        let details_head = Button::new("details-toggle")
            .ghost()
            .small()
            .icon(match open {
                true => "icons/chevron-down.svg",
                false => "icons/chevron-right.svg",
            })
            .label(t!("dl-detail-section"))
            .expanded(open)
            .on_click(cx.listener(|this, _, _, cx| {
                this.details_open = !this.details_open;
                cx.notify();
            }));
        let details = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .pt(px(12.))
            .border_t_1()
            .border_color(theme.border)
            .child(div().flex().ml(px(-8.)).child(details_head))
            .when(open, |this| {
                this.child(self.fact("d-url", t!("dl-detail-url"), s.url.clone(), true, true, cx))
                    .children(s.file_path.clone().map(|path| {
                        self.fact("d-file", t!("dl-detail-file"), path, true, true, cx)
                    }))
                    .child(self.fact(
                        "d-format",
                        t!("dl-detail-format"),
                        s.format_label.clone().unwrap_or_else(|| "-".into()),
                        false,
                        false,
                        cx,
                    ))
                    .children(size_fact(s).map(|size| {
                        self.fact("d-size", t!("dl-detail-size"), size, false, false, cx)
                    }))
                    .child(self.fact(
                        "d-options",
                        t!("dl-detail-options"),
                        s.preset.clone(),
                        true,
                        false,
                        cx,
                    ))
                    .children(cookies.clone().map(|used| {
                        self.fact("d-cookies", t!("dl-detail-cookies"), used, false, false, cx)
                    }))
                    .child(self.fact(
                        "d-attempts",
                        t!("dl-detail-attempts"),
                        s.attempts.to_string(),
                        false,
                        false,
                        cx,
                    ))
            });

        let fix_note = self.fix_note(id, cx);
        let error = s.error_code.as_ref().map(|code| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(10.))
                .py(px(8.))
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.danger.opacity(0.5))
                .bg(theme.danger.opacity(0.08))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_color(theme.danger)
                        .child(
                            svg()
                                .path(icons::path("icons/circle-alert.svg"))
                                .size(px(14.))
                                .flex_none()
                                .text_color(theme.danger),
                        )
                        .child(error_text_of(code)),
                )
                .children(s.error_message.clone().map(|m| {
                    div()
                        .line_clamp(4)
                        .font_family(MONO)
                        .text_size(theme.text(Text::Small))
                        .text_color(pencil)
                        .child(m)
                }))
        });

        let logs: Vec<_> = self
            .app
            .logs
            .read(cx)
            .for_job(id)
            .map(|l| (format::clock(l.at), l.message.clone()))
            .collect();
        let skip = logs.len().saturating_sub(TAIL);
        let empty = logs.is_empty();
        let tail = Scroller::new("detail-log", &self.log_bar)
            .fade_ends(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_px()
                    .children(logs.into_iter().skip(skip).map(|(time, text)| {
                        div()
                            .flex()
                            .gap(px(8.))
                            .font_family(MONO)
                            .text_size(theme.text(Text::Small))
                            .child(div().flex_none().text_color(pencil).child(time))
                            .child(div().min_w_0().flex_1().child(text))
                    })),
            );
        let log = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(theme.metrics.control_small)
                    .text_color(pencil)
                    .child(
                        svg()
                            .path(icons::path("icons/list.svg"))
                            .size(px(14.))
                            .text_color(pencil),
                    )
                    .child(t!("dl-detail-log")),
            )
            .child(
                div()
                    .h(px(180.))
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary.opacity(0.5))
                    .p(px(8.))
                    .map(|this| match empty {
                        true => this.text_color(pencil).child(t!("dl-detail-nolog")),
                        false => this.child(tail),
                    }),
            );

        let body = Scroller::new("detail-scroll", &self.detail_bar).child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .p(theme.metrics.gutter)
                .font_features(tabular())
                .child(header)
                .child(actions)
                .children(error)
                .children(fix_note)
                .child(details)
                .child(log),
        );
        let panel = Panel::new("detail-panel", Side::Right, px(PANEL))
            .limits(px(280.), px(WIDEST))
            .fill(fill)
            .border_color(theme.border)
            .bg(theme.background)
            .child(div().size_full().child(body));
        // in the flow beside the list, never over it: the list narrows (its columns fold) as the
        // panel grows out of the edge
        let span = match ui::motion::animates(cx) {
            true => SLIDE,
            false => Duration::from_millis(1),
        };
        div()
            .id("detail-slide")
            .flex()
            .h_full()
            .overflow_hidden()
            .map(|this| match fill {
                true => this.flex_1().min_w_0(),
                false => this.flex_none(),
            })
            .child(panel)
            .with_animation(
                ("detail-slide", usize::from(fill)),
                Animation::new(span).with_easing(ease_out_cubic),
                move |this, delta| match fill {
                    true => this.opacity(delta),
                    false => this.max_w(px(WIDEST * delta)),
                },
            )
            .into_any_element()
    }
}

/// The file's size, and after a fix what it was before and how much it changed.
fn size_fact(s: &domain::DownloadSummary) -> Option<String> {
    let size = s.file_size?;
    Some(match s.size_before_fix.filter(|b| *b > 0 && *b != size) {
        Some(before) => {
            let change = (size as f64 / before as f64 - 1.) * 100.;
            t!(
                "dl-detail-size-fixed",
                size = crate::format::bytes(size),
                before = crate::format::bytes(before),
                change = format!(
                    "{}{:.0}%",
                    if change < 0. { "−" } else { "+" },
                    change.abs()
                )
            )
            .to_string()
        }
        None => crate::format::bytes(size),
    })
}
