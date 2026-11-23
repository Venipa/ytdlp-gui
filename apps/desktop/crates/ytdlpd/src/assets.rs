use std::borrow::Cow;

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};

include!(concat!(env!("OUT_DIR"), "/fonts.rs"));

/// Embedded fonts exposed to GPUI alongside the active icon pack.
pub struct Assets;

impl Assets {
    /// Registers the bundled IBM Plex faces with the text system.
    pub fn load_fonts(&self, cx: &App) -> Result<()> {
        let embedded = FONTS
            .iter()
            .map(|(_, bytes)| Cow::Borrowed(*bytes))
            .collect();

        cx.text_system().add_fonts(embedded)
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = icons::asset(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some((_, bytes)) = FONTS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        log::warn!("assets: {path} is not registered");
        Ok(None)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut listed = icons::Assets.list(path)?;
        listed.extend(
            FONTS
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::new_static(name)),
        );
        Ok(listed)
    }
}
