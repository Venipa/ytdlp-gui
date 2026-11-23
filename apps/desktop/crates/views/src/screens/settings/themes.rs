//! The theme picker of Settings > Appearance, after maple's: one card per theme on a rail that
//! scrolls sideways, each card a miniature of a window painted in that theme. The built-in Ledger
//! System, Dark and Light come first, then the theme files bundled from maple
//! (`assets/themes`): a folder is a family, each file in it a variant, a loose file a family of
//! one. A file names its colours over the Ledger dark palette (`ui::ThemeOverrides`).

use std::sync::LazyLock;

use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, SharedString, div, linear_color_stop, linear_gradient, px};
use ui::{
    ActiveTheme as _, Menu, MenuItem, OptionCard, Picker, Rail, Theme, ThemeKind, ThemeOverrides,
};

use super::SettingsView;
use super::miniature::{CARD, SLICES, THUMB, sliced};

/// The popover key of the variant menu, which one card at a time owns.
const VARIANTS: &str = "theme-variants";
/// The side of a variant's swatch in the menu.
const SWATCH: f32 = 14.;

/// The bundled theme files by id, in the order the page lists them (maple's order).
const BUNDLED: [(&str, &str); 15] = [
    (
        "catppuccin/latte",
        include_str!("../../../../../assets/themes/catppuccin/latte.json"),
    ),
    (
        "catppuccin/frappe",
        include_str!("../../../../../assets/themes/catppuccin/frappe.json"),
    ),
    (
        "catppuccin/macchiato",
        include_str!("../../../../../assets/themes/catppuccin/macchiato.json"),
    ),
    (
        "catppuccin/mocha",
        include_str!("../../../../../assets/themes/catppuccin/mocha.json"),
    ),
    (
        "rose-pine/main",
        include_str!("../../../../../assets/themes/rose-pine/main.json"),
    ),
    (
        "rose-pine/moon",
        include_str!("../../../../../assets/themes/rose-pine/moon.json"),
    ),
    (
        "rose-pine/dawn",
        include_str!("../../../../../assets/themes/rose-pine/dawn.json"),
    ),
    (
        "gruvbox/dark",
        include_str!("../../../../../assets/themes/gruvbox/dark.json"),
    ),
    (
        "gruvbox/dark-hard",
        include_str!("../../../../../assets/themes/gruvbox/dark-hard.json"),
    ),
    (
        "gruvbox/light",
        include_str!("../../../../../assets/themes/gruvbox/light.json"),
    ),
    (
        "gruvbox/light-hard",
        include_str!("../../../../../assets/themes/gruvbox/light-hard.json"),
    ),
    (
        "solarized/dark",
        include_str!("../../../../../assets/themes/solarized/dark.json"),
    ),
    (
        "solarized/light",
        include_str!("../../../../../assets/themes/solarized/light.json"),
    ),
    (
        "nord",
        include_str!("../../../../../assets/themes/nord.json"),
    ),
    (
        "dracula",
        include_str!("../../../../../assets/themes/dracula.json"),
    ),
];

/// One theme file. Its id (`catppuccin/mocha`, `nord`) is what settings.json stores.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub id: &'static str,
    /// The name inside its family, such as "Mocha"; a family of one uses the whole title.
    pub name: String,
    /// The name the file gives, such as "Catppuccin Mocha".
    title: String,
    pub overrides: ThemeOverrides,
}

impl Variant {
    /// The colours the variant paints with, enough to draw its miniature.
    pub fn palette(&self) -> Theme {
        Theme::for_kind(ThemeKind::Dark).with_overrides(&self.overrides)
    }

    fn is_light(&self) -> bool {
        self.palette().background.l >= 0.5
    }
}

/// A theme as the page offers it: a name over one or more variants.
#[derive(Clone, Debug, PartialEq)]
pub struct Family {
    pub id: &'static str,
    pub name: String,
    /// Never empty.
    pub variants: Vec<Variant>,
}

static FAMILIES: LazyLock<Vec<Family>> = LazyLock::new(|| {
    let mut families: Vec<Family> = Vec::new();
    for (id, text) in BUNDLED {
        let variant = match parse(id, text) {
            Ok(variant) => variant,
            Err(error) => {
                log::error!("settings: cannot load bundled theme {id}: {error}");
                continue;
            }
        };
        let family = id.split_once('/').map_or(id, |(family, _)| family);
        match families.iter_mut().find(|f| f.id == family) {
            Some(f) => f.variants.push(variant),
            None => families.push(Family {
                id: family,
                name: String::new(),
                variants: vec![variant],
            }),
        }
    }
    families.iter_mut().for_each(name);
    families
});

/// Every bundled theme family.
pub fn families() -> &'static [Family] {
    &FAMILIES
}

/// The family and variant the theme id `id` names, if it is a theme file.
pub fn variant(id: &str) -> Option<(&'static Family, &'static Variant)> {
    families()
        .iter()
        .find_map(|f| f.variants.iter().find(|v| v.id == id).map(|v| (f, v)))
}

/// The colours the theme id lays over the Ledger palette; none for the built-in themes and for an
/// id no file has (it falls back to Ledger dark).
pub fn overrides(id: &str) -> ThemeOverrides {
    variant(id)
        .map(|(_, v)| v.overrides.clone())
        .unwrap_or_default()
}

/// The variant a click on `family` picks: the chosen one when it is in the family, else the first
/// whose lightness matches `light`, else the first.
fn pick<'a>(family: &'a Family, chosen: &str, light: bool) -> &'a Variant {
    family
        .variants
        .iter()
        .find(|v| v.id == chosen)
        .or_else(|| family.variants.iter().find(|v| v.is_light() == light))
        .unwrap_or(&family.variants[0])
}

/// Reads a theme file: `name`, `author`, `version` 1 and a `theme` object of known colours.
fn parse(id: &'static str, text: &str) -> Result<Variant, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let title = value["name"].as_str().unwrap_or_default().trim().to_owned();
    if title.is_empty() || value["version"].as_u64() != Some(1) {
        return Err("needs a name and version 1".into());
    }
    let theme = value["theme"]
        .as_object()
        .ok_or("theme must be an object")?;
    if let Some(field) = theme.keys().find(|k| !ThemeOverrides::is_color(k)) {
        return Err(format!("`{field}` is not a theme color"));
    }
    let overrides: ThemeOverrides =
        serde_json::from_value(value["theme"].clone()).map_err(|e| e.to_string())?;
    if let Some(field) = overrides.invalid_color() {
        return Err(format!("theme.{field} is not a valid color"));
    }
    Ok(Variant {
        id,
        name: title.clone(),
        title,
        overrides,
    })
}

/// Names a family by the words all its variant titles start with ("Catppuccin"), and each variant
/// by the rest ("Mocha"). A family of one keeps its whole title.
fn name(family: &mut Family) {
    let titles: Vec<Vec<String>> = family
        .variants
        .iter()
        .map(|v| v.title.split_whitespace().map(str::to_owned).collect())
        .collect();
    let shared = match titles.as_slice() {
        [] | [_] => 0,
        [first, rest @ ..] => (0..first.len())
            .take_while(|&i| rest.iter().all(|t| t.get(i) == first.get(i)))
            .count(),
    };
    family.name = match (shared, titles.as_slice()) {
        (_, [only]) => only.join(" "),
        (0, _) | (_, []) => family.id.to_owned(),
        (shared, [first, ..]) => first[..shared].join(" "),
    };
    if titles.len() > 1 {
        for (variant, words) in family.variants.iter_mut().zip(&titles) {
            if words.len() > shared {
                variant.name = words[shared..].join(" ");
            }
        }
    }
}

/// One card of the rail: a built-in theme or a family of theme files.
enum Entry {
    Kind(ThemeKind),
    Family(&'static Family),
}

impl SettingsView {
    /// The theme cards on one rail: System, Dark and Light, then every bundled family.
    pub(super) fn theme_cards(&self, chosen: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let chosen_family = variant(chosen).map(|(f, _)| f.id);
        let entries = ThemeKind::ALL
            .into_iter()
            .map(Entry::Kind)
            .chain(families().iter().map(Entry::Family));
        let cards = entries
            .map(|entry| match entry {
                Entry::Kind(kind) => self.kind_card(kind, kind.id() == chosen, cx),
                Entry::Family(family) => {
                    self.family_card(family, chosen_family == Some(family.id), chosen, cx)
                }
            })
            .collect::<Vec<_>>();
        // the ring around the chosen card stands outside it, so the rail leaves it room
        let room = theme.metrics.pad;
        Rail::new("theme-cards")
            .py(room)
            .arrows_at(room + THUMB / 2.)
            .children(cards)
            .into_any_element()
    }

    fn kind_card(&self, kind: ThemeKind, chosen: bool, cx: &mut Context<Self>) -> AnyElement {
        let palettes = match kind {
            ThemeKind::System => vec![Theme::dark(), Theme::light()],
            kind => vec![Theme::for_kind(kind)],
        };
        let label = i18n::lookup(&format!("set-theme-{}", kind.id()), None);
        OptionCard::new(SharedString::from(format!("theme-{}", kind.id())))
            .role(Role::RadioButton)
            .aria_label(label.clone())
            .w(CARD)
            .thumb_height(THUMB)
            .thumb(sliced(&palettes, cx.theme().radius, 1.))
            .label(label)
            .selected(chosen)
            .on_click(self.edit(move |s| s.theme = kind.id().to_owned()))
            .into_any_element()
    }

    /// A family's card. A family of several variants names the variant a click picks after its
    /// own name, and that line opens the menu of all of them.
    fn family_card(
        &self,
        family: &'static Family,
        chosen: bool,
        current: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let light = ThemeKind::System.resolved(cx) == ThemeKind::Light;
        let picked = pick(family, current, light);
        let several = family.variants.len() > 1;
        let label = match several {
            true => format!("{} {}", family.name, picked.name),
            false => family.name.clone(),
        };
        let palettes = family
            .variants
            .iter()
            .take(SLICES)
            .map(Variant::palette)
            .collect::<Vec<_>>();
        let id = picked.id;
        let card = OptionCard::new(SharedString::from(format!("theme-family-{}", family.id)))
            .role(Role::RadioButton)
            .aria_label(SharedString::from(label.clone()))
            .w(CARD)
            .thumb_height(THUMB)
            .thumb(sliced(&palettes, cx.theme().radius, 1.))
            .label(label)
            .selected(chosen)
            .on_click(self.edit(move |s| s.theme = id.to_owned()));
        match several {
            true => self.with_variants(card, family, picked.id, cx),
            false => card,
        }
        .into_any_element()
    }

    /// Gives a family's card the menu of its variants, `picked` checked, opened from its name.
    /// Only the family whose menu opened last owns the popover.
    fn with_variants(
        &self,
        card: OptionCard,
        family: &'static Family,
        picked: &'static str,
        cx: &mut Context<Self>,
    ) -> OptionCard {
        if self.theme_variants != Some(family.id) {
            return card.on_variants(cx.listener(move |this, _, _, cx| {
                this.theme_variants = Some(family.id);
                this.popovers.open(VARIANTS);
                cx.notify();
            }));
        }
        let items = family.variants.iter().map(|variant| {
            let id = variant.id;
            let settings = self.settings();
            MenuItem::new(
                SharedString::from(format!("theme-variant-{id}")),
                variant.name.clone(),
            )
            .leading(swatch(&variant.palette()))
            .checked(picked == id)
            .on_click(cx.listener(move |this, _, _, cx| {
                settings.update(cx, |s, cx| s.update(|s| s.theme = id.to_owned(), cx));
                this.popovers.close();
                cx.notify();
            }))
        });
        let menu = Menu::new("theme-variants-menu")
            .w(Picker::NARROW)
            .items(items);
        card.variant_menu(VARIANTS, &self.popovers, menu)
    }
}

/// A variant's swatch in the menu: a circle cut on the diagonal into its page colour and its
/// accent, the cut blended over one pixel so its edge stays smooth.
fn swatch(palette: &Theme) -> Div {
    let blend = 0.5 / SWATCH;
    div()
        .size(px(SWATCH))
        .flex_none()
        .rounded_full()
        .bg(linear_gradient(
            135.,
            linear_color_stop(palette.background, 0.5 - blend),
            linear_color_stop(palette.primary, 0.5 + blend),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_theme_loads_into_named_families() {
        let all: usize = families().iter().map(|f| f.variants.len()).sum();
        assert_eq!(all, BUNDLED.len());
        let (family, mocha) = variant("catppuccin/mocha").unwrap();
        assert_eq!(
            (family.name.as_str(), mocha.name.as_str()),
            ("Catppuccin", "Mocha")
        );
        assert_eq!(variant("nord").unwrap().0.name, "Nord");
        assert!(overrides("catppuccin/latte").background.is_some());
        assert_eq!(overrides("dark"), ThemeOverrides::default());
        assert_eq!(overrides("gone/theme"), ThemeOverrides::default());
    }

    #[test]
    fn a_family_click_keeps_the_chosen_variant_else_matches_the_system() {
        let family = variant("catppuccin/latte").unwrap().0;
        assert_eq!(
            pick(family, "catppuccin/frappe", true).id,
            "catppuccin/frappe"
        );
        assert_eq!(pick(family, "dark", true).id, "catppuccin/latte");
        assert_eq!(pick(family, "dark", false).id, "catppuccin/frappe");
    }

    #[test]
    fn a_bad_file_is_refused() {
        assert!(
            parse(
                "x",
                r##"{"name":"X","author":"a","version":1,"theme":{"nope":"#fff"}}"##
            )
            .is_err()
        );
        assert!(parse("x", r#"{"name":"X","author":"a","version":2,"theme":{}}"#).is_err());
        assert!(
            parse(
                "x",
                r#"{"name":"X","author":"a","version":1,"theme":{"background":"zz"}}"#
            )
            .is_err()
        );
    }
}
