//! Core middleware traits.

use std::future::Future;
use std::pin::Pin;

use super::context::{CallContext, CallResult};
use super::error::MiddlewareError;
use super::governance::MiddlewareGovernanceControl;

/// Type alias for a boxed async middleware future.
pub type MiddlewareFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MiddlewareError>> + Send + 'a>>;

/// Runs synchronously/asynchronously **before** a `tools/call` is dispatched.
///
/// Implementors may:
/// - Inspect or mutate `ctx.args` (e.g. redaction).
/// - Record audit state into `ctx.metadata`.
/// - Abort the pipeline by returning `Err(MiddlewareError::*)`.
pub trait BeforeCallMiddleware: Send + Sync {
    fn before_call<'a>(&'a self, ctx: &'a mut CallContext) -> MiddlewareFuture<'a, ()>;

    /// Optional read-only operator-facing descriptor.
    fn governance(&self) -> Option<MiddlewareGovernanceControl> {
        None
    }
}

/// Runs **after** a `tools/call` response is available, before it is serialised
/// and sent to the client.
///
/// Implementors may:
/// - Inspect `ctx` and `result` for audit logging.
/// - Mutate `result.text` for response transformation.
/// - Return `Err` to replace the response with an error.
pub trait AfterCallMiddleware: Send + Sync {
    fn after_call<'a>(
        &'a self,
        ctx: &'a CallContext,
        result: &'a mut CallResult,
    ) -> MiddlewareFuture<'a, ()>;

    /// Optional read-only operator-facing descriptor.
    fn governance(&self) -> Option<MiddlewareGovernanceControl> {
        None
    }
}
