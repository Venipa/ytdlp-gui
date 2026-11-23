use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use gpui::{AssetSource, SharedString};

pub const BASE: &str = "lucide";
pub const SAMPLES: &[&str] = &["house", "heart-filled", "play-filled", "shuffle", "search"];

const SHARED: &str = "common";
const KIND: &str = ".svg";

include!(concat!(env!("OUT_DIR"), "/packs.rs"));

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

pub struct Icon {
    pub name: &'static str,
    pub path: &'static str,
    pub bytes: &'static [u8],
}

pub struct Pack {
    pub id: &'static str,
    start: usize,
    end: usize,
}

impl Pack {
    pub fn title(&self) -> SharedString {
        let mut letters = self.id.chars();
        match letters.next() {
            Some(first) => first
                .to_uppercase()
                .chain(letters)
                .map(|c| if c == '-' { ' ' } else { c })
                .collect::<String>()
                .into(),
            None => SharedString::default(),
        }
    }

    pub fn icons(&self) -> &'static [Icon] {
        &ICONS[self.start..self.end]
    }

    pub fn icon(&self, name: &str) -> Option<&'static Icon> {
        let icons = self.icons();
        let at = icons.binary_search_by(|icon| icon.name.cmp(name)).ok()?;
        icons.get(at)
    }
}

pub fn packs() -> impl Iterator<Item = &'static Pack> {
    PACKS.iter().filter(|pack| pack.id != SHARED)
}

pub fn pack(id: &str) -> Option<&'static Pack> {
    packs().find(|pack| pack.id == id)
}

pub fn active() -> &'static Pack {
    let at = ACTIVE.load(Ordering::Relaxed);
    PACKS.get(at).unwrap_or(&PACKS[0])
}

pub fn set(id: &str) {
    let chosen = PACKS
        .iter()
        .position(|pack| pack.id == id && pack.id != SHARED)
        .or_else(|| PACKS.iter().position(|pack| pack.id == BASE))
        .unwrap_or_default();

    ACTIVE.store(chosen, Ordering::Relaxed);
}

pub fn path(icon: impl AsRef<str>) -> SharedString {
    shown(active(), icon)
}

pub fn shown(pack: &Pack, icon: impl AsRef<str>) -> SharedString {
    let icon = icon.as_ref();
    let name = stem(icon);
    match chased(pack, name) {
        Some(icon) => SharedString::new_static(icon.path),
        None => {
            log::warn!("icons: {name} is in no pack");
            SharedString::from(icon.to_owned())
        }
    }
}

pub fn asset(path: &str) -> Option<&'static [u8]> {
    let (id, file) = path.strip_prefix("icons/")?.split_once('/')?;
    let folder = PACKS.iter().find(|pack| pack.id == id)?;
    folder.icon(stem(file)).map(|icon| icon.bytes)
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(asset(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|icon| icon.path.starts_with(path))
            .map(|icon| SharedString::new_static(icon.path))
            .collect())
    }
}

fn chased(pack: &Pack, name: &str) -> Option<&'static Icon> {
    pack.icon(name)
        .or_else(|| folder(SHARED)?.icon(name))
        .or_else(|| folder(BASE)?.icon(name))
}

fn folder(id: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|pack| pack.id == id)
}

fn stem(icon: &str) -> &str {
    let name = icon.rsplit('/').next().unwrap_or(icon);
    name.strip_suffix(KIND).unwrap_or(name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_pack_has_its_own_pin() {
        for pack in super::packs() {
            assert!(pack.icon("pin").is_some(), "{} borrows its pin", pack.id);
            let path = super::shown(pack, "icons/pin.svg");
            assert!(
                super::asset(&path).is_some(),
                "pin missing from {}",
                pack.id
            );
        }
    }
}
