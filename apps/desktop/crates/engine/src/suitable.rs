//! Does a URL belong to a yt-dlp extractor? A regex match only: no extractor runs, no network.
use anyhow::{Context as _, Result};
use pyo3::prelude::*;

use crate::runtime;

/// What kind of page a URL is, guessed from the extractor key (and the URL for tab extractors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UrlKind {
    /// One video.
    Video,
    /// A playlist, album, set or series.
    Playlist,
    /// A channel, user or profile page.
    Channel,
    /// Cannot tell.
    Unknown,
}

/// The extractor that claims a URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suitable {
    /// yt-dlp `ie_key()`, e.g. `Youtube`.
    pub key: String,
    /// yt-dlp `IE_NAME`, e.g. `youtube`.
    pub name: String,
    /// Page kind guess.
    pub kind: UrlKind,
}

/// Best-effort page kind. `Tab` extractors serve both playlists and channels, so the URL decides.
pub fn kind_of(key: &str, url: &str) -> UrlKind {
    let u = url.to_ascii_lowercase();
    let has = |s: &[&str]| s.iter().any(|p| u.contains(p));
    if key.ends_with("Tab") {
        return if has(&["/playlist", "list="]) {
            UrlKind::Playlist
        } else if has(&["/@", "/channel/", "/c/", "/user/", "/channels", "/videos"]) {
            UrlKind::Channel
        } else {
            UrlKind::Unknown
        };
    }
    let ends = |s: &[&str]| s.iter().any(|p| key.ends_with(p));
    if ends(&["Playlist", "Album", "Set", "Series", "Playlists"]) {
        UrlKind::Playlist
    } else if ends(&["Channel", "User", "Profile", "Uploads"]) {
        UrlKind::Channel
    } else {
        UrlKind::Video
    }
}

/// First extractor (in yt-dlp's own order) whose `_VALID_URL` matches `url`. `Generic` matches
/// everything and the `Known*` stubs (DRM, piracy, liability sites) only exist to fail, so both are
/// skipped. Needs [`runtime::init`]; the first call imports every extractor (about 0.5 s), later
/// ones take a few ms. Blocking: call from a worker thread.
pub fn suitable(url: &str) -> Result<Option<Suitable>> {
    if runtime::get().is_none() {
        anyhow::bail!("python runtime is not initialised");
    }
    runtime::with_python(|py| -> Result<Option<Suitable>> {
        let find = || -> PyResult<Option<(String, String)>> {
            let classes = py
                .import("yt_dlp.extractor")?
                .call_method0("gen_extractor_classes")?;
            for ie in classes.try_iter()? {
                let ie = ie?;
                let key: String = ie.call_method0("ie_key")?.extract()?;
                if key == "Generic" || key.starts_with("Known") {
                    continue;
                }
                if ie.call_method1("suitable", (url,))?.is_truthy()? {
                    let name: String = ie.getattr("IE_NAME")?.extract()?;
                    return Ok(Some((key, name)));
                }
            }
            Ok(None)
        };
        let found = find()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("cannot match extractors")?;
        Ok(found.map(|(key, name)| Suitable {
            kind: kind_of(&key, url),
            key,
            name,
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(
            kind_of("Youtube", "https://youtube.com/watch?v=1"),
            UrlKind::Video
        );
        assert_eq!(
            kind_of("YoutubeTab", "https://youtube.com/playlist?list=PL1"),
            UrlKind::Playlist
        );
        assert_eq!(
            kind_of("YoutubeTab", "https://youtube.com/@a/videos"),
            UrlKind::Channel
        );
        assert_eq!(
            kind_of("YoutubeTab", "https://youtube.com/feed/history"),
            UrlKind::Unknown
        );
        assert_eq!(
            kind_of("SoundcloudSet", "https://soundcloud.com/a/sets/b"),
            UrlKind::Playlist
        );
        assert_eq!(
            kind_of("TwitchChannel", "https://twitch.tv/a"),
            UrlKind::Channel
        );
    }
}
