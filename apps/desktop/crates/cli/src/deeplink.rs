//! `ytdlpd://add?url=<link>` parsing and link validation. Only http(s) links are ever accepted.
use percent_encoding::percent_decode_str;
use url::Url;

/// Scheme registered with the operating system.
pub const SCHEME: &str = "ytdlpd";

/// `s` as a normalized http(s) link with a host, else `None` (`javascript:`, `file:`, ...).
pub fn http_url(s: &str) -> Option<String> {
    let u = Url::parse(s.trim()).ok()?;
    (matches!(u.scheme(), "http" | "https") && u.host_str().is_some()).then(|| u.to_string())
}

/// Links carried by a `ytdlpd://add?url=...` deep link (several `url=` allowed). A value that is
/// not a link after one decode gets a second decode (double-encoded by a careless sender).
/// Anything else, including a missing `url`, yields no links.
pub fn parse(link: &str) -> Vec<String> {
    let Ok(u) = Url::parse(link.trim()) else {
        return vec![];
    };
    // `ytdlpd://add?...` has host `add`; `ytdlpd:add?...` has path `add`
    let action = u.host_str().unwrap_or_else(|| u.path().trim_matches('/'));
    if u.scheme() != SCHEME || action != "add" {
        return vec![];
    }
    let mut out: Vec<String> = Vec::new();
    for (k, v) in u.query_pairs() {
        if k != "url" {
            continue;
        }
        let found = http_url(&v).or_else(|| http_url(&percent_decode_str(&v).decode_utf8_lossy()));
        out.extend(found.filter(|f| !out.contains(f)));
    }
    out
}

/// Links from command-line arguments: deep links are parsed, plain arguments must be http(s).
pub fn links(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for a in args {
        let found = if a.trim().to_ascii_lowercase().starts_with("ytdlpd:") {
            parse(a)
        } else {
            http_url(a).into_iter().collect()
        };
        if found.is_empty() {
            log::warn!("cli: ignoring argument that is not a link: {a:?}");
        }
        out.extend(found);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_link_table() {
        let cases: &[(&str, &[&str])] = &[
            (
                "ytdlpd://add?url=https%3A%2F%2Fexample.com%2Fwatch%3Fv%3D1",
                &["https://example.com/watch?v=1"],
            ),
            // double encoded
            (
                "ytdlpd://add?url=https%253A%252F%252Fexample.com%252Fa",
                &["https://example.com/a"],
            ),
            ("ytdlpd:add?url=http%3A%2F%2Fa.b%2F", &["http://a.b/"]),
            (
                "YTDLPD://add?url=https://a.b/x&url=https://c.d/y",
                &["https://a.b/x", "https://c.d/y"],
            ),
            // missing or empty url
            ("ytdlpd://add", &[]),
            ("ytdlpd://add?url=", &[]),
            ("ytdlpd://add?other=https%3A%2F%2Fa.b", &[]),
            // rejected schemes, also after double decoding
            ("ytdlpd://add?url=javascript%3Aalert(1)", &[]),
            (
                "ytdlpd://add?url=file%3A%2F%2F%2Fc%3A%2Fwindows%2Fwin.ini",
                &[],
            ),
            (
                "ytdlpd://add?url=file%253A%252F%252F%252Fetc%252Fpasswd",
                &[],
            ),
            ("ytdlpd://add?url=data%3Atext%2Fhtml%2Chi", &[]),
            // wrong action or scheme
            ("ytdlpd://remove?url=https%3A%2F%2Fa.b", &[]),
            ("https://add?url=https%3A%2F%2Fa.b", &[]),
        ];
        for (input, want) in cases {
            assert_eq!(parse(input), *want, "{input}");
        }
    }

    #[test]
    fn plain_args() {
        let a = |s: &[&str]| links(&s.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            a(&["https://a.b", "javascript:x", "file:///x", "not a url"]),
            ["https://a.b/"]
        );
    }
}
