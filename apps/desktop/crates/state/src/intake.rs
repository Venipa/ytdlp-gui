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
}

impl EventEmitter<IntakeEvent> for Intake {}

impl Intake {
    /// Links to add; only http and https survive (`split_urls`).
    pub fn add(&mut self, urls: &[String], cx: &mut Context<Self>) {
        let urls = split_urls(&urls.join("\n")).urls;
        if urls.is_empty() {
            return;
        }
        self.pending.extend(urls.iter().cloned());
        cx.emit(IntakeEvent::Add(urls));
        cx.notify();
    }

    /// Links seen on the clipboard.
    pub fn offer(&mut self, urls: &[String], cx: &mut Context<Self>) {
        let urls = split_urls(&urls.join("\n")).urls;
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
