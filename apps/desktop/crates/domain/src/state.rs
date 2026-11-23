use crate::model::DownloadStatus::{self, *};

/// Inputs to the job state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateEvent {
    /// Scheduler picked the job.
    Start,
    PhaseDownloading,
    PhaseProcessing,
    Finished,
    /// Failure with attempts below max.
    FailRetryable,
    FailFinal,
    Pause,
    Resume,
    Cancel,
    /// Manual retry of a failed/cancelled job.
    Retry,
    /// Startup recovery.
    Interrupted,
}

/// Side effects the caller must perform for a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Effects {
    /// `attempts += 1`.
    pub attempts_inc: bool,
    /// Set `next_attempt_at` from the backoff (automatic retry only).
    pub schedule_retry: bool,
    /// Kill the worker process tree.
    pub kill_worker: bool,
    /// Remove from the scheduler queue immediately (never reaches the engine).
    pub unqueue: bool,
    /// Delete known partial files (cancel); pause keeps them.
    pub delete_partials: bool,
    /// Manual retry: reset the backoff gate (`next_attempt_at = NULL`).
    pub clear_backoff: bool,
}

/// A (status, event) pair with no legal transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: DownloadStatus,
    pub event: StateEvent,
}

impl std::fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal transition: {} on {:?}", self.from, self.event)
    }
}
impl std::error::Error for IllegalTransition {}

impl StateEvent {
    /// Every event, in table column order.
    pub const ALL: [StateEvent; 11] = [
        Self::Start,
        Self::PhaseDownloading,
        Self::PhaseProcessing,
        Self::Finished,
        Self::FailRetryable,
        Self::FailFinal,
        Self::Pause,
        Self::Resume,
        Self::Cancel,
        Self::Retry,
        Self::Interrupted,
    ];
}

/// Pure transition function (architecture section 5). Never panics.
pub fn transition(
    from: DownloadStatus,
    ev: StateEvent,
) -> Result<(DownloadStatus, Effects), IllegalTransition> {
    use StateEvent::*;
    let fx = Effects::default();
    let kill = Effects {
        kill_worker: true,
        ..fx
    };
    let r = match (from, ev) {
        (Queued, Start) => (Probing, fx),
        (Queued, Pause) => (
            Paused,
            Effects {
                unqueue: true,
                ..fx
            },
        ),
        (Queued, Cancel) => (
            Cancelled,
            Effects {
                unqueue: true,
                ..fx
            },
        ),
        (Queued, Interrupted) => (Queued, fx),

        (Probing | Downloading | Processing, PhaseDownloading) => (Downloading, fx),
        (Probing | Downloading | Processing, PhaseProcessing) => (Processing, fx),
        (Probing | Downloading | Processing, Finished) => (Completed, fx),
        (Probing | Downloading | Processing, FailRetryable) => (
            Queued,
            Effects {
                attempts_inc: true,
                schedule_retry: true,
                ..fx
            },
        ),
        (Probing | Downloading | Processing, FailFinal) => (Failed, fx),
        (Probing | Downloading | Processing, Pause) => (Paused, kill),
        (Probing | Downloading | Processing, Cancel) => (
            Cancelled,
            Effects {
                delete_partials: true,
                ..kill
            },
        ),
        (Probing | Downloading | Processing, Interrupted) => (Paused, fx),

        (Paused, Pause | Interrupted) => (Paused, fx),
        (Paused, Resume) => (Queued, fx),
        (Paused, Cancel) => (
            Cancelled,
            Effects {
                delete_partials: true,
                ..fx
            },
        ),

        (Completed, Interrupted) => (Completed, fx),
        (Failed, Interrupted) => (Failed, fx),
        (Cancelled, Interrupted) => (Cancelled, fx),
        (Failed | Cancelled, Retry) => (
            Queued,
            Effects {
                attempts_inc: true,
                clear_backoff: true,
                ..fx
            },
        ),

        _ => return Err(IllegalTransition { from, event: ev }),
    };
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim copy of the table in architecture.md section 5 (column order = `StateEvent::ALL`).
    const TABLE: &[(&str, [&str; 11])] = &[
        (
            "queued",
            [
                "probing",
                "err",
                "err",
                "err",
                "err",
                "err",
                "paused",
                "err",
                "cancelled",
                "err",
                "queued",
            ],
        ),
        (
            "probing",
            [
                "err",
                "downloading",
                "processing",
                "completed",
                "queued",
                "failed",
                "paused",
                "err",
                "cancelled",
                "err",
                "paused",
            ],
        ),
        (
            "downloading",
            [
                "err",
                "downloading",
                "processing",
                "completed",
                "queued",
                "failed",
                "paused",
                "err",
                "cancelled",
                "err",
                "paused",
            ],
        ),
        (
            "processing",
            [
                "err",
                "downloading",
                "processing",
                "completed",
                "queued",
                "failed",
                "paused",
                "err",
                "cancelled",
                "err",
                "paused",
            ],
        ),
        (
            "paused",
            [
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "paused",
                "queued",
                "cancelled",
                "err",
                "paused",
            ],
        ),
        (
            "completed",
            [
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "completed",
            ],
        ),
        (
            "failed",
            [
                "err", "err", "err", "err", "err", "err", "err", "err", "err", "queued", "failed",
            ],
        ),
        (
            "cancelled",
            [
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "err",
                "queued",
                "cancelled",
            ],
        ),
    ];

    #[test]
    fn full_table_88_pairs() {
        assert_eq!(TABLE.len() * 11, 88);
        for (from, row) in TABLE {
            let from: DownloadStatus = from.parse().unwrap();
            for (ev, want) in StateEvent::ALL.iter().zip(row) {
                match (transition(from, *ev), *want) {
                    (Err(_), "err") => {}
                    (Ok((to, _)), w) if w != "err" => {
                        assert_eq!(to.as_str(), w, "{from} on {ev:?}")
                    }
                    (got, w) => panic!("{from} on {ev:?}: got {got:?}, want {w}"),
                }
            }
        }
    }

    #[test]
    fn effects() {
        let (_, e) = transition(Downloading, StateEvent::FailRetryable).unwrap();
        assert!(e.attempts_inc && e.schedule_retry && !e.kill_worker);
        let (_, e) = transition(Queued, StateEvent::Cancel).unwrap();
        assert!(e.unqueue && !e.kill_worker);
        let (_, e) = transition(Paused, StateEvent::Cancel).unwrap();
        assert!(e.delete_partials);
        let (_, e) = transition(Probing, StateEvent::Pause).unwrap();
        assert!(e.kill_worker && !e.delete_partials);
        let (_, e) = transition(Failed, StateEvent::Retry).unwrap();
        assert!(e.attempts_inc && e.clear_backoff && !e.schedule_retry);
    }
}
