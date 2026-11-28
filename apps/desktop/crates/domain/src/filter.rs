//! Search-box grammar: whitespace-separated terms, `type:`, `status:`, `size:`, `site:`, else free text.
//! Terms are ANDed; `type:`/`status:` accept comma lists (OR within the list).
use crate::model::{DownloadStatus, DownloadSummary, MediaKind};

/// Comparison operator of a `size:` term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

/// One parsed term.
#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Type(Vec<MediaKind>),
    Status(Vec<DownloadStatus>),
    /// Bytes (1 KB = 1024 B).
    Size(Cmp, u64),
    /// Lowercase substring of the domain.
    Site(String),
    /// Lowercase substring of title, uploader or url.
    Text(String),
    /// A `key:value` that did not parse (e.g. `size:abc`, or a half-typed `type:`); matches everything,
    /// reported so the input can highlight it.
    Invalid(String),
}

/// A parsed search expression.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    pub terms: Vec<Term>,
}

/// Status chips with counts above the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    All,
    Active,
    Done,
    Failed,
}

impl Chip {
    /// Whether `s` belongs under this chip (Active = queued, probing, downloading, processing, paused).
    pub fn matches(self, s: DownloadStatus) -> bool {
        match self {
            Chip::All => true,
            Chip::Active => !s.is_terminal(),
            Chip::Done => s == DownloadStatus::Completed,
            Chip::Failed => matches!(s, DownloadStatus::Failed | DownloadStatus::Cancelled),
        }
    }
}

fn parse_status(v: &str) -> Option<Vec<DownloadStatus>> {
    use DownloadStatus::*;
    v.split(',')
        .map(|p| match p {
            "active" => Some(vec![Probing, Downloading, Processing]),
            "done" => Some(vec![Completed]),
            "error" => Some(vec![Failed]),
            p => p.parse().ok().map(|s| vec![s]),
        })
        .collect::<Option<Vec<_>>>()
        .map(|v| v.concat())
}

fn parse_size(v: &str) -> Option<(Cmp, u64)> {
    let (cmp, rest) = if let Some(r) = v.strip_prefix("<=") {
        (Cmp::Le, r)
    } else if let Some(r) = v.strip_prefix(">=") {
        (Cmp::Ge, r)
    } else if let Some(r) = v.strip_prefix('<') {
        (Cmp::Lt, r)
    } else if let Some(r) = v.strip_prefix('>') {
        (Cmp::Gt, r)
    } else {
        (Cmp::Eq, v.strip_prefix('=').unwrap_or(v))
    };
    let split = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let (num, unit) = rest.split_at(split);
    let n: f64 = num.parse().ok()?;
    let mult: f64 = match unit {
        "" | "b" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" => 1024.0f64.powi(4),
        _ => return None,
    };
    Some((cmp, (n * mult) as u64))
}

impl Filter {
    /// Parses a search string; never fails (bad terms become `Term::Invalid`).
    pub fn parse(input: &str) -> Filter {
        let terms = input
            .split_whitespace()
            .map(|tok| {
                let lc = tok.to_lowercase();
                let Some((key, val)) = lc.split_once(':') else {
                    return Term::Text(lc);
                };
                let bad = || Term::Invalid(tok.to_string());
                match key {
                    "type" => val
                        .split(',')
                        .map(|v| match v {
                            "video" => Some(MediaKind::Video),
                            "audio" => Some(MediaKind::Audio),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>()
                        .map_or_else(bad, Term::Type),
                    "status" => parse_status(val).map_or_else(bad, Term::Status),
                    "size" => parse_size(val).map_or_else(bad, |(c, n)| Term::Size(c, n)),
                    "site" if !val.is_empty() => Term::Site(val.to_string()),
                    "site" => bad(),
                    _ => Term::Text(lc), // e.g. a URL typed into search
                }
            })
            .collect();
        Filter { terms }
    }

    /// True when no term restricts anything.
    pub fn is_empty(&self) -> bool {
        self.terms.iter().all(|t| matches!(t, Term::Invalid(_)))
    }

    /// Any term that did not parse.
    pub fn has_invalid(&self) -> bool {
        self.terms.iter().any(|t| matches!(t, Term::Invalid(_)))
    }

    /// In-memory match against a summary (uses the precomputed lowercase fields).
    pub fn matches(&self, s: &DownloadSummary) -> bool {
        self.terms.iter().all(|t| match t {
            Term::Type(k) => k.contains(&s.kind),
            Term::Status(v) => v.contains(&s.status),
            Term::Size(c, n) => s.file_size.or(s.bytes_total).is_some_and(|sz| {
                let (sz, n) = (sz.max(0) as u64, *n);
                match c {
                    Cmp::Lt => sz < n,
                    Cmp::Le => sz <= n,
                    Cmp::Gt => sz > n,
                    Cmp::Ge => sz >= n,
                    Cmp::Eq => sz == n,
                }
            }),
            Term::Site(v) => s.domain.contains(v.as_str()),
            Term::Text(v) => {
                s.title_lc.contains(v.as_str())
                    || s.uploader_lc.contains(v.as_str())
                    || s.url.to_lowercase().contains(v.as_str())
            }
            Term::Invalid(_) => true,
        })
    }
}

#[cfg(test)]
pub(crate) fn test_summary(
    title: &str,
    kind: MediaKind,
    status: DownloadStatus,
    size: Option<i64>,
    domain: &str,
) -> DownloadSummary {
    use crate::model::DownloadId;
    let mut s = DownloadSummary {
        id: DownloadId(1),
        url: format!("https://{domain}/watch?v=1"),
        domain: domain.into(),
        extractor: None,
        video_id: None,
        title: Some(title.into()),
        uploader: Some("Some Channel".into()),
        title_lc: String::new(),
        uploader_lc: String::new(),
        duration_s: None,
        thumbnail_url: None,
        thumbnail_path: None,
        kind,
        preset: "video:best".into(),
        format_label: None,
        status,
        status_reason: None,
        phase: None,
        progress: 0.0,
        bytes_done: None,
        bytes_total: None,
        file_path: None,
        file_size: size,
        size_before_fix: None,
        file_missing: false,
        error_code: None,
        error_message: None,
        attempts: 0,
        next_attempt_at: None,
        playlist_id: None,
        playlist_index: None,
        position: 1.0,
        created_at: 0,
        started_at: None,
        finished_at: None,
        updated_at: 0,
    };
    s.refresh_lc();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use DownloadStatus::*;
    use MediaKind::*;

    #[test]
    fn parses_tokens() {
        let f = Filter::parse("Cats type:audio,video status:done size:>=1.5GB site:YouTube.com");
        assert_eq!(
            f.terms,
            vec![
                Term::Text("cats".into()),
                Term::Type(vec![Audio, Video]),
                Term::Status(vec![Completed]),
                Term::Size(Cmp::Ge, (1.5 * 1024.0 * 1024.0 * 1024.0) as u64),
                Term::Site("youtube.com".into()),
            ]
        );
        for (inp, c, n) in [
            ("size:<50mb", Cmp::Lt, 50 << 20),
            ("size:<=1k", Cmp::Le, 1024),
            ("size:>10", Cmp::Gt, 10),
            ("size:=5mb", Cmp::Eq, 5 << 20),
            ("size:7kb", Cmp::Eq, 7168),
        ] {
            assert_eq!(Filter::parse(inp).terms, vec![Term::Size(c, n)], "{inp}");
        }
        assert!(Filter::parse("type:").has_invalid());
        assert!(
            Filter::parse("size:abc status:nope")
                .terms
                .iter()
                .all(|t| matches!(t, Term::Invalid(_)))
        );
        assert!(Filter::parse("").is_empty() && Filter::parse("type:").is_empty());
        assert_eq!(
            Filter::parse("status:active").terms,
            vec![Term::Status(vec![Probing, Downloading, Processing])]
        );
    }

    #[test]
    fn matching() {
        let a = test_summary(
            "Cat Video",
            Video,
            Completed,
            Some(100 << 20),
            "www.youtube.com",
        );
        let b = test_summary("Podcast", Audio, Failed, None, "soundcloud.com");
        let m = |q: &str, s: &DownloadSummary| Filter::parse(q).matches(s);
        assert!(
            m("", &a) && m("cat", &a) && !m("cat", &b) && m("channel", &b) && m("watch?v=1", &a)
        );
        assert!(m("type:video", &a) && !m("type:audio", &a) && m("type:audio status:failed", &b));
        assert!(m("size:>50mb", &a) && !m("size:<50mb", &a) && !m("size:>1b", &b)); // no size never matches
        assert!(m("site:youtube", &a) && !m("site:youtube", &b));
        assert!(m("cat type:video status:done size:>=100mb site:you", &a));
        assert!(
            Chip::Active.matches(Paused)
                && Chip::Failed.matches(Cancelled)
                && !Chip::Done.matches(Failed)
        );
    }
}
