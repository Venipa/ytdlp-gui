//! Worker IPC: newline-delimited JSON (architecture.md section 4, D-029).
//! Host to worker: one [`Command`] line at start, later `exit`. Worker to host: [`Event`] lines on stdout.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use domain::ErrorCode;

/// Host to worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Run one download. `spec` = yt-dlp opts (plain JSON, see `spec::to_opts`). Schema extension: `url` sits beside `spec`.
    Download { url: String, spec: Value },
    /// Graceful stop; the host kills after 1 s anyway.
    Exit,
}

/// Worker to host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum Event {
    /// Interpreter is up.
    Ready,
    /// At most 4 per second (the last one of a file is always sent).
    Progress {
        done: u64,
        total: Option<u64>,
        speed: Option<f64>,
        eta: Option<u64>,
        frag: Option<(u32, u32)>,
    },
    /// `downloading`, `merging` or `postprocess:<name>`.
    Phase { name: String },
    /// Final path candidate; the last one wins.
    Path { path: String },
    /// Partials and intermediates created (`*.part`, `*.ytdl`, `*.fNNN.*`), for cleanup.
    Files { paths: Vec<String> },
    /// yt-dlp logger or stray stdout/stderr line.
    Log { level: String, msg: String },
    /// Success. Schema extension: `path` (verified final path, also in `paths` for playlists) and a small `info` summary.
    Result {
        ok: bool,
        path: Option<String>,
        paths: Vec<String>,
        info: Value,
    },
    /// Failure, already classified.
    Error {
        code: ErrorCode,
        message: String,
        retryable: bool,
    },
}

impl Event {
    /// True for `Result` and `Error`: the worker is about to exit.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Event::Result { .. } | Event::Error { .. })
    }
}

/// Write one event line to stdout (worker side). Errors are ignored: a closed pipe means the host is gone.
pub fn emit(ev: &Event) {
    use std::io::Write;
    let mut line = serde_json::to_string(ev).expect("event serialises");
    line.push('\n');
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(line.as_bytes());
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_wire_format() {
        let e = Event::Progress {
            done: 10,
            total: Some(100),
            speed: Some(1.5),
            eta: None,
            frag: Some((1, 3)),
        };
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(
            s,
            r#"{"ev":"progress","done":10,"total":100,"speed":1.5,"eta":null,"frag":[1,3]}"#
        );
        assert_eq!(serde_json::from_str::<Event>(&s).unwrap(), e);
        assert_eq!(
            serde_json::to_string(&Event::Ready).unwrap(),
            r#"{"ev":"ready"}"#
        );
        let err: Event = serde_json::from_str(
            r#"{"ev":"error","code":"rate_limited","message":"m","retryable":true}"#,
        )
        .unwrap();
        assert!(err.is_terminal());
    }

    #[test]
    fn command_wire_format() {
        let c = Command::Download {
            url: "u".into(),
            spec: json!({"format":"b"}),
        };
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"cmd":"download","url":"u","spec":{"format":"b"}}"#
        );
        assert_eq!(
            serde_json::from_str::<Command>(r#"{"cmd":"exit"}"#).unwrap(),
            Command::Exit
        );
    }

    #[test]
    fn unknown_event_is_an_error_not_a_panic() {
        assert!(serde_json::from_str::<Event>(r#"{"ev":"nope"}"#).is_err());
    }
}
