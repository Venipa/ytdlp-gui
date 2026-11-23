//! Error classification: yt-dlp exception text to a stable [`ErrorCode`] (D-023). Pure.
use domain::{EngineError, ErrorCode};

/// Ordered rules: first match wins. Specific wording before generic wording.
const RULES: &[(ErrorCode, &[&str])] = &[
    (
        ErrorCode::RateLimited,
        &["http error 429", "too many requests"],
    ),
    (
        ErrorCode::Private,
        &["private video", "this video is private", "video is private"],
    ),
    (
        ErrorCode::Geo,
        &[
            "not made this video available in your country",
            "geo restrict",
            "not available in your country",
            "geo-restrict",
        ],
    ),
    (
        ErrorCode::AgeRestricted,
        &[
            "confirm your age",
            "age-restricted",
            "age restricted",
            "inappropriate for some users",
        ],
    ),
    (
        ErrorCode::Login,
        &[
            "not a bot",
            "login required",
            "requires login",
            "requires authentication",
            "use --cookies",
            "cookies for authentication",
        ],
    ),
    (ErrorCode::UnsupportedUrl, &["unsupported url"]),
    (
        ErrorCode::FormatUnavailable,
        &[
            "requested format is not available",
            "requested format not available",
        ],
    ),
    (
        ErrorCode::Ssl,
        &[
            "certificate_verify_failed",
            "ssl:",
            "sslerror",
            "certificate verify failed",
        ],
    ),
    (
        ErrorCode::Ffmpeg,
        &[
            "ffmpeg not found",
            "ffprobe and ffmpeg not found",
            "ffmpeg is not installed",
            "ffprobe not found",
            "ffmpeg or avconv not found",
        ],
    ),
    (
        ErrorCode::JsRuntimeMissing,
        &["no supported javascript runtime", "javascript runtime"],
    ),
    (
        ErrorCode::DiskFull,
        &[
            "no space left on device",
            "[errno 28]",
            "not enough space on the disk",
            "[winerror 112]",
        ],
    ),
    (
        ErrorCode::Permission,
        &[
            "permission denied",
            "[errno 13]",
            "[winerror 5]",
            "access is denied",
        ],
    ),
    (
        ErrorCode::CookiesLocked,
        &[
            "could not copy chrome cookie database",
            "could not copy edge cookie database",
            "cookie database is locked",
            "cookies database is locked",
            "database is locked",
        ],
    ),
    (
        ErrorCode::Unavailable,
        &[
            "video unavailable",
            "no longer available",
            "has been removed",
            "this video is not available",
            "does not exist",
            "has been terminated",
            "http error 404",
            "http error 410",
        ],
    ),
    (
        ErrorCode::Network,
        &[
            "getaddrinfo failed",
            "name or service not known",
            "temporary failure in name resolution",
            "timed out",
            "connection reset",
            "connection aborted",
            "connection refused",
            "network is unreachable",
            "remote end closed",
            "incompleteread",
            "urlopen error",
            "http error 500",
            "http error 502",
            "http error 503",
            "http error 504",
        ],
    ),
];

/// Map a yt-dlp failure to a code. `http_status` (from `HTTPError.status`) wins for 429, 5xx and 404/410.
pub fn classify(exc_type: &str, msg: &str, http_status: Option<u16>) -> EngineError {
    let hay = format!("{exc_type}: {msg}").to_lowercase();
    let code = match http_status {
        Some(429) => ErrorCode::RateLimited,
        Some(404 | 410) => ErrorCode::Unavailable,
        Some(500..=599) => ErrorCode::Network,
        _ => RULES
            .iter()
            .find(|(_, pats)| pats.iter().any(|p| hay.contains(p)))
            .map_or(ErrorCode::Unknown, |(c, _)| *c),
    };
    EngineError::new(code, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden table: `exc_type<TAB>http_status<TAB>message<TAB>expected_code`.
    #[test]
    fn golden_tsv() {
        let rows: Vec<_> = include_str!("../tests/errors.tsv")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .collect();
        assert!(rows.len() >= 30, "need >= 30 rows, have {}", rows.len());
        for r in rows {
            let f: Vec<_> = r.split('\t').collect();
            assert_eq!(f.len(), 4, "bad row: {r}");
            let got = classify(f[0], f[2], f[1].parse().ok());
            assert_eq!(got.code.as_str(), f[3], "row: {r}");
        }
    }

    #[test]
    fn retryable_only_transient() {
        assert!(classify("DownloadError", "HTTP Error 429", None).retryable);
        assert!(!classify("DownloadError", "Private video", None).retryable);
        assert_eq!(classify("X", "weird", None).code, ErrorCode::Unknown);
    }
}
