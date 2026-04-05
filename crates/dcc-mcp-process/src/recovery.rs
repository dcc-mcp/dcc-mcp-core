//! Crash-recovery policy for DCC processes.
//!
//! `CrashRecoveryPolicy` encapsulates the restart strategy (max attempts,
//! back-off delay) but does **not** own any I/O resources — it is a pure
//! decision engine.  The actual re-spawn is performed by `DccLauncher`.

use std::time::Duration;

use tracing::{info, warn};

use crate::error::ProcessError;
use crate::types::{DccProcessConfig, ProcessStatus};

/// Back-off strategy used between restart attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffStrategy {
    /// Wait a fixed number of milliseconds between every attempt.
    Fixed { delay_ms: u64 },
    /// Double the delay after each attempt (capped at `max_delay_ms`).
    Exponential { initial_ms: u64, max_delay_ms: u64 },
}

impl BackoffStrategy {
    /// Compute the delay for the nth attempt (0-indexed).
    #[must_use]
    pub fn delay_for(&self, attempt: u32) -> Duration {
        match self {
            Self::Fixed { delay_ms } => Duration::from_millis(*delay_ms),
            Self::Exponential {
                initial_ms,
                max_delay_ms,
            } => {
                let shift = attempt.min(62);
                let multiplier = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
                let raw = initial_ms.saturating_mul(multiplier);
                Duration::from_millis(raw.min(*max_delay_ms))
            }
        }
    }
}

/// Encapsulates the restart decision logic for a crashed DCC process.
///
/// This is intentionally side-effect-free so it can be unit-tested without
/// spawning actual processes.
#[derive(Debug, Clone)]
pub struct CrashRecoveryPolicy {
    /// Maximum number of restart attempts before giving up.
    pub max_restarts: u32,
    /// Back-off strategy between attempts.
    pub backoff: BackoffStrategy,
}

impl CrashRecoveryPolicy {
    /// Sensible defaults: 3 restarts with 2 s fixed back-off.
    pub fn new(max_restarts: u32) -> Self {
        Self {
            max_restarts,
            backoff: BackoffStrategy::Fixed { delay_ms: 2_000 },
        }
    }

    /// Use exponential back-off starting at `initial_ms`, capped at `max_delay_ms`.
    pub fn with_exponential_backoff(mut self, initial_ms: u64, max_delay_ms: u64) -> Self {
        self.backoff = BackoffStrategy::Exponential {
            initial_ms,
            max_delay_ms,
        };
        self
    }

    /// Decide whether `attempt` (0-indexed) should proceed or give up.
    ///
    /// Returns the `Duration` to sleep before the next launch, or
    /// `ProcessError::MaxRestartsExceeded` if the limit has been reached.
    pub fn next_restart_delay(
        &self,
        config: &DccProcessConfig,
        attempt: u32,
    ) -> Result<Duration, ProcessError> {
        if attempt >= self.max_restarts {
            warn!(
                name = %config.name,
                attempt,
                max = self.max_restarts,
                "max restarts exceeded"
            );
            return Err(ProcessError::MaxRestartsExceeded {
                name: config.name.clone(),
                max_restarts: self.max_restarts,
            });
        }

        let delay = self.backoff.delay_for(attempt);
        info!(
            name = %config.name,
            attempt,
            delay_ms = delay.as_millis(),
            "scheduling restart"
        );
        Ok(delay)
    }

    /// Determine whether the given exit-code / status warrants a restart.
    ///
    /// Convention:
    /// - `ProcessStatus::Crashed` or `ProcessStatus::Unresponsive` → should restart
    /// - `ProcessStatus::Stopped` (clean exit) → no restart
    #[must_use]
    pub fn should_restart(&self, status: ProcessStatus) -> bool {
        matches!(status, ProcessStatus::Crashed | ProcessStatus::Unresponsive)
    }
}

impl Default for CrashRecoveryPolicy {
    fn default() -> Self {
        Self::new(3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_backoff_strategy {
        use super::*;

        #[test]
        fn fixed_always_returns_same_delay() {
            let strategy = BackoffStrategy::Fixed { delay_ms: 500 };
            for attempt in 0..10 {
                assert_eq!(strategy.delay_for(attempt), Duration::from_millis(500));
            }
        }

        #[test]
        fn exponential_doubles_each_attempt() {
            let strategy = BackoffStrategy::Exponential {
                initial_ms: 100,
                max_delay_ms: 10_000,
            };
            assert_eq!(strategy.delay_for(0), Duration::from_millis(100));
            assert_eq!(strategy.delay_for(1), Duration::from_millis(200));
            assert_eq!(strategy.delay_for(2), Duration::from_millis(400));
            assert_eq!(strategy.delay_for(3), Duration::from_millis(800));
        }

        #[test]
        fn exponential_caps_at_max() {
            let strategy = BackoffStrategy::Exponential {
                initial_ms: 1_000,
                max_delay_ms: 5_000,
            };
            // 2^3 * 1000 = 8000 > 5000 → should cap
            assert_eq!(strategy.delay_for(3), Duration::from_millis(5_000));
            assert_eq!(strategy.delay_for(10), Duration::from_millis(5_000));
        }

        #[test]
        fn exponential_no_overflow_on_large_attempt() {
            let strategy = BackoffStrategy::Exponential {
                initial_ms: 1,
                max_delay_ms: u64::MAX,
            };
            // attempt=63 would overflow a naive 1 << attempt; saturating_shl handles it
            let _ = strategy.delay_for(63);
            let _ = strategy.delay_for(100);
        }
    }

    mod test_crash_recovery_policy {
        use super::*;

        fn make_config(name: &str) -> DccProcessConfig {
            DccProcessConfig::new(name, "/usr/bin/dummy")
        }

        #[test]
        fn default_allows_three_restarts() {
            let policy = CrashRecoveryPolicy::default();
            assert_eq!(policy.max_restarts, 3);
        }

        #[test]
        fn next_restart_delay_below_max_ok() {
            let policy = CrashRecoveryPolicy::new(3);
            let cfg = make_config("maya");
            assert!(policy.next_restart_delay(&cfg, 0).is_ok());
            assert!(policy.next_restart_delay(&cfg, 1).is_ok());
            assert!(policy.next_restart_delay(&cfg, 2).is_ok());
        }

        #[test]
        fn next_restart_delay_at_max_errors() {
            let policy = CrashRecoveryPolicy::new(3);
            let cfg = make_config("maya");
            let result = policy.next_restart_delay(&cfg, 3);
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                ProcessError::MaxRestartsExceeded { .. }
            ));
        }

        #[test]
        fn with_exponential_backoff_builder() {
            let policy = CrashRecoveryPolicy::new(5).with_exponential_backoff(500, 30_000);
            let cfg = make_config("blender");
            let delay0 = policy.next_restart_delay(&cfg, 0).unwrap();
            let delay1 = policy.next_restart_delay(&cfg, 1).unwrap();
            assert_eq!(delay0, Duration::from_millis(500));
            assert_eq!(delay1, Duration::from_millis(1_000));
        }

        #[test]
        fn should_restart_crashed() {
            let policy = CrashRecoveryPolicy::default();
            assert!(policy.should_restart(ProcessStatus::Crashed));
        }

        #[test]
        fn should_restart_unresponsive() {
            let policy = CrashRecoveryPolicy::default();
            assert!(policy.should_restart(ProcessStatus::Unresponsive));
        }

        #[test]
        fn should_not_restart_clean_stop() {
            let policy = CrashRecoveryPolicy::default();
            assert!(!policy.should_restart(ProcessStatus::Stopped));
        }

        #[test]
        fn should_not_restart_running() {
            let policy = CrashRecoveryPolicy::default();
            assert!(!policy.should_restart(ProcessStatus::Running));
        }

        #[test]
        fn max_restarts_exceeded_error_contains_name() {
            let policy = CrashRecoveryPolicy::new(1);
            let cfg = make_config("houdini");
            let err = policy.next_restart_delay(&cfg, 1).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("houdini"),
                "error message should contain process name"
            );
        }
    }
}
