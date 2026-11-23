//! The text fields of the settings pages: how each shows its setting, validates what was typed
//! and writes it back. Pure functions over `domain::Settings`, so they are unit tested.

use domain::Settings;

/// Fluent key of the error shown under a field.
pub type Invalid = &'static str;

/// One editable text field of the settings pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Template,
    Proxy,
    Rate,
    Global,
    PerDomain,
    Retries,
    Langs,
}

impl Field {
    pub const ALL: [Field; 7] = [
        Field::Template,
        Field::Proxy,
        Field::Rate,
        Field::Global,
        Field::PerDomain,
        Field::Retries,
        Field::Langs,
    ];

    /// The text the field shows for `s`.
    pub fn show(self, s: &Settings) -> String {
        match self {
            Field::Template => s.outtmpl.clone(),
            Field::Proxy => s.proxy.clone().unwrap_or_default(),
            Field::Rate => s.rate_limit.clone().unwrap_or_default(),
            Field::Global => s.concurrent_global.to_string(),
            Field::PerDomain => s.concurrent_per_domain.to_string(),
            Field::Retries => s.max_attempts.to_string(),
            Field::Langs => s.subtitles.langs.join(", "),
        }
    }

    /// Validates `text` and writes it into `s`; on `Err` nothing was written.
    pub fn apply(self, text: &str, s: &mut Settings) -> Result<(), Invalid> {
        let text = text.trim();
        match self {
            Field::Template if text.is_empty() => return Err("set-err-template"),
            Field::Template => s.outtmpl = text.to_owned(),
            Field::Proxy => s.proxy = proxy(text)?,
            Field::Rate => s.rate_limit = rate(text)?,
            Field::Global => s.concurrent_global = number(text, 1, 10)?,
            Field::PerDomain => s.concurrent_per_domain = number(text, 1, 10)?,
            Field::Retries => s.max_attempts = number(text, 1, 10)?,
            Field::Langs => s.subtitles.langs = langs(text)?,
        }
        Ok(())
    }

    /// Moves a numeric field by `delta`, kept inside its range. Other fields ignore it.
    pub fn bump(self, delta: i32, s: &mut Settings) {
        let current = self.show(s).parse::<i32>().unwrap_or(0);
        let next = (current + delta).max(0).unsigned_abs();
        // out of range clamps to the nearest bound through `apply`
        let (min, max) = self.range();
        let _ = self.apply(&next.clamp(min, max).to_string(), s);
    }

    fn range(self) -> (u32, u32) {
        (1, 10)
    }
}

fn number(text: &str, min: u32, max: u32) -> Result<u32, Invalid> {
    text.parse::<u32>()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or("set-err-number")
}

/// `http`, `https`, `socks4`, `socks5` or `socks5h` URL with a host; empty clears the proxy.
fn proxy(text: &str) -> Result<Option<String>, Invalid> {
    if text.is_empty() {
        return Ok(None);
    }
    let ok = text.split_once("://").is_some_and(|(scheme, rest)| {
        ["http", "https", "socks4", "socks5", "socks5h"].contains(&scheme)
            && !rest.is_empty()
            && !rest.starts_with(['/', ':'])
            && !text.contains(char::is_whitespace)
    });
    ok.then(|| Some(text.to_owned())).ok_or("set-err-proxy")
}

/// A rate such as `500K`, `2M` or `1.5M` (bytes per second); empty clears the limit.
fn rate(text: &str) -> Result<Option<String>, Invalid> {
    if text.is_empty() {
        return Ok(None);
    }
    let digits = text.trim_end_matches(['k', 'K', 'm', 'M', 'g', 'G']);
    let unit = &text[digits.len()..];
    let ok = unit.len() <= 1
        && digits.parse::<f64>().is_ok_and(|n| n > 0.)
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.');
    ok.then(|| Some(text.to_owned())).ok_or("set-err-rate")
}

/// Comma separated language codes (`en`, `pt-BR`, `all`).
fn langs(text: &str) -> Result<Vec<String>, Invalid> {
    let list: Vec<String> = text
        .split(',')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    let ok = list.iter().all(|l| {
        l.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    });
    ok.then_some(list).ok_or("set-err-langs")
}

/// Whether `text` looks like a Netscape cookies file: at least one line of seven tab-separated
/// fields (comments and blank lines are skipped).
pub fn is_cookies_text(text: &str) -> bool {
    text.lines()
        .map(|l| l.trim_start_matches("#HttpOnly_"))
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .any(|l| l.split('\t').count() == 7)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(field: Field, text: &str) -> (Result<(), Invalid>, Settings) {
        let mut s = Settings::default();
        (field.apply(text, &mut s), s)
    }

    #[test]
    fn proxy_urls() {
        assert_eq!(apply(Field::Proxy, "socks5://127.0.0.1:1080").0, Ok(()));
        assert_eq!(apply(Field::Proxy, "").1.proxy, None);
        for bad in [
            "127.0.0.1:8080",
            "ftp://x",
            "http://",
            "http://a b",
            "http:///x",
        ] {
            assert_eq!(apply(Field::Proxy, bad).0, Err("set-err-proxy"), "{bad}");
        }
    }

    #[test]
    fn rates() {
        for good in ["500K", "2M", "1.5m", "100"] {
            assert_eq!(apply(Field::Rate, good).0, Ok(()), "{good}");
        }
        for bad in ["fast", "0", "-1M", "5MB", "K", "1.2.3M"] {
            assert_eq!(apply(Field::Rate, bad).0, Err("set-err-rate"), "{bad}");
        }
    }

    #[test]
    fn template_numbers_langs() {
        assert_eq!(apply(Field::Template, "  ").0, Err("set-err-template"));
        assert_eq!(apply(Field::Global, "0").0, Err("set-err-number"));
        assert_eq!(apply(Field::Global, "4").1.concurrent_global, 4);
        assert_eq!(
            apply(Field::Langs, "en, pt-BR").1.subtitles.langs,
            ["en", "pt-BR"]
        );
        assert_eq!(apply(Field::Langs, "en;fr").0, Err("set-err-langs"));
    }

    #[test]
    fn bump_clamps() {
        let mut s = Settings::default();
        Field::Global.bump(-9, &mut s);
        assert_eq!(s.concurrent_global, 1);
        Field::Global.bump(99, &mut s);
        assert_eq!(s.concurrent_global, 10);
    }

    #[test]
    fn cookies_text() {
        assert!(is_cookies_text(
            "# Netscape HTTP Cookie File\n.a.com\tTRUE\t/\tTRUE\t0\tk\tv\n"
        ));
        assert!(!is_cookies_text("hello world"));
    }
}
