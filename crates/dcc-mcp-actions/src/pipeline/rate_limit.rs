//! Rate limiting middleware — limits calls per action per time window.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::dispatcher::DispatchError;

use super::{ActionMiddleware, MiddlewareContext};

/// Rate limiting middleware — limits calls per action per time window.
///
/// Uses a token-bucket approach (simplified: fixed window counter).
/// Rejects requests that exceed `max_calls` within `window`.
pub struct RateLimitMiddleware {
    /// Maximum allowed calls per action per window.
    max_calls: u64,
    /// Time window for rate limiting.
    window: Duration,
    /// Per-action counters and window start times.
    state: Mutex<HashMap<String, (u64, Instant)>>,
}

impl RateLimitMiddleware {
    /// Create a new rate limiter: at most `max_calls` per `window`.
    #[must_use]
    pub fn new(max_calls: u64, window: Duration) -> Self {
        Self {
            max_calls,
            window,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Get the current call count for an action (for testing).
    #[must_use]
    pub fn call_count(&self, action: &str) -> u64 {
        let state = self.state.lock();
        state.get(action).map(|(count, _)| *count).unwrap_or(0)
    }
}

impl ActionMiddleware for RateLimitMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        let mut state = self.state.lock();
        let now = Instant::now();

        let entry = state.entry(ctx.action.clone()).or_insert_with(|| (0, now));

        // Reset window if expired
        if entry.1.elapsed() >= self.window {
            *entry = (0, now);
        }

        entry.0 += 1;

        if entry.0 > self.max_calls {
            return Err(DispatchError::HandlerError(format!(
                "rate limit exceeded for action '{}': {} calls in {:?} (max {})",
                ctx.action,
                entry.0 - 1,
                self.window,
                self.max_calls
            )));
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "rate_limit"
    }
}
