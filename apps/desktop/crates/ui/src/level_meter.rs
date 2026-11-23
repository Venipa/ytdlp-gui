use gpui::prelude::*;
use gpui::{App, Div, Interactivity, Pixels, StyleRefinement, Window, div, px, relative};

use crate::theme::ActiveTheme as _;

/// The levels the meter spans, in dBFS, its left edge first. Full scale sits near the right, so
/// the clip zone past it is a narrow band.
const RANGE: (f32, f32) = (-48., 6.);
/// How tall the bar is unless the caller sets a height.
const THICKNESS: Pixels = px(4.);
/// How wide the peak tick is.
const TICK: Pixels = px(2.);
/// How strongly the clip zone tints the track where the level does not reach.
const CLIP_ZONE: f32 = 0.25;

/// A thin horizontal level meter in dBFS. It fills to the level, marks the held peak with a
/// tick, and draws everything past full scale in the danger color.
#[derive(IntoElement)]
pub struct LevelMeter {
    base: Div,
    level: f32,
    peak: Option<f32>,
}

impl LevelMeter {
    /// A meter filled to `level`, in dBFS.
    #[track_caller]
    pub fn new(level: f32) -> Self {
        Self {
            base: div(),
            level,
            peak: None,
        }
    }

    /// Marks `peak`, in dBFS, with a tick. A peak below the meter's range shows none.
    pub fn peak(mut self, peak: f32) -> Self {
        self.peak = Some(peak);
        self
    }
}

impl Styled for LevelMeter {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for LevelMeter {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for LevelMeter {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            mut base,
            level,
            peak,
        } = self;
        let theme = cx.theme();
        let overrides = std::mem::take(base.style());
        let full = share(0.);
        let level = share(level);
        let peak = peak.map(share).filter(|peak| *peak > 0.);
        let span = || div().absolute().top_0().bottom_0();

        let mut meter = base
            .relative()
            .flex_none()
            .h(THICKNESS)
            .rounded_full()
            .bg(theme.muted)
            .child(
                span()
                    .left(relative(full))
                    .right_0()
                    .rounded_r_full()
                    .bg(theme.danger.opacity(CLIP_ZONE)),
            )
            .child(
                span()
                    .left_0()
                    .w(relative(level.min(full)))
                    .rounded_l_full()
                    .bg(theme.primary),
            )
            .when(level > full, |this| {
                this.child(
                    span()
                        .left(relative(full))
                        .w(relative(level - full))
                        .rounded_r_full()
                        .bg(theme.danger),
                )
            })
            .when_some(peak, |this, peak| {
                this.child(span().left(relative(peak)).ml(-TICK / 2.).w(TICK).bg(
                    match peak > full {
                        true => theme.danger,
                        false => theme.foreground,
                    },
                ))
            });
        meter.style().refine(&overrides);
        meter
    }
}

/// How far along the meter `db` sits, from 0 at its left edge to 1 at its right.
fn share(db: f32) -> f32 {
    ((db - RANGE.0) / (RANGE.1 - RANGE.0)).clamp(0., 1.)
}
