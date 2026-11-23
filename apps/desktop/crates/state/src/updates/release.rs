//! The pure half of the updater: versions, which release a channel takes, which asset this
//! platform installs from, and the checksum it must match.
use std::cmp::Ordering;

use domain::UpdateChannel;
use serde::Deserialize;

/// A semver version (`1.2.3`, `1.2.3-beta.1`, build metadata ignored), ordered by the semver rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: (u64, u64, u64),
    pre: Vec<Ident>,
}

/// A pre-release identifier; numbers sort before words (declaration order).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Ident {
    Num(u64),
    Word(String),
}

impl Version {
    /// Reads `v1.2.3[-pre][+build]`; `None` when it is not one.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split_once('+').map_or(text, |(v, _)| v);
        let (core, pre) = text.split_once('-').unwrap_or((text, ""));
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let core = (parts.next()??, parts.next()??, parts.next()??);
        if parts.next().is_some() {
            return None;
        }
        let pre = match pre {
            "" => Vec::new(),
            pre => pre
                .split('.')
                .map(|id| match id.parse() {
                    Ok(n) => Ident::Num(n),
                    Err(_) => Ident::Word(id.to_string()),
                })
                .collect(),
        };
        Some(Self { core, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            // a release sorts after its pre-releases
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// One entry of the GitHub releases list.
#[derive(Debug, Clone, Deserialize)]
pub struct Published {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// A file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// The newest release `channel` takes that is newer than `current` (never a downgrade). Stable
/// skips pre-releases, Beta includes them; drafts and tags that are not versions never count.
pub fn pick<'a>(
    releases: &'a [Published],
    channel: UpdateChannel,
    current: &Version,
) -> Option<(&'a Published, Version)> {
    releases
        .iter()
        .filter(|r| !r.draft && (channel == UpdateChannel::Beta || !r.prerelease))
        .filter_map(|r| Some((r, Version::parse(&r.tag_name)?)))
        .filter(|(_, v)| v > current)
        .max_by(|a, b| a.1.cmp(&b.1))
}

/// The asset name `scripts/package-*` gives version `version` for `os`/`arch` (Rust's
/// `std::env::consts` names); `None` where no package is built.
pub fn asset_name(version: &str, os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    match os {
        "windows" => Some(format!("ytdlpd-{version}-windows-{arch}-setup.exe")),
        "macos" => Some(format!("ytdlpd-{version}-macos-{arch}.dmg")),
        "linux" => Some(format!("ytdlpd-{version}-linux-{arch}.AppImage")),
        _ => None,
    }
}

/// The file the release workflow writes with `sha256sum *`.
pub const SUMS: &str = "SHA256SUMS.txt";

/// The lower-case sha256 `sums` (sha256sum output) lists for `name`. Fails closed: a missing
/// line or a malformed hash is `None`, which never installs.
pub fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        // binary mode marks the name with `*`
        let file = file.trim_start().trim_start_matches('*');
        let ok = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        (file == name && ok).then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn release(tag: &str, prerelease: bool) -> Published {
        Published {
            tag_name: tag.into(),
            html_url: format!("https://example.test/{tag}"),
            prerelease,
            draft: false,
            assets: vec![],
        }
    }

    #[test]
    fn versions_order_by_semver() {
        assert!(v("0.2.0") > v("0.1.9"));
        assert!(v("v1.0.0") > v("1.0.0-rc.1"));
        assert!(v("1.0.0-beta.2") > v("1.0.0-beta.1"));
        assert!(v("1.0.0-beta.11") > v("1.0.0-beta.2"));
        assert!(v("1.0.0-beta") > v("1.0.0-1"));
        assert!(v("1.0.0-alpha.1") > v("1.0.0-alpha"));
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
        assert!(Version::parse("nightly").is_none() && Version::parse("1.2").is_none());
        assert!(Version::parse("1.2.3.4").is_none());
    }

    #[test]
    fn stable_skips_prereleases_beta_takes_them_never_downgrade() {
        let list = vec![
            release("v0.3.0-beta.1", true),
            release("v0.2.0", false),
            release("v0.1.0", false),
            release("nightly", false),
            Published {
                draft: true,
                ..release("v9.0.0", false)
            },
        ];
        let tag = |c, cur: &str| pick(&list, c, &v(cur)).map(|(r, _)| r.tag_name.clone());
        assert_eq!(tag(UpdateChannel::Stable, "0.1.0").as_deref(), Some("v0.2.0"));
        assert_eq!(
            tag(UpdateChannel::Beta, "0.1.0").as_deref(),
            Some("v0.3.0-beta.1")
        );
        assert_eq!(tag(UpdateChannel::Stable, "0.2.0"), None);
        // a beta user switching back to stable is never offered an older build
        assert_eq!(tag(UpdateChannel::Stable, "0.3.0-beta.1"), None);
        assert_eq!(tag(UpdateChannel::Beta, "0.3.0"), None);
    }

    #[test]
    fn assets_follow_the_package_scripts() {
        let name = |os, arch| asset_name("0.2.0", os, arch);
        assert_eq!(
            name("windows", "x86_64").as_deref(),
            Some("ytdlpd-0.2.0-windows-x64-setup.exe")
        );
        assert_eq!(
            name("macos", "aarch64").as_deref(),
            Some("ytdlpd-0.2.0-macos-arm64.dmg")
        );
        assert_eq!(
            name("macos", "x86_64").as_deref(),
            Some("ytdlpd-0.2.0-macos-x64.dmg")
        );
        assert_eq!(
            name("linux", "aarch64").as_deref(),
            Some("ytdlpd-0.2.0-linux-arm64.AppImage")
        );
        assert_eq!(name("linux", "riscv64"), None);
        assert_eq!(name("freebsd", "x86_64"), None);
    }

    #[test]
    fn checksums_parse_and_fail_closed() {
        let hash = "AB".repeat(32);
        let sums = format!(
            "{hash}  ytdlpd-0.2.0-windows-x64-setup.exe\n{}  *ytdlpd-0.2.0-macos-arm64.dmg\nzz  short.AppImage\n",
            "0".repeat(64)
        );
        assert_eq!(
            expected_sum(&sums, "ytdlpd-0.2.0-windows-x64-setup.exe"),
            Some("ab".repeat(32))
        );
        assert_eq!(
            expected_sum(&sums, "ytdlpd-0.2.0-macos-arm64.dmg"),
            Some("0".repeat(64))
        );
        assert_eq!(expected_sum(&sums, "short.AppImage"), None);
        assert_eq!(expected_sum(&sums, "missing.exe"), None);
        assert_eq!(expected_sum("", "x"), None);
    }
}
