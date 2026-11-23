//! Cookie sets: Netscape cookies.txt lines, per-domain summaries and the per-download selection
//! (only cookies for the link's site go to yt-dlp; when two sets have a site, the earlier set wins).

/// Header yt-dlp (and every exporter) writes first.
pub const HEADER: &str = "# Netscape HTTP Cookie File";

/// Sites that sign in through another domain: a link to the key also takes the listed domains.
const PARTNERS: &[(&str, &[&str])] = &[
    ("youtube.com", &["google.com"]),
    ("youtu.be", &["youtube.com", "google.com"]),
];

/// One cookie line: its domain (lowercase, without the leading dot) and expiry (0 = session).
struct Line<'a> {
    domain: String,
    expires: i64,
    raw: &'a str,
}

/// Cookie lines of `text`; `#HttpOnly_` lines count, other comments and malformed lines do not.
fn lines(text: &str) -> impl Iterator<Item = Line<'_>> {
    text.lines().filter_map(|raw| {
        let body = raw.strip_prefix("#HttpOnly_").unwrap_or(raw);
        if body.starts_with('#') || body.trim().is_empty() {
            return None;
        }
        let f: Vec<&str> = body.split('\t').collect();
        if f.len() < 7 {
            return None;
        }
        Some(Line {
            domain: norm(f[0]),
            expires: f[4].trim().parse().ok()?,
            raw,
        })
    })
}

fn norm(domain: &str) -> String {
    domain.trim().trim_start_matches('.').to_lowercase()
}

/// Whether a cookie for `cookie` is sent to `host` (RFC 6265 domain match).
fn domain_match(cookie: &str, host: &str) -> bool {
    host == cookie || host.ends_with(&format!(".{cookie}"))
}

/// The site a cookie domain belongs to: its last two labels, or three under a two-letter country
/// code with a generic second level (`bbc.co.uk`, `abc.net.au`). An IP address stays whole.
// shortcut: a heuristic, not the public suffix list; switch to one if sites group wrong
pub fn site_of(domain: &str) -> String {
    let domain = norm(domain);
    let labels: Vec<&str> = domain.split('.').collect();
    let n = labels.len();
    if n <= 2 || labels.iter().all(|l| l.chars().all(|c| c.is_ascii_digit())) {
        return domain;
    }
    const GENERIC: [&str; 10] = [
        "co", "com", "net", "org", "gov", "ac", "edu", "ne", "or", "go",
    ];
    let take = if labels[n - 1].len() == 2 && GENERIC.contains(&labels[n - 2]) {
        3
    } else {
        2
    };
    labels[n - take..].join(".")
}

/// The cookies one domain has in a set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSummary {
    pub domain: String,
    pub count: usize,
    /// Latest expiry (unix seconds); `None` when every cookie ends with the session.
    pub expires: Option<i64>,
}

/// Domains of `text` with their cookie counts, sorted by domain.
pub fn summarize(text: &str) -> Vec<DomainSummary> {
    let mut map: std::collections::BTreeMap<String, DomainSummary> = Default::default();
    for l in lines(text) {
        let e = map.entry(l.domain.clone()).or_insert(DomainSummary {
            domain: l.domain,
            count: 0,
            expires: None,
        });
        e.count += 1;
        if l.expires > 0 {
            e.expires = Some(e.expires.map_or(l.expires, |x| x.max(l.expires)));
        }
    }
    map.into_values().collect()
}

/// Checks text for the editor: `Ok(cookie count)`, or the first bad line (1-based, 0 for the
/// whole text) and the Fluent key saying why.
pub fn check(text: &str) -> Result<usize, (usize, &'static str)> {
    let mut count = 0;
    for (i, raw) in text.lines().enumerate() {
        let body = raw.strip_prefix("#HttpOnly_").unwrap_or(raw);
        if body.starts_with('#') || body.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = body.split('\t').collect();
        if f.len() != 7 {
            return Err((i + 1, "set-ck-bad-fields"));
        }
        if f[0].trim().is_empty() {
            return Err((i + 1, "set-ck-bad-domain"));
        }
        if f[4].trim().parse::<i64>().is_err() {
            return Err((i + 1, "set-ck-bad-expiry"));
        }
        count += 1;
    }
    if count == 0 {
        return Err((0, "set-ck-bad-empty"));
    }
    Ok(count)
}

/// One switched-on set, in priority order, with its text.
pub struct SetText<'a> {
    pub name: &'a str,
    /// Domains switched off in this set (as [`DomainSummary::domain`]).
    pub off_domains: &'a [String],
    pub text: &'a str,
}

/// The cookies chosen for one link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// A cookies.txt with only the chosen lines.
    pub text: String,
    /// Which set gave which cookie domains, in set order.
    pub used: Vec<(String, Vec<String>)>,
}

/// Picks the cookies for `url`: for the link's host and each sign-in partner, the first set with a
/// matching cookie gives all of its cookies for that site. `None` when nothing matches.
pub fn select(sets: &[SetText<'_>], url: &str) -> Option<Selection> {
    let host = url::Url::parse(url)
        .ok()?
        .host_str()?
        .trim_end_matches('.')
        .to_lowercase();
    let mut targets = vec![host.clone()];
    for (site, partners) in PARTNERS {
        if domain_match(site, &host) {
            targets.extend(partners.iter().map(|p| p.to_string()));
        }
    }
    let parsed: Vec<Vec<Line>> = sets
        .iter()
        .map(|s| {
            lines(s.text)
                .filter(|l| !s.off_domains.contains(&l.domain))
                .collect()
        })
        .collect();
    let mut picked: Vec<(usize, &Line)> = vec![];
    for target in &targets {
        // a partner like google.com also takes cookies set on its subdomains (accounts.google.com)
        let hit = |l: &Line| domain_match(&l.domain, target) || domain_match(target, &l.domain);
        let Some(i) = parsed.iter().position(|ls| ls.iter().any(hit)) else {
            continue;
        };
        for l in parsed[i].iter().filter(|l| hit(l)) {
            if !picked.iter().any(|(_, p)| std::ptr::eq(*p, l)) {
                picked.push((i, l));
            }
        }
    }
    if picked.is_empty() {
        return None;
    }
    let mut text = format!("{HEADER}\n");
    let mut used: Vec<(String, Vec<String>)> = vec![];
    for (i, set) in sets.iter().enumerate() {
        let mut domains: Vec<String> = vec![];
        for (_, l) in picked.iter().filter(|(j, _)| *j == i) {
            text.push_str(l.raw);
            text.push('\n');
            if !domains.contains(&l.domain) {
                domains.push(l.domain.clone());
            }
        }
        if !domains.is_empty() {
            used.push((set.name.to_string(), domains));
        }
    }
    Some(Selection { text, used })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(domain: &str, name: &str) -> String {
        format!("{domain}\tTRUE\t/\tTRUE\t1900000000\t{name}\tv")
    }

    #[test]
    fn summary_groups_domains_and_skips_comments() {
        let t = format!(
            "{HEADER}\n# note\n{}\n{}\n#HttpOnly_{}\nvimeo.com\tFALSE\t/\tFALSE\t0\tsess\tx\nbad line\n",
            line(".youtube.com", "a"),
            line("youtube.com", "b"),
            line(".youtube.com", "c")
        );
        let s = summarize(&t);
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[1].domain.as_str(), s[1].count, s[1].expires),
            ("youtube.com", 3, Some(1900000000))
        );
        assert_eq!(
            (s[0].domain.as_str(), s[0].count, s[0].expires),
            ("vimeo.com", 1, None)
        );
    }

    #[test]
    fn sites_group_subdomains() {
        assert_eq!(site_of(".accounts.google.com"), "google.com");
        assert_eq!(site_of("youtube.com"), "youtube.com");
        assert_eq!(site_of("www.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(site_of("news.abc.net.au"), "abc.net.au");
        assert_eq!(site_of("a.b.example.de"), "example.de");
        assert_eq!(site_of("127.0.0.1"), "127.0.0.1");
        assert_eq!(site_of("localhost"), "localhost");
    }

    #[test]
    fn check_points_at_the_bad_line() {
        assert_eq!(check(&format!("{HEADER}\n{}\n", line("a.com", "x"))), Ok(1));
        assert_eq!(
            check(&format!("{HEADER}\n{}\na.com\tTRUE\n", line("a.com", "x")))
                .unwrap_err()
                .0,
            3
        );
        assert!(check(HEADER).is_err());
    }

    #[test]
    fn first_set_wins_per_site_and_partners_come_along() {
        let main = format!(
            "{}\n{}\n{}\n",
            line(".youtube.com", "SID"),
            line(".google.com", "SAPISID"),
            line("vimeo.com", "v")
        );
        let firefox = format!(
            "{}\n{}\n",
            line(".youtube.com", "OTHER"),
            line("x.com", "t")
        );
        let sets = [
            SetText {
                name: "Main",
                off_domains: &[],
                text: &main,
            },
            SetText {
                name: "Firefox",
                off_domains: &[],
                text: &firefox,
            },
        ];
        let s = select(&sets, "https://www.youtube.com/watch?v=1").unwrap();
        assert!(s.text.contains("SID") && s.text.contains("SAPISID"));
        assert!(!s.text.contains("OTHER") && !s.text.contains("vimeo"));
        assert_eq!(
            s.used,
            vec![(
                "Main".to_string(),
                vec!["youtube.com".into(), "google.com".into()]
            )]
        );
        let s = select(&sets, "https://x.com/a/status/1").unwrap();
        assert_eq!(s.used, vec![("Firefox".to_string(), vec!["x.com".into()])]);
        assert!(select(&sets, "https://example.org/v").is_none());
    }

    #[test]
    fn a_switched_off_domain_falls_through_to_the_next_set() {
        let main = format!("{}\n", line(".youtube.com", "SID"));
        let firefox = format!("{}\n", line(".youtube.com", "OTHER"));
        let off = ["youtube.com".to_string()];
        let sets = [
            SetText {
                name: "Main",
                off_domains: &off,
                text: &main,
            },
            SetText {
                name: "Firefox",
                off_domains: &[],
                text: &firefox,
            },
        ];
        let s = select(&sets, "https://youtu.be/abc").unwrap();
        assert!(s.text.contains("OTHER") && !s.text.contains("SID"));
    }
}
