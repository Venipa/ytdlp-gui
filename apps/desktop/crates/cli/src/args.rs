//! Clap pieces the `ytdlpd` binary flattens into its own arguments.
use clap::{Args, Subcommand};

use crate::deeplink;

/// Launch flags besides the app's own (`--data-dir`, `--window-size`).
#[derive(Args, Clone, Debug, Default)]
pub struct Launch {
    /// Start hidden in the tray.
    #[arg(long)]
    pub minimized: bool,
    /// Open a demo scene (handed to the demo layer as is).
    #[arg(long, hide = true, value_name = "SCENE")]
    pub demo: Option<String>,
    /// Log frame timings.
    #[arg(long, hide = true)]
    pub frame_stats: bool,
}

/// Subcommands.
#[derive(Subcommand, Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Add links to the running app, starting it when needed. Also receives `ytdlpd://add?url=`
    /// links from the operating system.
    Add {
        #[arg(required = true, value_name = "URL")]
        urls: Vec<String>,
    },
}

impl Command {
    /// The valid http(s) links this command carries; invalid ones are dropped with a warning.
    pub fn urls(&self) -> Vec<String> {
        let Command::Add { urls } = self;
        deeplink::links(urls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct App {
        #[command(flatten)]
        launch: Launch,
        #[command(subcommand)]
        command: Option<Command>,
    }

    #[test]
    fn add_and_flags_parse() {
        let a = App::try_parse_from([
            "x",
            "--minimized",
            "add",
            "https://a.b/c",
            "ytdlpd://add?url=https%3A%2F%2Fd.e",
        ])
        .unwrap();
        assert!(a.launch.minimized);
        assert_eq!(a.command.unwrap().urls(), ["https://a.b/c", "https://d.e/"]);
        assert!(App::try_parse_from(["x", "add"]).is_err());
        let a = App::try_parse_from(["x", "--demo", "busy", "--frame-stats"]).unwrap();
        assert_eq!(a.launch.demo.as_deref(), Some("busy"));
        assert!(a.launch.frame_stats && a.command.is_none());
    }
}
