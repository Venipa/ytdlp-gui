//! URL candidates from clipboard text. Read only: this module has no way to write the clipboard.
use crate::deeplink::http_url;

/// At most this many candidates per read.
const LIMIT: usize = 20;

/// Whatever supplies clipboard text. Read only by design.
pub trait ClipboardRead {
    /// Current clipboard text, if any.
    fn text(&self) -> Option<String>;
}

/// http(s) links in `text` (whitespace separated, wrapping punctuation trimmed), deduplicated.
pub fn candidates(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in text.split_whitespace() {
        let tok =
            tok.trim_matches(|c: char| matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | ',' | ';'));
        if let Some(u) = http_url(tok).filter(|u| !out.contains(u)) {
            out.push(u);
        }
        if out.len() == LIMIT {
            break;
        }
    }
    out
}

/// Remembers the last clipboard text so reactivating the window offers a link once.
#[derive(Default)]
pub struct Watcher {
    last: Option<String>,
}

impl Watcher {
    /// Called when the window is activated: candidates when the clipboard changed since last time.
    pub fn check(&mut self, clipboard: &dyn ClipboardRead) -> Vec<String> {
        let text = clipboard.text();
        if text == self.last {
            return vec![];
        }
        let found = text.as_deref().map(candidates).unwrap_or_default();
        self.last = text;
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake(RefCell<Option<String>>);
    impl ClipboardRead for Fake {
        fn text(&self) -> Option<String> {
            self.0.borrow().clone()
        }
    }

    #[test]
    fn clipboard_candidates() {
        let t = "see (https://a.b/x), and https://a.b/x again\nhttp://c.d/y?z=1 ftp://no javascript:x file:///x plain words";
        assert_eq!(candidates(t), ["https://a.b/x", "http://c.d/y?z=1"]);
        assert!(candidates("").is_empty() && candidates("hello world").is_empty());
        let many: String = (0..50).map(|i| format!("https://a.b/{i} ")).collect();
        assert_eq!(candidates(&many).len(), LIMIT);
        // the watcher offers a change once
        let fake = Fake(RefCell::new(Some("https://a.b/1".into())));
        let mut w = Watcher::default();
        assert_eq!(w.check(&fake), ["https://a.b/1"]);
        assert!(w.check(&fake).is_empty());
        *fake.0.borrow_mut() = Some("https://a.b/2".into());
        assert_eq!(w.check(&fake), ["https://a.b/2"]);
    }

    #[test]
    fn clipboard_never_writes() {
        // `ClipboardRead` has no write method; the gpui adapter must not call the write API either.
        // The needle is assembled so this file does not match itself.
        let needle = ["write_to", "_clipboard"].concat();
        for src in [
            include_str!("clipboard.rs"),
            include_str!("../../ytdlpd/src/platform/clipboard.rs"),
        ] {
            assert!(!src.contains(&needle), "clipboard code must not write");
        }
    }
}
