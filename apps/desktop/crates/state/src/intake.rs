use domain::{ClipAction, Settings, rule_for, split_urls};
use gpui::{Context, EventEmitter};

/// Links that arrived from outside the intake box (command line, deep link, second launch, clipboard).
#[derive(Debug, Clone, PartialEq)]
pub enum IntakeEvent {
    /// Add these: the Downloads screen probes them like pasted text.
    Add(Vec<String>),
    /// Found on the clipboard: offer them, never add silently.
    Offer(Vec<String>),
}

/// Inbox for external links. Views subscribe to `IntakeEvent`; `pending` keeps what arrived before
/// a view existed (cold start with links on the command line).
#[derive(Default)]
pub struct Intake {
    pending: Vec<String>,
    offered: Vec<String>,
    /// Every link offered or added this session, so the same link is not offered twice.
    seen: std::collections::HashSet<String>,
}

/// The links in `urls` not in `seen`, which they join.
fn fresh(seen: &mut std::collections::HashSet<String>, urls: Vec<String>) -> Vec<String> {
    urls.into_iter()
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

/// Sorts clipboard links by the site rules: (to add at once, to offer). Ignored links and
/// everything while the watch is off are dropped; links no rule claims follow `clipboard_auto_add`.
pub fn sort_clipboard(s: &Settings, urls: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut add, mut ask) = (vec![], vec![]);
    if !s.clipboard_watch {
        return (add, ask);
    }
    let fallback = if s.clipboard_auto_add {
        ClipAction::Add
    } else {
        ClipAction::Ask
    };
    for url in urls {
        let action = rule_for(&s.site_rules, url)
            .and_then(|(_, r)| r.clipboard)
            .unwrap_or(fallback);
        match action {
            ClipAction::Add => add.push(url.clone()),
            ClipAction::Ask => ask.push(url.clone()),
            ClipAction::Ignore => {}
        }
    }
    (add, ask)
}

impl EventEmitter<IntakeEvent> for Intake {}

impl Intake {
    /// Links to add; only http and https survive (`split_urls`).
    pub fn add(&mut self, urls: &[String], cx: &mut Context<Self>) {
        let urls = split_urls(&urls.join("\n")).urls;
        if urls.is_empty() {
            return;
        }
        self.seen.extend(urls.iter().cloned());
        self.pending.extend(urls.iter().cloned());
        cx.emit(IntakeEvent::Add(urls));
        cx.notify();
    }

    /// Clipboard links a site rule adds at once: like `add`, but a link seen this session is skipped.
    pub fn add_new(&mut self, urls: &[String], cx: &mut Context<Self>) {
        let urls = fresh(&mut self.seen, split_urls(&urls.join("\n")).urls);
        if urls.is_empty() {
            return;
        }
        self.pending.extend(urls.iter().cloned());
        cx.emit(IntakeEvent::Add(urls));
        cx.notify();
    }

    /// Links seen on the clipboard.
    pub fn offer(&mut self, urls: &[String], cx: &mut Context<Self>) {
        let urls = fresh(&mut self.seen, split_urls(&urls.join("\n")).urls);
        if urls.is_empty() {
            return;
        }
        self.offered = urls.clone();
        cx.emit(IntakeEvent::Offer(urls));
        cx.notify();
    }

    /// Takes the links added before anyone listened.
    pub fn take_pending(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending)
    }

    /// The last clipboard offer.
    pub fn offered(&self) -> &[String] {
        &self.offered
    }

    /// Drops the clipboard offer (user dismissed it).
    pub fn clear_offer(&mut self, cx: &mut Context<Self>) {
        self.offered.clear();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::fresh;

    #[test]
    fn clipboard_links_follow_the_rules() {
        use domain::{ClipAction, MatchKind, Settings, SiteRule};
        let rule = |pattern: &str, action| SiteRule {
            on: true,
            kind: MatchKind::Domain,
            pattern: pattern.into(),
            clipboard: Some(action),
            ..Default::default()
        };
        let mut s = Settings {
            site_rules: vec![
                rule("youtube.com", ClipAction::Add),
                rule("x.com", ClipAction::Ignore),
            ],
            ..Default::default()
        };
        let urls: Vec<String> = [
            "https://www.youtube.com/watch?v=1",
            "https://x.com/a",
            "https://vimeo.com/2",
        ]
        .map(String::from)
        .into();
        let (add, ask) = super::sort_clipboard(&s, &urls);
        assert_eq!((add.len(), ask.len()), (1, 1));
        assert!(add[0].contains("youtube") && ask[0].contains("vimeo"));
        s.clipboard_auto_add = true;
        assert_eq!(super::sort_clipboard(&s, &urls).0.len(), 2);
        s.clipboard_watch = false;
        assert_eq!(super::sort_clipboard(&s, &urls), (vec![], vec![]));
    }

    #[test]
    fn a_link_is_offered_once() {
        let mut seen = Default::default();
        let a = "https://a.example/v".to_string();
        assert_eq!(fresh(&mut seen, vec![a.clone()]), vec![a.clone()]);
        assert!(fresh(&mut seen, vec![a]).is_empty());
    }
}
