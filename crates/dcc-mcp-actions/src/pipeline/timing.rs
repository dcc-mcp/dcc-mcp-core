//! Timing middleware — measures and records action execution latency.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::dispatcher::{DispatchError, DispatchResult};

use super::{ActionMiddleware, MiddlewareContext};

/// Timing middleware — measures and records action execution latency.
///
/// Stores the start time in `ctx.extensions["timing.start_ns"]` and
/// the elapsed duration in `ctx.extensions["timing.elapsed_ms"]` (available
/// in `after_dispatch` via a shared state mechanism).
pub struct TimingMiddleware {
    /// Shared per-call timers (action → start Instant).
    ///
    /// Using a Mutex<HashMap> instead of thread-local to support both
    /// single-threaded DCC main loops and multi-threaded test environments.
    timers: Mutex<HashMap<String, Instant>>,
}

impl TimingMiddleware {
    /// Create a new timing middleware.
    #[must_use]
    pub fn new() -> Self {
        Self {
            timers: Mutex::new(HashMap::new()),
        }
    }

    /// Get the last recorded elapsed time for an action (for test assertions).
    #[must_use]
    pub fn last_elapsed(&self, action: &str) -> Option<Duration> {
        let timers = self.timers.lock().expect("timing lock poisoned");
        timers.get(action).map(|start| start.elapsed())
    }
}

impl Default for TimingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionMiddleware for TimingMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        let start = Instant::now();
        let mut timers = self.timers.lock().expect("timing lock poisoned");
        timers.insert(ctx.action.clone(), start);
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
        let elapsed_ms = {
            let timers = self.timers.lock().expect("timing lock poisoned");
            timers
                .get(&ctx.action)
                .map(|start| start.elapsed().as_millis() as u64)
                .unwrap_or(0)
        };
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
