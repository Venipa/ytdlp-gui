use gpui::{Pixels, SharedString, TextRun, Window, px};
use i18n::t;

const HEADER: f32 = 0.75;

pub const LEADING: f32 = 1.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    Square,
    Subtle,
    Rounded,
    Round,
}

impl Rounding {
    pub const ALL: [Self; 4] = [Self::Square, Self::Subtle, Self::Rounded, Self::Round];

    pub fn id(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Subtle => "subtle",
            Self::Rounded => "rounded",
            Self::Round => "round",
        }
    }

    pub fn label(self) -> SharedString {
        match self {
            Self::Square => t!("corners-square"),
            Self::Subtle => t!("corners-subtle"),
            Self::Rounded => t!("corners-rounded"),
            Self::Round => t!("corners-round"),
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "square" => Self::Square,
            "rounded" => Self::Rounded,
            "round" => Self::Round,
            _ => Self::Subtle,
        }
    }

    pub fn radius(self) -> Pixels {
        match self {
            Self::Square => px(0.),
            Self::Subtle => px(2.),
            Self::Rounded => px(6.),
            Self::Round => px(12.),
        }
    }
}

/// Sizes on a 4px grid. The control system has three heights and every control in one row shares
/// one of them: `control_small` (24 at the 12px base: icon buttons over rows, tags), `control`
/// (28: buttons, chips, inputs, pickers, steppers, search) and `field` (36: the intake and its
/// button). Pages keep `gutter` (16) to their sides; groups inside a page are `gutter` apart.
#[derive(Clone, Copy)]
pub struct Metrics {
    pub row: Pixels,
    pub header: Pixels,
    pub pad: Pixels,
    /// The side margin of a page and the gap between its groups.
    pub gutter: Pixels,
    pub inset: Pixels,
    pub control: Pixels,
    pub control_small: Pixels,
    pub field: Pixels,
    pub title_bar: Pixels,
    pub player_bar: Pixels,
    pub list_row: Pixels,
    pub thumb: Pixels,
    pub cover: Pixels,
    /// The widest a page grows, its side insets included. A wider content area centres the
    /// page and gives what is left to its side margins.
    pub page: Pixels,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new(px(14.))
    }
}

impl Metrics {
    pub fn new(base: Pixels) -> Self {
        let text = base / px(1.);
        let roomy = |step: f32, lines: f32| px(step.max((text * lines).round()));
        let row = roomy(42., 2.4);

        Self {
            row,
            header: px((row / px(1.) * HEADER).round()),
            pad: px(8.),
            gutter: px(16.),
            inset: px(24.),
            control: roomy(28., 2.34),
            control_small: roomy(24., 2.),
            field: roomy(36., 3.),
            title_bar: roomy(36., 2.4),
            player_bar: roomy(76., 5.),
            list_row: roomy(52., 3.2),
            thumb: px(34.),
            cover: px(140.),
            page: px(1280.),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Text {
    Tiny,
    Small,
    Label,
    Body,
    /// A dialog's title.
    Heading,
    Large,
    Title,
    Display,
}

impl Text {
    pub(crate) fn ratio(self) -> f32 {
        match self {
            // at the 12px base Tiny is 10 (pills, badges) and Small 11 (meta), the least that
            // IBM Plex Sans stays readable at
            Self::Tiny => 0.84,
            Self::Small => 0.92,
            Self::Label => 0.92,
            Self::Body => 1.,
            Self::Heading => 1.15,
            Self::Large => 1.38,
            Self::Title => 1.69,
            Self::Display => 2.15,
        }
    }
}

const INSET: f32 = 0.25;
const BORDER: Pixels = px(1.);

pub fn snapped(value: Pixels, window: &Window) -> Pixels {
    let scale = window.scale_factor();
    px((value / px(1.) * scale).round() / scale)
}

pub fn text_width(text: impl Into<SharedString>, window: &Window) -> Pixels {
    let text = text.into();
    if text.is_empty() {
        return Pixels::ZERO;
    }

    let style = window.text_style();
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let size = style.font_size.to_pixels(window.rem_size());

    window
        .text_system()
        .shape_line(text, size, &[run], None)
        .width
}

pub fn tucked(radius: Pixels, window: &Window) -> Pixels {
    tucked_in(radius, window.rem_size())
}

/// The radius of something one inset (and the border) inside a surface rounded at `radius`, so
/// the two curves run parallel. `rem` is the window's rem size, the theme's font size.
pub fn tucked_in(radius: Pixels, rem: Pixels) -> Pixels {
    concentric(radius, rem * INSET + BORDER)
}

/// The radius of something `inset` inside a surface rounded at `radius`, such as a cover in a
/// row's hover, so the two curves run parallel. A full-round surface keeps its contents round
/// and a square one keeps them square.
pub fn concentric(radius: Pixels, inset: Pixels) -> Pixels {
    (radius - inset).max(Pixels::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings row's hover sits one inset and the border inside its card, so for every
    /// Corners choice its radius is the card's less that, never below zero, and Square stays
    /// square.
    #[test]
    fn tucked_rows_nest_in_every_card() {
        let rem = px(16.);
        for rounding in Rounding::ALL {
            let card = crate::modal::corners(rounding.radius());
            let row = tucked_in(card, rem);
            assert!(row >= Pixels::ZERO && row <= card);
            match card > rem * INSET + BORDER {
                true => assert_eq!(row, card - rem * INSET - BORDER),
                false => assert_eq!(row, Pixels::ZERO),
            }
        }
        assert_eq!(
            tucked_in(crate::modal::corners(Rounding::Square.radius()), rem),
            Pixels::ZERO
        );
    }
}
