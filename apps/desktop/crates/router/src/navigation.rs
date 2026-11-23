use gpui::{Context, EventEmitter};

use crate::{Destination, SettingsPage};

/// Emitted whenever the current page changes.
pub enum NavigationEvent {
    Moved(Destination),
}

/// The back and forward trail of pages.
pub struct Navigation {
    trail: Vec<Destination>,
    at: usize,
    /// The settings destination last on show, which opening settings again returns to.
    last_settings: Option<Destination>,
}

impl EventEmitter<NavigationEvent> for Navigation {}

impl Navigation {
    pub fn new(start: Destination) -> Self {
        Self {
            trail: vec![start],
            at: 0,
            last_settings: None,
        }
    }

    pub fn current(&self) -> Destination {
        self.trail[self.at]
    }

    pub fn can_go_back(&self) -> bool {
        self.at > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.at + 1 < self.trail.len()
    }

    pub fn go(&mut self, destination: Destination, cx: &mut Context<Self>) {
        if self.current() == destination {
            return;
        }
        self.trail.truncate(self.at + 1);
        self.trail.push(destination);
        self.at = self.trail.len() - 1;
        self.arrive(cx);
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        if !self.can_go_back() {
            return;
        }
        self.at -= 1;
        self.arrive(cx);
    }

    /// Replaces the current page without leaving its stale destination in navigation history.
    pub fn replace(&mut self, destination: Destination, cx: &mut Context<Self>) {
        if self.current() == destination {
            return;
        }
        self.trail[self.at] = destination;
        self.arrive(cx);
    }

    /// Opens settings on the destination last shown there, or on General the first time.
    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        let destination = self
            .last_settings
            .unwrap_or(Destination::Settings(SettingsPage::General));
        self.go(destination, cx);
    }

    pub fn forward(&mut self, cx: &mut Context<Self>) {
        if !self.can_go_forward() {
            return;
        }
        self.at += 1;
        self.arrive(cx);
    }

    fn arrive(&mut self, cx: &mut Context<Self>) {
        let current = self.current();
        if current.in_settings() {
            self.last_settings = Some(current);
        }
        cx.emit(NavigationEvent::Moved(current));
        cx.notify();
    }
}
