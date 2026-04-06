//! Logging middleware — logs action name and result status.

use crate::dispatcher::{DispatchError, DispatchResult};

use super::{ActionMiddleware, MiddlewareContext};

/// Logging middleware — logs action name and result status.
///
/// Uses `tracing::info!` / `tracing::warn!` to emit structured log lines.
/// Suitable for development and production environments.
pub struct LoggingMiddleware {
    /// Whether to log the full parameter payload (may be large; default: false).
    pub log_params: bool,
}

impl LoggingMiddleware {
    /// Create a new logging middleware (params not logged by default).
    #[must_use]
    pub fn new() -> Self {
        Self { log_params: false }
    }

    /// Create a logging middleware that also logs params.
    #[must_use]
    pub fn with_params() -> Self {
        Self { log_params: true }
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionMiddleware for LoggingMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        if self.log_params {
            tracing::info!(action = %ctx.action, params = %ctx.params, "dispatching action");
        } else {
            tracing::info!(action = %ctx.action, "dispatching action");
        }
        Ok(())
    }

    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        match result {
            Ok(_) => tracing::info!(action = %ctx.action, "action succeeded"),
            Err(e) => tracing::warn!(action = %ctx.action, error = %e, "action failed"),
        }
    }

    fn name(&self) -> &'static str {
        "logging"
    }
}
