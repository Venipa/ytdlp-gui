use gpui::{Hsla, rgb};

use crate::theme::Theme;

/// The colours of the first-run onboarding: a warm cream surface with coral and red. They do
/// not follow the app theme's own tokens, because the wizard has a look of its own, so they
/// live as one named set the wizard reads. A set is a plain value: `Brand::of` picks the one for
/// the active theme's appearance, from `light` or `dark`.
#[derive(Clone, Copy)]
pub struct Brand {
    /// The top and the bottom of the backdrop.
    pub cream_top: Hsla,
    pub cream_bottom: Hsla,
    /// The two glows laid over the backdrop, their alpha being how strong they are at the centre.
    pub glow: [Hsla; 2],
    /// The lighter coral of the gradient, and the colour of the drifting leaves.
    pub coral: Hsla,
    /// The red of the gradient's end and the active step dot.
    pub red: Hsla,
    /// A step dot that is still to come.
    pub dot_off: Hsla,
    /// Titles.
    pub title: Hsla,
    /// The text of an outline button and of a row.
    pub outline_text: Hsla,
    /// The line under a title, and a row's detail.
    pub detail: Hsla,
    /// The "Step N of 3" label.
    pub step_label: Hsla,
    /// The text of a quiet link button.
    pub link: Hsla,
    /// The border of an outline button and of a row.
    pub outline: Hsla,
    /// The rule beside "or", and the "or" between the rules.
    pub divider: Hsla,
    pub muted: Hsla,
    /// The frosted card's fill and border.
    pub card: Hsla,
    pub card_border: Hsla,
    /// The shadows under the card and under the badge, their alpha being their strength.
    pub card_shadow: Hsla,
    pub badge_shadow: Hsla,
    /// A row's translucent fill, and the fill it lifts to under the pointer.
    pub row: Hsla,
    pub row_hover: Hsla,
    /// The text on the gradient.
    pub on_accent: Hsla,
    /// The round badge's fill, from its lit corner to its far edge.
    pub badge_light: Hsla,
    pub badge_shade: Hsla,
    /// The faint ring on the badge's rim.
    pub badge_ring: Hsla,
}

impl Brand {
    /// The set for `theme`'s appearance.
    pub fn of(theme: &Theme) -> Self {
        match theme.background.l < 0.5 {
            true => Self::dark(),
            false => Self::light(),
        }
    }

    /// Cream, plum-brown and coral.
    pub fn light() -> Self {
        let red = hex(0xe2483f);
        let white = Hsla::white();
        Self {
            cream_top: hex(0xfff8f2),
            cream_bottom: hex(0xffeadc),
            glow: [hex(0xffb38a).opacity(0.45), hex(0xff8a65).opacity(0.25)],
            coral: hex(0xff8a65),
            red,
            dot_off: hex(0xf1d9cc),
            title: hex(0x2b1b1d),
            outline_text: hex(0x5a2d26),
            detail: hex(0x8a6a62),
            step_label: hex(0xa3786a),
            link: hex(0xb0453a),
            outline: hex(0xf0c9b8),
            divider: hex(0xf0d6ca),
            muted: hex(0xb08a7e),
            card: white.opacity(0.72),
            card_border: red.opacity(0.12),
            card_shadow: hex(0xa04628).opacity(0.15),
            badge_shadow: hex(0xa04628).opacity(0.22),
            row: white.opacity(0.6),
            row_hover: white.opacity(0.9),
            on_accent: white,
            badge_light: white,
            badge_shade: hex(0xffe6d8),
            badge_ring: hex(0xff8a65).opacity(0.18),
        }
    }

    /// Ember plum: a deep plum-brown ground with a faint coral glow, a warm dark frosted card and
    /// cream text. The gradient and the leaves stay as they are in light.
    pub fn dark() -> Self {
        let coral = hex(0xff8a65);
        let white = Hsla::white();
        let black = Hsla::black();
        Self {
            cream_top: hex(0x221719),
            cream_bottom: hex(0x140d0f),
            glow: [coral.opacity(0.22), hex(0xe2483f).opacity(0.16)],
            coral,
            red: hex(0xe2483f),
            dot_off: hex(0x3a2c2c),
            title: hex(0xfbefe9),
            outline_text: hex(0xf5e9e4),
            detail: hex(0xb9a19a),
            step_label: hex(0xa08a83),
            link: hex(0xffb39a),
            outline: coral.opacity(0.28),
            divider: white.opacity(0.1),
            muted: hex(0x8a7a75),
            card: hex(0x1e1618).opacity(0.72),
            card_border: coral.opacity(0.16),
            card_shadow: black.opacity(0.5),
            badge_shadow: black.opacity(0.5),
            row: white.opacity(0.03),
            row_hover: white.opacity(0.08),
            on_accent: white,
            badge_light: hex(0x3a2a2c),
            badge_shade: hex(0x221719),
            badge_ring: coral.opacity(0.14),
        }
    }
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}
