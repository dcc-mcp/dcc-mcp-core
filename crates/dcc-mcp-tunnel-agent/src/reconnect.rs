//! Reconnect loop with [`ReconnectPolicy`]-driven back-off.
//!
//! [`run_with_reconnect`] wraps [`crate::run_once`] in an outer loop that
//! drives reconnection forever (the agent never gives up; see
//! [`crate::ReconnectPolicy`] for the rationale).
//!
//! Two delays are tracked:
//!
//! 1. **Backoff** — applied between consecutive *failed* attempts. Doubles
//!    each cycle for [`ReconnectPolicy::Exponential`], capped at `max`.
//! 2. **Reset window** — a successful registration resets the backoff to
//!    `initial`, so a long-lived tunnel that drops once doesn't have to
//!    crawl back from a 60-second wait.
//!
//! Cancellation is via [`tokio_util::sync::CancellationToken`]-style
//! signals expressed as a `tokio::sync::watch::Receiver<bool>` — when the
//! watch flips to `true` the loop exits at the next decision point. This
//! keeps the dependency surface to `tokio` only, in line with the agent
//! crate's no-extra-deps stance.

use std::time::Duration;

use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::client::{ClientError, run_once};
use crate::config::{AgentConfig, ReconnectPolicy};

/// Outcome of [`run_with_reconnect`].
#[derive(Debug)]
pub enum ReconnectExit {
    /// The supplied shutdown watch flipped to `true`.
    Shutdown,
    /// A non-retryable error was hit (currently: relay rejected the
    /// registration with `ok=false`, which usually means a misconfigured
    /// JWT and should not be papered over with infinite retries).
    Fatal(ClientError),
}

/// Run the agent forever, reconnecting on transport / handshake errors.
///
/// `shutdown_rx` is polled on every reconnect-decision boundary; flip the
/// corresponding sender to `true` to wind down cleanly.
pub async fn run_with_reconnect(
    config: AgentConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) -> ReconnectExit {
    let mut current_delay = initial_delay(&config.reconnect);
    loop {
        if *shutdown_rx.borrow() {
            return ReconnectExit::Shutdown;
        }
        info!(relay = %config.relay_url, "agent connecting");
        let outcome = tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    return ReconnectExit::Shutdown;
                }
                continue;
            }
            r = run_once(config.clone()) => r,
        };
        match outcome {
            Ok(reg) => {
                info!(tunnel_id = %reg.tunnel_id, "agent disconnected; reconnecting");
                current_delay = initial_delay(&config.reconnect);
            }
            Err(e @ ClientError::Rejected(_)) => {
                warn!(error = %e, "relay rejected registration; not retrying");
                return ReconnectExit::Fatal(e);
            }
            Err(e) => {
                warn!(error = %e, delay_ms = current_delay.as_millis() as u64, "agent failed; backing off");
            }
        }
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    return ReconnectExit::Shutdown;
                }
            }
            _ = tokio::time::sleep(current_delay) => {}
        }
        current_delay = next_delay(&config.reconnect, current_delay);
        debug!(
            next_delay_ms = current_delay.as_millis() as u64,
            "advanced backoff"
        );
    }
}

/// Starting delay for the policy. Used both at start-up and after every
/// successful registration.
pub fn initial_delay(policy: &ReconnectPolicy) -> Duration {
    match policy {
        ReconnectPolicy::Constant { delay } => *delay,
        ReconnectPolicy::Exponential { initial, .. } => *initial,
    }
}

/// Successor delay after the previous attempt failed.
pub fn next_delay(policy: &ReconnectPolicy, prev: Duration) -> Duration {
    match policy {
        ReconnectPolicy::Constant { delay } => *delay,
        ReconnectPolicy::Exponential { max, initial } => {
            // Doubling ceiling-clamped; saturate on overflow to dodge the
            // pathological u64::MAX-second case if `prev` ever ends up
            // close to it (would need a misconfigured `max`, but defending
            // is cheap).
            let doubled = prev.saturating_mul(2);
            let bounded = doubled.min(*max);
            // If the operator's `max` is smaller than `initial`, prefer
            // `initial` so we never go below the configured minimum.
            bounded.max(*initial)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_policy_keeps_delay_stable() {
        let p = ReconnectPolicy::Constant {
            delay: Duration::from_millis(250),
        };
        assert_eq!(initial_delay(&p), Duration::from_millis(250));
        assert_eq!(
            next_delay(&p, Duration::from_millis(999)),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn exponential_policy_doubles_then_caps() {
        let p = ReconnectPolicy::Exponential {
            initial: Duration::from_millis(100),
            max: Duration::from_millis(800),
        };
        let mut d = initial_delay(&p);
        assert_eq!(d, Duration::from_millis(100));
        d = next_delay(&p, d);
        assert_eq!(d, Duration::from_millis(200));
        d = next_delay(&p, d);
        assert_eq!(d, Duration::from_millis(400));
        d = next_delay(&p, d);
        assert_eq!(d, Duration::from_millis(800));
        d = next_delay(&p, d);
        assert_eq!(d, Duration::from_millis(800)); // capped
    }

    #[test]
    fn exponential_policy_never_drops_below_initial() {
        let p = ReconnectPolicy::Exponential {
            initial: Duration::from_millis(500),
            max: Duration::from_millis(100), // misconfigured: max < initial
        };
        let d = next_delay(&p, Duration::from_millis(50));
        assert_eq!(d, Duration::from_millis(500));
    }
}
