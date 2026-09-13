//! Timing middleware — measures and records action execution latency.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

use crate::dispatcher::{DispatchError, DispatchResult};

use super::{ActionMiddleware, MiddlewareContext};

/// Timing middleware — measures and records action execution latency.
///
/// Each context owns its monotonic start offset. Completed durations are
/// retained separately so reads and rejected dispatches cannot change them.
pub struct TimingMiddleware {
    epoch: Instant,
    completed: Mutex<HashMap<String, Duration>>,
}

impl TimingMiddleware {
    /// Create a new timing middleware.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            completed: Mutex::new(HashMap::new()),
        }
    }

    /// Get the last recorded elapsed time for an action (for test assertions).
    #[must_use]
    pub fn last_elapsed(&self, action: &str) -> Option<Duration> {
        self.completed.lock().get(action).copied()
    }
}

impl Default for TimingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionMiddleware for TimingMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        let start_ns = u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
        ctx.insert("timing.start_ns", Value::Number(start_ns.into()));
        // Record start time in extensions as epoch milliseconds (u64)
        let start_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        ctx.insert("timing.start_ms", Value::Number(start_ms.into()));
        Ok(())
    }

    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        _result: Result<&DispatchResult, &DispatchError>,
    ) {
        let Some(start_ns) = ctx.get("timing.start_ns").and_then(Value::as_u64) else {
            return;
        };
        let elapsed = self
            .epoch
            .elapsed()
            .saturating_sub(Duration::from_nanos(start_ns));
        self.completed.lock().insert(ctx.action.clone(), elapsed);
        let elapsed_ms = elapsed.as_millis() as u64;
        tracing::debug!(
            action = %ctx.action,
            elapsed_ms = elapsed_ms,
            "action timing"
        );
    }

    fn name(&self) -> &'static str {
        "timing"
    }
}
