use std::collections::VecDeque;

use domain::{Clock, DownloadId, SystemClock};
use gpui::Context;

/// Lines kept in memory for the Logs screen.
const CAP: usize = 5000;

/// `text` without terminal colour codes (`ESC [ ... m` and the like), which tools such as deno
/// print even when their output is captured.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
        } else if chars.next() == Some('[') {
            // a control sequence ends at its first byte in `@`..=`~`
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

/// One line on the Logs screen.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub at: i64,
    /// The download the line belongs to; `None` for app lines.
    pub job: Option<DownloadId>,
    pub level: String,
    pub message: String,
}

/// In-memory ring of recent log lines (per-job history lives in `download_logs`).
#[derive(Default)]
pub struct Logs {
    entries: VecDeque<LogEntry>,
}

impl Logs {
    /// Appends a line, dropping the oldest beyond the cap.
    pub fn push(
        &mut self,
        job: Option<DownloadId>,
        level: &str,
        message: &str,
        cx: &mut Context<Self>,
    ) {
        if self.entries.len() == CAP {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry {
            at: SystemClock.now_ms(),
            job,
            level: level.to_string(),
            message: plain(message),
        });
        cx.notify();
    }

    /// All lines, oldest first.
    pub fn entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }

    /// Lines of one download.
    pub fn for_job(&self, id: DownloadId) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter().filter(move |e| e.job == Some(id))
    }

    /// Number of lines kept.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no line is kept.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Empties the ring.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.entries.clear();
        cx.notify();
    }
}

#[cfg(test)]
mod plain_tests {
    #[test]
    fn colour_codes_go() {
        assert_eq!(
            super::plain(
                "\u{1b}[0m\u{1b}[1m\u{1b}[31merror\u{1b}[0m: at \u{1b}[38;5;245mfile\u{1b}[0m"
            ),
            "error: at file"
        );
        assert_eq!(super::plain("plain [0m text"), "plain [0m text");
    }
}
