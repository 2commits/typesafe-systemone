use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Retry behaviour for transient failures.
///
/// Retried: HTTP 408, 429, every 5xx (including 529 Overloaded), connection errors and
/// timeouts. Not retried: 4xx other than 408/429. Defaults mirror TypeSafe's official SDKs.
#[derive(Clone, Debug, PartialEq)]
pub struct RetryPolicy {
    /// Retries after the first attempt. `0` disables retrying.
    pub max_retries: u32,
    /// First backoff delay; doubled on each retry up to `backoff_max`.
    pub backoff_initial: Duration,
    /// Upper bound for a backoff delay.
    pub backoff_max: Duration,
    /// Fraction of each delay randomly subtracted, in `0.0..=1.0`.
    pub backoff_jitter: f64,
    /// Honour a `retry-after` header when the server sends one.
    pub respect_retry_after: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            backoff_initial: Duration::from_millis(500),
            backoff_max: Duration::from_secs(5),
            backoff_jitter: 0.25,
            respect_retry_after: true,
        }
    }
}

impl RetryPolicy {
    /// Never retry.
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Self::default()
        }
    }

    pub(crate) fn is_retryable_status(status: u16) -> bool {
        status == 408 || status == 429 || (500..=599).contains(&status)
    }

    /// Delay before retry number `retry` (1-based). `retry_after` wins when honoured.
    pub(crate) fn delay(&self, retry: u32, retry_after: Option<Duration>) -> Duration {
        if self.respect_retry_after {
            if let Some(d) = retry_after {
                return d.min(self.backoff_max.max(d));
            }
        }
        let exp = self
            .backoff_initial
            .saturating_mul(2u32.saturating_pow(retry.saturating_sub(1)));
        let base = exp.min(self.backoff_max);
        let jitter = self.backoff_jitter.clamp(0.0, 1.0);
        if jitter == 0.0 {
            return base;
        }
        base.mul_f64(1.0 - jitter * unit_random())
    }
}

/// Cheap uniform random in `[0, 1)` without a `rand` dependency; jitter needs no quality.
fn unit_random() -> f64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    (x % 10_000) as f64 / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_statuses() {
        for s in [408, 429, 500, 502, 529, 599] {
            assert!(RetryPolicy::is_retryable_status(s), "{s}");
        }
        for s in [200, 400, 401, 403, 404, 422] {
            assert!(!RetryPolicy::is_retryable_status(s), "{s}");
        }
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let p = RetryPolicy {
            backoff_jitter: 0.0,
            ..RetryPolicy::default()
        };
        assert_eq!(p.delay(1, None), Duration::from_millis(500));
        assert_eq!(p.delay(2, None), Duration::from_millis(1000));
        assert_eq!(p.delay(3, None), Duration::from_millis(2000));
        assert_eq!(p.delay(10, None), Duration::from_secs(5));
    }

    #[test]
    fn jitter_only_shortens() {
        let p = RetryPolicy::default();
        for _ in 0..50 {
            let d = p.delay(1, None);
            assert!(
                d <= Duration::from_millis(500) && d >= Duration::from_millis(375),
                "{d:?}"
            );
        }
    }

    #[test]
    fn retry_after_wins_when_respected() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay(1, Some(Duration::from_secs(3))), Duration::from_secs(3));
        let p = RetryPolicy {
            respect_retry_after: false,
            backoff_jitter: 0.0,
            ..RetryPolicy::default()
        };
        assert_eq!(p.delay(1, Some(Duration::from_secs(3))), Duration::from_millis(500));
    }
}
