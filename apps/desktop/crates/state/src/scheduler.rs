//! Pure scheduling: which queued rows may start now. Running counts are derived from the rows, so
//! nothing can leak a slot.
use std::collections::HashMap;

use domain::{DownloadId, DownloadStatus, DownloadSummary};

/// Concurrency limits (settings `concurrent_global` / `concurrent_per_domain`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub global: usize,
    pub per_domain: usize,
    /// Slots a penalised domain gets.
    pub penalty_slots: usize,
    /// How long a 429 penalty lasts.
    pub penalty_ms: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            global: 3,
            per_domain: 2,
            penalty_slots: 1,
            penalty_ms: 5 * 60 * 1000,
        }
    }
}

/// Holds the per-domain 429 penalty box.
#[derive(Debug, Default)]
pub struct Scheduler {
    penalties: HashMap<String, i64>,
}

impl Scheduler {
    /// A 429 happened on `domain`: its slots drop for `limits.penalty_ms`.
    pub fn penalize(&mut self, domain: &str, now_ms: i64, limits: &Limits) {
        self.penalties
            .insert(domain.to_string(), now_ms + limits.penalty_ms);
    }

    /// True while `domain` is in the penalty box.
    pub fn penalized(&self, domain: &str, now_ms: i64) -> bool {
        self.penalties
            .get(domain)
            .is_some_and(|until| *until > now_ms)
    }

    fn slots(&self, domain: &str, now_ms: i64, limits: &Limits) -> usize {
        if self.penalized(domain, now_ms) {
            limits.penalty_slots.min(limits.per_domain)
        } else {
            limits.per_domain
        }
    }

    /// Queued rows that may start now, in queue order (`position`), honouring the backoff gate.
    pub fn pick(
        &mut self,
        rows: &[DownloadSummary],
        now_ms: i64,
        limits: &Limits,
    ) -> Vec<DownloadId> {
        self.penalties.retain(|_, until| *until > now_ms);
        let mut per_domain: HashMap<&str, usize> = HashMap::new();
        let mut running = 0;
        for r in rows.iter().filter(|r| r.status.is_active()) {
            running += 1;
            *per_domain.entry(&r.domain).or_default() += 1;
        }
        let mut queued: Vec<&DownloadSummary> = rows
            .iter()
            .filter(|r| {
                r.status == DownloadStatus::Queued
                    && r.next_attempt_at.is_none_or(|at| at <= now_ms)
            })
            .collect();
        queued.sort_by(|a, b| a.position.total_cmp(&b.position));
        let mut out = vec![];
        for r in queued {
            if running >= limits.global {
                break;
            }
            let slots = self.slots(&r.domain, now_ms, limits);
            let used = per_domain.entry(&r.domain).or_default();
            if *used >= slots {
                continue;
            }
            *used += 1;
            running += 1;
            out.push(r.id);
        }
        out
    }
}
