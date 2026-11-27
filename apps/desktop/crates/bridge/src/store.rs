//! `extension.json`: paired devices (token hash + pinned Origin), one file per data dir.
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Renames an unreadable file to `extension.json.bad` so a later save cannot wipe the pairings in it.
fn set_aside(path: &Path, why: &str) {
    let bad = path.with_extension("json.bad");
    match std::fs::rename(path, &bad) {
        Ok(()) => log::warn!(
            "bridge: unreadable extension.json ({why}), kept as {}",
            bad.display()
        ),
        Err(e) => {
            log::error!("bridge: unreadable extension.json ({why}) and cannot move it aside: {e}")
        }
    }
}

/// A paired browser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    /// Lowercase hex SHA-256 of the token.
    pub hash: String,
    /// Device name given at pairing.
    pub name: String,
    /// Origin seen at pairing; `auth` must come from the same one.
    pub origin: String,
    /// Unix seconds.
    pub created: i64,
    /// Unix seconds.
    pub last_seen: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    devices: Vec<Stored>,
}

/// The device list and its file.
pub(crate) struct Store {
    path: PathBuf,
    pub(crate) devices: Vec<Stored>,
}

/// Hex SHA-256 of a token.
pub(crate) fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Unix seconds now.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Store {
    /// Reads `<dir>/extension.json`; a missing or unreadable file means no devices.
    pub(crate) fn load(dir: &Path) -> Self {
        let path = dir.join("extension.json");
        let devices = match std::fs::read(&path) {
            Ok(b) => match serde_json::from_slice::<File>(&b) {
                Ok(f) => f.devices,
                Err(e) => {
                    set_aside(&path, &e.to_string());
                    vec![]
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => {
                set_aside(&path, &e.to_string());
                vec![]
            }
        };
        Self { path, devices }
    }

    /// Writes the file (temp + rename; 0600 on unix; on Windows the data dir's ACLs apply).
    pub(crate) fn save(&self) -> Result<()> {
        let json = serde_json::to_vec_pretty(&File {
            devices: self.devices.clone(),
        })?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).context("cannot create the data folder")?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let mut f = open_private(&tmp).context("cannot write extension.json")?;
        f.write_all(&json).context("cannot write extension.json")?;
        f.sync_all().ok();
        std::fs::rename(&tmp, &self.path).context("cannot replace extension.json")
    }
}

#[cfg(unix)]
fn open_private(p: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(p)
}

#[cfg(not(unix))]
fn open_private(p: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create(p)
}
