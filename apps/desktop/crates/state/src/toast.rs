use domain::DownloadId;
use gpui::Context;

/// Severity of a toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

/// A transient message. `message` is a Fluent key or ready text chosen by the pusher; the view
/// decides how long it stays (the model keeps it until dismissed).
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub message: String,
    /// The download an action on the toast (Show in folder, Retry) refers to.
    pub download: Option<DownloadId>,
}

/// Toast stack.
#[derive(Default)]
pub struct Toasts {
    next: u64,
    items: Vec<Toast>,
}

impl Toasts {
    /// Pushes a toast and returns its id.
    pub fn push(
        &mut self,
        kind: ToastKind,
        message: impl Into<String>,
        download: Option<DownloadId>,
        cx: &mut Context<Self>,
    ) -> u64 {
        self.next += 1;
        self.items.push(Toast {
            id: self.next,
            kind,
            message: message.into(),
            download,
        });
        cx.notify();
        self.next
    }

    /// Removes a toast.
    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.items.retain(|t| t.id != id);
        cx.notify();
    }

    /// Current toasts, oldest first.
    pub fn items(&self) -> &[Toast] {
        &self.items
    }
}
