mod link;
mod navigation;

pub use link::Link;
pub use navigation::{Navigation, NavigationEvent};

use gpui::{App, AppContext as _, Entity, Global};

/// One page of settings, in the order the settings side nav lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Downloads,
    Formats,
    Network,
    Tools,
    Appearance,
    Updates,
    About,
}

impl SettingsPage {
    /// Every page, top to bottom.
    pub const ALL: [Self; 8] = [
        Self::General,
        Self::Downloads,
        Self::Formats,
        Self::Network,
        Self::Tools,
        Self::Appearance,
        Self::Updates,
        Self::About,
    ];

    /// The id the page is announced and stored by.
    pub fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Downloads => "downloads",
            Self::Formats => "formats",
            Self::Network => "network",
            Self::Tools => "tools",
            Self::Appearance => "appearance",
            Self::Updates => "updates",
            Self::About => "about",
        }
    }

    /// The Fluent key of the page's name.
    pub fn key(self) -> &'static str {
        match self {
            Self::General => "set-page-general",
            Self::Downloads => "set-page-downloads",
            Self::Formats => "set-page-formats",
            Self::Network => "set-page-network",
            Self::Tools => "set-page-tools",
            Self::Appearance => "set-page-appearance",
            Self::Updates => "set-page-updates",
            Self::About => "set-page-about",
        }
    }

    /// The page an id names.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.id() == id)
    }
}

/// A page of the app. Frozen at the end of Phase 0a: adding a variant is a contract change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// The Getting ready screen shown while the tools are fetched.
    Welcome,
    Downloads,
    Logs,
    Settings(SettingsPage),
}

impl Destination {
    /// Whether the title-bar entry leading to `self` is lit while `other` is open. Every
    /// settings page lights the one Settings entry.
    pub fn same_section(&self, other: &Destination) -> bool {
        (self.in_settings() && other.in_settings()) || self == other
    }

    /// Whether this destination is a settings page.
    pub fn in_settings(&self) -> bool {
        matches!(self, Destination::Settings(_))
    }
}

#[derive(Clone)]
struct Router(Entity<Navigation>);

impl Global for Router {}

/// Starts navigation on `start`.
pub fn init(start: Destination, cx: &mut App) {
    let navigation = cx.new(|_| Navigation::new(start));
    cx.set_global(Router(navigation));
}

/// The navigation entity.
pub fn trail(cx: &App) -> Entity<Navigation> {
    cx.global::<Router>().0.clone()
}

/// Opens `destination`.
pub fn navigate(destination: Destination, cx: &mut App) {
    trail(cx).update(cx, |navigation, cx| navigation.go(destination, cx));
}

/// Replaces the current destination so Back and Forward do not revisit a page that disappeared.
pub fn replace(destination: Destination, cx: &mut App) {
    trail(cx).update(cx, |navigation, cx| navigation.replace(destination, cx));
}

/// Goes back one page.
pub fn back(cx: &mut App) {
    trail(cx).update(cx, |navigation, cx| navigation.back(cx));
}

/// Goes forward one page.
pub fn forward(cx: &mut App) {
    trail(cx).update(cx, |navigation, cx| navigation.forward(cx));
}

/// Opens settings on the page they last showed, or on General the first time.
pub fn open_settings(cx: &mut App) {
    trail(cx).update(cx, |navigation, cx| navigation.open_settings(cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pages_round_trip_and_light_one_entry() {
        for page in SettingsPage::ALL {
            assert_eq!(SettingsPage::from_id(page.id()), Some(page));
        }
        let a = Destination::Settings(SettingsPage::General);
        let b = Destination::Settings(SettingsPage::About);
        assert!(a.same_section(&b));
        assert!(!a.same_section(&Destination::Logs));
    }
}
