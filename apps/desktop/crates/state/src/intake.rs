use domain::split_urls;
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
    fn a_link_is_offered_once() {
        let mut seen = Default::default();
        let a = "https://a.example/v".to_string();
        assert_eq!(fresh(&mut seen, vec![a.clone()]), vec![a.clone()]);
        assert!(fresh(&mut seen, vec![a]).is_empty());
    }
}
