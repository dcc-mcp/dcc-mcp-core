//! [`MiddlewareChain`] — ordered pipeline of before/after middlewares.

use std::sync::Arc;

use super::context::{CallContext, CallResult};
use super::error::MiddlewareError;
use super::governance::{MiddlewareGovernanceControl, MiddlewareGovernanceSnapshot};
use super::traits::{AfterCallMiddleware, BeforeCallMiddleware};

/// An ordered pipeline of [`BeforeCallMiddleware`] and [`AfterCallMiddleware`].
///
/// Middlewares are called in registration order. If any `before` middleware
/// returns an error the pipeline is aborted and no further middlewares run.
///
/// Cloning is cheap — inner `Arc`s are just reference-counted.
#[derive(Clone, Default)]
pub struct MiddlewareChain {
    before: Vec<Arc<dyn BeforeCallMiddleware>>,
    after: Vec<Arc<dyn AfterCallMiddleware>>,
}

impl MiddlewareChain {
    /// Create an empty chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a `BeforeCallMiddleware` at the end of the before-chain.
    pub fn with_before(mut self, m: Arc<dyn BeforeCallMiddleware>) -> Self {
        self.before.push(m);
        self
    }

    /// Append an `AfterCallMiddleware` at the end of the after-chain.
    pub fn with_after(mut self, m: Arc<dyn AfterCallMiddleware>) -> Self {
        self.after.push(m);
        self
    }

    /// Register a `BeforeCallMiddleware` in-place.
    pub fn add_before(&mut self, m: Arc<dyn BeforeCallMiddleware>) {
        self.before.push(m);
    }

    /// Register an `AfterCallMiddleware` in-place.
    pub fn add_after(&mut self, m: Arc<dyn AfterCallMiddleware>) {
        self.after.push(m);
    }

    /// Insert a `BeforeCallMiddleware` at the **front** of the before-chain
    /// so it runs before any previously-registered middlewares.
    pub fn prepend_before(&mut self, m: Arc<dyn BeforeCallMiddleware>) {
        self.before.insert(0, m);
    }

    /// Insert an `AfterCallMiddleware` at the **front** of the after-chain
    /// so it runs before any previously-registered after middlewares.
    pub fn prepend_after(&mut self, m: Arc<dyn AfterCallMiddleware>) {
        self.after.insert(0, m);
    }

    /// Run every `before` middleware in order.
    ///
    /// Stops and returns the first error encountered.
    pub async fn run_before(&self, ctx: &mut CallContext) -> Result<(), MiddlewareError> {
        for m in &self.before {
            m.before_call(ctx).await?;
        }
        Ok(())
    }

    /// Run every `after` middleware in order.
    ///
    /// Stops and returns the first error encountered.
    pub async fn run_after(
        &self,
        ctx: &CallContext,
        result: &mut CallResult,
    ) -> Result<(), MiddlewareError> {
        for m in &self.after {
            m.after_call(ctx, result).await?;
        }
        Ok(())
    }

    /// Returns `true` when no middlewares are registered (fast-path skip).
    pub fn is_empty(&self) -> bool {
        self.before.is_empty() && self.after.is_empty()
    }

    /// Return bounded governance descriptors for operator UIs.
    #[must_use]
    pub fn governance_snapshot(&self) -> MiddlewareGovernanceSnapshot {
        let mut controls: Vec<MiddlewareGovernanceControl> = self
            .before
            .iter()
            .filter_map(|middleware| middleware.governance())
            .collect();
        controls.extend(
            self.after
                .iter()
                .filter_map(|middleware| middleware.governance()),
        );
        MiddlewareGovernanceSnapshot {
            before_count: self.before.len(),
            after_count: self.after.len(),
            controls,
        }
    }
}

impl std::fmt::Debug for MiddlewareChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiddlewareChain")
            .field("before_count", &self.before.len())
            .field("after_count", &self.after.len())
            .finish()
    }
}
