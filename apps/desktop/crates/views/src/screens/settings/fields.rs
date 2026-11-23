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
            Field::Rate => rate_parts(s).0,
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
            // the number only; the unit comes from the KB/s / MB/s choice
            Field::Rate => s.rate_limit = rate(&format!("{text}{}", rate_parts(s).1))?,
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

/// The stored rate split into its number and unit suffix (`K`, `M`, `G` or empty for bytes);
/// no limit yet gives an empty number and `M`.
pub fn rate_parts(s: &Settings) -> (String, String) {
    let Some(rate) = &s.rate_limit else {
        return (String::new(), "M".into());
    };
    let digits = rate.trim_end_matches(char::is_alphabetic);
    (digits.to_owned(), rate[digits.len()..].to_uppercase())
}

/// What a valid proxy URL means, for the line under the field: (kind, host, port).
pub fn proxy_parts(url: &str) -> Option<(String, String, Option<String>)> {
    let (scheme, rest) = url.split_once("://")?;
    let rest = rest.split('/').next()?;
    let host_port = rest.rsplit_once('@').map_or(rest, |(_, h)| h);
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h, Some(p.to_owned()))
        }
        _ => (host_port, None),
    };
    (!host.is_empty()).then(|| (scheme.to_owned(), host.to_owned(), port))
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
    fn rates_take_a_number_and_keep_the_unit() {
        for good in ["500", "2", "1.5"] {
            assert_eq!(apply(Field::Rate, good).0, Ok(()), "{good}");
        }
        assert_eq!(apply(Field::Rate, "2").1.rate_limit.as_deref(), Some("2M"));
        for bad in ["fast", "0", "-1", "5MB", "1.2.3"] {
            assert_eq!(apply(Field::Rate, bad).0, Err("set-err-rate"), "{bad}");
        }
        let mut s = Settings {
            rate_limit: Some("500K".into()),
            ..Default::default()
        };
        assert_eq!(rate_parts(&s), ("500".to_string(), "K".to_string()));
        Field::Rate.apply("750", &mut s).unwrap();
        assert_eq!(s.rate_limit.as_deref(), Some("750K"));
    }

    #[test]
    fn proxy_read_back() {
        assert_eq!(
            proxy_parts("socks5://user:pw@127.0.0.1:1080"),
            Some(("socks5".into(), "127.0.0.1".into(), Some("1080".into())))
        );
        assert_eq!(
            proxy_parts("http://proxy.lan/"),
            Some(("http".into(), "proxy.lan".into(), None))
        );
        assert_eq!(proxy_parts("nope"), None);
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
}
