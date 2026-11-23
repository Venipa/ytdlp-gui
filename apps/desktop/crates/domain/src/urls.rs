use url::Url;

/// Result of splitting pasted text into URLs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SplitUrls {
    /// Valid http/https URLs, first-seen order, deduplicated.
    pub urls: Vec<String>,
    /// `(1-based line number, token)` for everything that is not an http/https URL.
    pub invalid: Vec<(usize, String)>,
}

/// Splits pasted text on whitespace per line, keeping http/https URLs (deduped) and reporting the rest.
/// Deep links and CLI args go through this too: `javascript:`, `file:`, `ftp:` are invalid.
pub fn split_urls(text: &str) -> SplitUrls {
    let mut out = SplitUrls::default();
    for (i, line) in text.lines().enumerate() {
        for tok in line.split_whitespace() {
            let tok = tok
                .trim_matches(|c| matches!(c, '<' | '>' | '"' | '\'' | ','))
                .trim();
            match Url::parse(tok) {
                Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => {
                    if !out.urls.iter().any(|x| x == tok) {
                        out.urls.push(tok.to_string());
                    }
                }
                _ if tok.is_empty() => {}
                _ => out.invalid.push((i + 1, tok.to_string())),
            }
        }
    }
    out
}

/// Normalised host used for per-domain concurrency: lowercase, no `www.`, no port. Empty on parse failure.
pub fn domain_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .map(|h| h.strip_prefix("www.").unwrap_or(&h).to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_dedupes_and_reports() {
        let r = split_urls(
            "https://a.com/x  http://b.org/y\n\nftp://c.com/z\nnot a url\nhttps://a.com/x\njavascript:alert(1)\n<https://d.net/q>,",
        );
        assert_eq!(
            r.urls,
            ["https://a.com/x", "http://b.org/y", "https://d.net/q"]
        );
        let bad: Vec<_> = r.invalid.iter().map(|(l, t)| (*l, t.as_str())).collect();
        assert_eq!(
            bad,
            [
                (3, "ftp://c.com/z"),
                (4, "not"),
                (4, "a"),
                (4, "url"),
                (6, "javascript:alert(1)")
            ]
        );
        assert!(split_urls("   \n\n").urls.is_empty());
    }

    #[test]
    fn domains() {
        assert_eq!(
            domain_of("https://WWW.YouTube.com:443/watch?v=1"),
            "youtube.com"
        );
        assert_eq!(domain_of("garbage"), "");
    }
}
