//! `CustomArgs`: what the check of each custom argument line found (Settings, Network, Custom
//! arguments) and the arguments a download takes. yt-dlp lines go to the engine, which asks
//! yt-dlp's own parser; aria2c lines are checked here, for their shape.
use std::collections::HashMap;
use std::sync::Arc;

use domain::args::{check_aria2c, split};
use domain::{ArgCheck, Backend};
use gpui::{Context, Task};

/// The answer for a line whose quotes do not close; yt-dlp is never asked about it.
fn unbalanced() -> ArgCheck {
    ArgCheck::Invalid {
        message: "a quote is not closed".into(),
        suggestion: None,
    }
}

/// The arguments of the lines a download may use: a line with a mark is left out, a line still
/// being checked goes in (it was typed a moment ago and most lines are fine).
fn usable(lines: &[String], check: impl Fn(&str) -> Option<ArgCheck>) -> Vec<String> {
    lines
        .iter()
        .filter(|line| check(line).is_none_or(|c| c == ArgCheck::Ok))
        .filter_map(|line| split(line))
        .flatten()
        .collect()
}

/// Checks of custom argument lines, kept per line text.
pub struct CustomArgs {
    backend: Arc<dyn Backend>,
    checked: HashMap<String, ArgCheck>,
    running: Option<Task<()>>,
}

impl CustomArgs {
    /// No line checked yet.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            checked: HashMap::new(),
            running: None,
        }
    }

    /// The answer for a yt-dlp line; `None` while yt-dlp has not answered.
    pub fn ytdlp(&self, line: &str) -> Option<ArgCheck> {
        match split(line) {
            None => Some(unbalanced()),
            Some(args) if args.is_empty() => Some(ArgCheck::Ok),
            Some(_) => self.checked.get(line).cloned(),
        }
    }

    /// The answer for an aria2c line.
    pub fn aria2c(&self, line: &str) -> ArgCheck {
        split(line).map_or_else(unbalanced, |args| check_aria2c(&args))
    }

    /// True while yt-dlp is reading lines.
    pub fn checking(&self) -> bool {
        self.running.is_some()
    }

    /// Asks yt-dlp about the lines it has not answered for yet. A newer call replaces a running
    /// one; the lines that one carried are asked again.
    pub fn check(&mut self, lines: &[String], cx: &mut Context<Self>) {
        let mut todo: Vec<String> = lines
            .iter()
            .filter(|l| self.ytdlp(l).is_none())
            .cloned()
            .collect();
        todo.dedup();
        if todo.is_empty() {
            return;
        }
        let call = self
            .backend
            .check_args(todo.iter().filter_map(|l| split(l)).collect());
        self.running = Some(cx.spawn(async move |this, cx| {
            let answers = call.await;
            this.update(cx, |this, cx| {
                this.checked.extend(todo.into_iter().zip(answers));
                this.running = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The yt-dlp arguments a download takes from `lines`.
    pub fn ytdlp_usable(&self, lines: &[String]) -> Vec<String> {
        usable(lines, |l| self.ytdlp(l))
    }

    /// The aria2c arguments a download takes from `lines`.
    pub fn aria2c_usable(&self, lines: &[String]) -> Vec<String> {
        usable(lines, |l| Some(self.aria2c(l)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_lines_stay_out_and_unchecked_ones_go_in() {
        let lines: Vec<String> = [
            "--sleep-interval 2",
            "--no-mtmie",
            "--format best",
            "--user-agent 'My Agent'",
            "--x \"open",
        ]
        .map(String::from)
        .to_vec();
        let check = |l: &str| match l {
            "--sleep-interval 2" => Some(ArgCheck::Ok),
            "--no-mtmie" => Some(ArgCheck::Invalid {
                message: "no such option".into(),
                suggestion: None,
            }),
            "--format best" => Some(ArgCheck::Reserved),
            "--x \"open" => Some(unbalanced()),
            _ => None,
        };
        assert_eq!(
            usable(&lines, check),
            ["--sleep-interval", "2", "--user-agent", "My Agent"]
        );
    }
}
