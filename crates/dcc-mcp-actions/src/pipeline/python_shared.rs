//! Arc newtype wrappers that implement [`ActionMiddleware`] so the pipeline
//! can share one middleware instance between the Python handle and the Rust
//! middleware chain.

use std::sync::Arc;

use crate::dispatcher::{DispatchError, DispatchResult};
use crate::pipeline::{
    ActionMiddleware, AuditMiddleware, MiddlewareContext, RateLimitMiddleware, TimingMiddleware,
};

/// Newtype wrapper so `Arc<TimingMiddleware>` can be passed to `add_middleware`.
pub(crate) struct SharedTimingMiddleware(pub(crate) Arc<TimingMiddleware>);

impl ActionMiddleware for SharedTimingMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        self.0.before_dispatch(ctx)
    }

    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        self.0.after_dispatch(ctx, result);
    }

    fn name(&self) -> &'static str {
        "timing"
    }
}

/// Newtype wrapper so `Arc<AuditMiddleware>` can be passed to `add_middleware`.
pub(crate) struct SharedAuditMiddleware(pub(crate) Arc<AuditMiddleware>);

impl ActionMiddleware for SharedAuditMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        self.0.before_dispatch(ctx)
    }

    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        self.0.after_dispatch(ctx, result);
    }

    fn name(&self) -> &'static str {
        "audit"
    }
}

/// Newtype wrapper so `Arc<RateLimitMiddleware>` can be passed to `add_middleware`.
pub(crate) struct SharedRateLimitMiddleware(pub(crate) Arc<RateLimitMiddleware>);

impl ActionMiddleware for SharedRateLimitMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        self.0.before_dispatch(ctx)
    }

    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        self.0.after_dispatch(ctx, result);
    }

    fn name(&self) -> &'static str {
        "rate_limit"
    }
}
