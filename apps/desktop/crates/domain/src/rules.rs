//! Site rules: links matched by domain (with subdomains), URL prefix or regex decide what the
//! clipboard watch does, which preset a download uses and which fix runs afterwards. The first
//! switched-on rule that matches wins; settings without a match fall back to the defaults.

use serde::{Deserialize, Serialize};

use crate::Preset;

/// How a rule's pattern is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    /// `youtube.com` matches youtube.com and every subdomain.
    #[default]
    Domain,
    /// The link starts with the pattern (case ignored).
    Prefix,
    Regex,
}

/// What the clipboard watch does with a matching link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipAction {
    Add,
    #[default]
    Ask,
    Ignore,
}

/// A named ffmpeg fix run on a finished download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fix {
    /// Plays everywhere: H.264/AAC formats are picked first; converted only when none exist.
    Compatible,
    /// Constant frame rate H.264 (ProRes on macOS) for editing apps.
    EditFriendly,
    /// Rebuilds timestamps without re-encoding.
    Repair,
    /// EBU R128 loudness.
    Normalize,
    /// HEVC at good quality.
    Smaller,
}

impl Fix {
    pub const ALL: [Fix; 5] = [
        Fix::Compatible,
        Fix::EditFriendly,
        Fix::Repair,
        Fix::Normalize,
        Fix::Smaller,
    ];
}

impl Fix {
    /// Whether this fix re-encodes the video; those conflict, a [`FixChain`] holds one.
    pub fn is_video(self) -> bool {
        matches!(self, Fix::Compatible | Fix::EditFriendly | Fix::Smaller)
    }
}

/// The fixes run after a download, merged into one ffmpeg pass: at most one video fix, plus
/// loudness normalising and timestamp repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FixChain {
    /// Compatible, EditFriendly or Smaller; `None` keeps the video.
    pub video: Option<Fix>,
    /// Loudness normalising of the audio.
    pub normalize: bool,
    /// Rebuilds the timestamps.
    pub repair: bool,
}

impl FixChain {
    /// Nothing runs.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Sets the video fix; a non-video fix is ignored (keeps the invariant).
    pub fn set_video(&mut self, fix: Option<Fix>) {
        self.video = fix.filter(|f| f.is_video());
    }

    /// The fixes that run, in order: video, Normalize, Repair.
    pub fn fixes(&self) -> Vec<Fix> {
        self.video
            .into_iter()
            .chain(self.normalize.then_some(Fix::Normalize))
            .chain(self.repair.then_some(Fix::Repair))
            .collect()
    }
}

impl From<Fix> for FixChain {
    fn from(fix: Fix) -> Self {
        Self {
            video: fix.is_video().then_some(fix),
            normalize: fix == Fix::Normalize,
            repair: fix == Fix::Repair,
        }
    }
}

/// Reads a stored fix: null (none), a single fix name (older files) or a [`FixChain`] object.
pub fn chain_or_fix<'de, D: serde::Deserializer<'de>>(d: D) -> Result<FixChain, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Chain(FixChain),
        One(Fix),
        None(()),
    }
    Ok(match Stored::deserialize(d)? {
        Stored::Chain(mut c) => {
            c.set_video(c.video);
            c
        }
        Stored::One(fix) => fix.into(),
        Stored::None(()) => FixChain::default(),
    })
}

/// One site rule. `None` choices keep the default.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteRule {
    pub id: String,
    pub on: bool,
    pub kind: MatchKind,
    pub pattern: String,
    pub clipboard: Option<ClipAction>,
    pub preset: Option<Preset>,
    #[serde(deserialize_with = "chain_or_fix")]
    pub fix: FixChain,
    /// The folder matching downloads are saved to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
    /// The yt-dlp output template for matching single downloads (playlist entries keep theirs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outtmpl: Option<String>,
    /// Smaller file quality for matching downloads; `None` uses the setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smaller_quality: Option<u8>,
    /// Smaller file largest size for matching downloads; `None` uses the setting, 0 keeps the size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smaller_height: Option<u32>,
}

impl SiteRule {
    /// Whether `url` matches; a pattern that does not parse (bad regex) matches nothing.
    pub fn matches(&self, url: &str) -> bool {
        let pattern = self.pattern.trim();
        if pattern.is_empty() {
            return false;
        }
        match self.kind {
            MatchKind::Domain => {
                let Some(host) = url::Url::parse(url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_lowercase))
                else {
                    return false;
                };
                let want = pattern
                    .trim_start_matches("*.")
                    .trim_start_matches('.')
                    .to_lowercase();
                host == want || host.ends_with(&format!(".{want}"))
            }
            MatchKind::Prefix => url.to_lowercase().starts_with(&pattern.to_lowercase()),
            MatchKind::Regex => regex::Regex::new(pattern).is_ok_and(|re| re.is_match(url)),
        }
    }
}

/// Why `pattern` cannot be used as `kind`, if it cannot.
pub fn check_pattern(kind: MatchKind, pattern: &str) -> Result<(), String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err("empty".into());
    }
    match kind {
        MatchKind::Regex => regex::Regex::new(pattern)
            .map(drop)
            .map_err(|e| e.to_string()),
        MatchKind::Domain if pattern.contains(['/', ':', ' ']) => Err("not a domain".into()),
        _ => Ok(()),
    }
}

/// The first switched-on rule matching `url`, with its position.
pub fn rule_for<'a>(rules: &'a [SiteRule], url: &str) -> Option<(usize, &'a SiteRule)> {
    rules
        .iter()
        .enumerate()
        .find(|(_, r)| r.on && r.matches(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(kind: MatchKind, pattern: &str) -> SiteRule {
        SiteRule {
            on: true,
            kind,
            pattern: pattern.into(),
            ..Default::default()
        }
    }

    #[test]
    fn domain_takes_subdomains_only() {
        let r = rule(MatchKind::Domain, "youtube.com");
        assert!(r.matches("https://www.youtube.com/watch?v=1"));
        assert!(r.matches("https://music.youtube.com/x"));
        assert!(r.matches("https://YOUTUBE.com/x"));
        assert!(!r.matches("https://notyoutube.com/x"));
        assert!(!r.matches("https://youtube.com.evil.net/x"));
        assert!(rule(MatchKind::Domain, "*.youtube.com").matches("https://m.youtube.com/"));
    }

    #[test]
    fn prefix_and_regex() {
        assert!(
            rule(MatchKind::Prefix, "https://twitch.tv/videos/")
                .matches("https://twitch.tv/videos/12")
        );
        assert!(
            !rule(MatchKind::Prefix, "https://twitch.tv/videos/").matches("https://twitch.tv/clip")
        );
        let re = rule(MatchKind::Regex, r"^https://(www\.)?twitch\.tv/videos/\d+");
        assert!(re.matches("https://www.twitch.tv/videos/99"));
        assert!(!rule(MatchKind::Regex, "(").matches("https://a.com"));
        assert!(check_pattern(MatchKind::Regex, "(").is_err());
        assert!(check_pattern(MatchKind::Domain, "https://a.com").is_err());
        assert!(check_pattern(MatchKind::Domain, "a.com").is_ok());
    }

    #[test]
    fn first_switched_on_match_wins() {
        let mut off = rule(MatchKind::Domain, "youtube.com");
        off.on = false;
        let rules = [
            off,
            rule(MatchKind::Prefix, "https://www.youtube.com/"),
            rule(MatchKind::Domain, "youtube.com"),
        ];
        assert_eq!(
            rule_for(&rules, "https://www.youtube.com/watch").map(|(i, _)| i),
            Some(1)
        );
        assert_eq!(
            rule_for(&rules, "https://m.youtube.com/").map(|(i, _)| i),
            Some(2)
        );
        assert!(rule_for(&rules, "https://vimeo.com/1").is_none());
    }

    #[test]
    fn stored_shape() {
        let r = SiteRule {
            fix: Fix::EditFriendly.into(),
            clipboard: Some(ClipAction::Add),
            preset: Some(Preset::DEFAULT),
            ..rule(MatchKind::Regex, "x")
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(
            json.contains("\"edit-friendly\"")
                && json.contains("\"regex\"")
                && json.contains("\"add\"")
        );
        assert_eq!(serde_json::from_str::<SiteRule>(&json).unwrap(), r);
    }

    #[test]
    fn a_stored_fix_reads_as_null_one_fix_or_a_chain() {
        let read = |fix: &str| {
            serde_json::from_str::<SiteRule>(&format!(r#"{{"fix":{fix}}}"#))
                .unwrap()
                .fix
        };
        assert!(read("null").is_empty());
        assert!(
            serde_json::from_str::<SiteRule>("{}")
                .unwrap()
                .fix
                .is_empty()
        );
        assert_eq!(read(r#""smaller""#).video, Some(Fix::Smaller));
        assert!(read(r#""normalize""#).normalize);
        assert!(read(r#""repair""#).repair);
        let chain = read(r#"{"video":"compatible","normalize":true}"#);
        assert_eq!(chain.fixes(), [Fix::Compatible, Fix::Normalize]);
        // a non-video fix cannot sit in the video slot
        assert!(read(r#"{"video":"repair"}"#).is_empty());
        let r = SiteRule {
            fix: chain,
            ..Default::default()
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<SiteRule>(&json).unwrap(), r);
    }
}
