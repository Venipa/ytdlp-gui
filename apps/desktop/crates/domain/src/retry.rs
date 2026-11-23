use crate::model::ErrorCode;

/// Time source, injected so tests run in microseconds.
pub trait Clock: Send + Sync {
    /// Unix milliseconds.
    fn now_ms(&self) -> i64;
}

/// Wall clock.
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

/// Settable clock for tests and the fake backend.
#[derive(Default)]
pub struct ManualClock(std::sync::atomic::AtomicI64);
impl ManualClock {
    /// Clock starting at `ms`.
    pub fn new(ms: i64) -> Self {
        Self(ms.into())
    }
    /// Moves time forward.
    pub fn advance_ms(&self, ms: i64) {
        self.0.fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
    }
}
impl Clock for ManualClock {
    fn now_ms(&self) -> i64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Random source for jitter: uniform in `[0, 1)`.
pub trait Jitter {
    fn unit(&mut self) -> f64;
}

/// Small xorshift64* generator (no rand dependency; jitter needs no quality).
pub struct XorShift(pub u64);
impl Jitter for XorShift {
    fn unit(&mut self) -> f64 {
        let mut x = self.0.max(1);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Default maximum attempts (settings `max_attempts`).
pub const MAX_ATTEMPTS: u32 = 5;

/// Seconds to wait before the next try after `attempt` failures (1-based):
/// `min(60, 15 * 2^(n-1))` with +-20% jitter; `retry_after_s` (HTTP Retry-After) overrides, unjittered.
pub fn backoff_secs(attempt: u32, retry_after_s: Option<u64>, rng: &mut impl Jitter) -> f64 {
    if let Some(r) = retry_after_s {
        return r as f64;
    }
    let base = (15.0 * 2f64.powi(attempt.saturating_sub(1).min(10) as i32)).min(60.0);
    base * (0.8 + 0.4 * rng.unit())
}

/// Absolute `next_attempt_at` (unix ms) for a retry.
pub fn next_attempt_at(
    clock: &dyn Clock,
    attempt: u32,
    retry_after_s: Option<u64>,
    rng: &mut impl Jitter,
) -> i64 {
    clock.now_ms() + (backoff_secs(attempt, retry_after_s, rng) * 1000.0) as i64
}

/// True when a failure with `code` after `attempts` tries (including this one) should retry.
pub fn should_retry(code: ErrorCode, attempts: u32, max_attempts: u32) -> bool {
    code.retryable() && attempts < max_attempts
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(f64);
    impl Jitter for Fixed {
        fn unit(&mut self) -> f64 {
            self.0
        }
    }

    #[test]
    fn backoff_curve_and_bounds() {
        // unit 0.5 = zero jitter
        let mid = |n| backoff_secs(n, None, &mut Fixed(0.5));
        assert_eq!(
            [mid(1), mid(2), mid(3), mid(4), mid(9)],
            [15.0, 30.0, 60.0, 60.0, 60.0]
        );
        assert!((backoff_secs(1, None, &mut Fixed(0.0)) - 12.0).abs() < 1e-9);
        assert!((backoff_secs(3, None, &mut Fixed(0.999999)) - 72.0).abs() < 1e-3);
        assert_eq!(backoff_secs(1, Some(120), &mut Fixed(0.0)), 120.0);
        let mut r = XorShift(42);
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&r.unit()));
        }
    }

    #[test]
    fn retry_rules_and_clock() {
        assert!(should_retry(ErrorCode::Network, 4, 5) && !should_retry(ErrorCode::Network, 5, 5));
        assert!(!should_retry(ErrorCode::Login, 1, 5) && !should_retry(ErrorCode::Private, 1, 5));
        let c = ManualClock::new(1_000);
        assert_eq!(next_attempt_at(&c, 1, None, &mut Fixed(0.5)), 16_000);
        c.advance_ms(500);
        assert_eq!(c.now_ms(), 1_500);
    }
}
