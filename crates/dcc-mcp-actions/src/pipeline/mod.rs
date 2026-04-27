//! Action middleware pipeline — composable pre/post dispatch hooks.
//!
//! Provides a lightweight middleware chain that wraps [`ActionDispatcher`],
//! allowing cross-cutting concerns (logging, timing, rate-limiting, auditing)
//! to be applied uniformly without modifying individual action handlers.
//!
//! # Architecture
//!
//! ```text
//! ActionPipeline
//!   │
//!   ├── middleware[0]: LoggingMiddleware   (before: log params, after: log result)
//!   ├── middleware[1]: TimingMiddleware    (before: record start, after: record elapsed)
//!   ├── middleware[2]: RateLimitMiddleware (before: check rate, after: noop)
//!   └── middleware[N]: AuditMiddleware    (after: write audit record)
//!   │
//!   └── ActionDispatcher (actual handler invocation)
//! ```
//!
//! Middleware runs in registration order for `before_dispatch`, and in
//! **reverse** order for `after_dispatch` (standard onion model).
//!
//! # Example
//!
//! ```rust
//! use dcc_mcp_actions::pipeline::{ActionPipeline, LoggingMiddleware, TimingMiddleware};
//! use dcc_mcp_actions::dispatcher::ActionDispatcher;
//! use dcc_mcp_actions::registry::{ActionMeta, ActionRegistry};
//! use serde_json::json;
//!
//! let registry = ActionRegistry::new();
//! registry.register_action(ActionMeta {
//!     name: "ping".into(),
//!     dcc: "mock".into(),
//!     ..Default::default()
//! });
//!
//! let dispatcher = ActionDispatcher::new(registry);
//! dispatcher.register_handler("ping", |_| Ok(json!("pong")));
//!
//! let mut pipeline = ActionPipeline::new(dispatcher);
//! pipeline.add_middleware(LoggingMiddleware::new());
//! pipeline.add_middleware(TimingMiddleware::new());
//!
//! let result = pipeline.dispatch("ping", json!({})).unwrap();
//! assert_eq!(result.output, json!("pong"));
//! ```

mod audit;
mod logging;
#[cfg(feature = "python-bindings")]
pub mod python;
mod rate_limit;
mod timing;

#[cfg(test)]
mod tests;

pub use audit::{AuditMiddleware, AuditRecord};
pub use logging::LoggingMiddleware;
pub use rate_limit::RateLimitMiddleware;
pub use timing::TimingMiddleware;

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::dispatcher::{ActionDispatcher, DispatchError, DispatchResult};

// ── MiddlewareContext ─────────────────────────────────────────────────────────

/// Context passed through the middleware chain for a single dispatch call.
///
/// Middleware can read and write arbitrary key-value data in `extensions`
/// to communicate state across `before_dispatch` and `after_dispatch` calls.
#[derive(Debug, Clone)]
pub struct MiddlewareContext {
    /// The action name being dispatched.
    pub action: String,
    /// The input parameters (may be mutated by middleware).
    pub params: Value,
    /// Arbitrary middleware-specific state (e.g. start time, request ID).
    pub extensions: HashMap<String, Value>,
}

impl MiddlewareContext {
    /// Create a new context for an action dispatch.
    #[must_use]
    pub fn new(action: impl Into<String>, params: Value) -> Self {
        Self {
            action: action.into(),
            params,
            extensions: HashMap::new(),
        }
    }

    /// Insert a value into the extensions map.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        self.extensions.insert(key.into(), value);
    }

    /// Get a value from the extensions map.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.extensions.get(key)
    }
}

// ── ActionMiddleware ──────────────────────────────────────────────────────────

/// Trait for composable middleware in the action dispatch pipeline.
///
/// Implement this trait to inject logic before and/or after action dispatch.
/// Middleware is **synchronous** (matching the DCC main-thread constraint).
pub trait ActionMiddleware: Send + Sync {
    /// Called before the action handler is invoked.
    ///
    /// Return `Ok(())` to continue the pipeline, or `Err(DispatchError)` to
    /// abort dispatch (the handler and subsequent middleware will not run).
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        let _ = ctx;
        Ok(())
    }

    /// Called after the action handler has run (whether it succeeded or failed).
    ///
    /// `result` is `Ok(&DispatchResult)` on success, `Err(&DispatchError)` on failure.
    /// Middleware should generally not change a success to a failure here.
    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        let _ = (ctx, result);
    }

    /// Human-readable name for this middleware (used in logging/debugging).
    fn name(&self) -> &'static str {
        "unnamed_middleware"
    }
}

// ── ActionPipeline ────────────────────────────────────────────────────────────

/// A middleware-wrapped [`ActionDispatcher`].
///
/// Middleware runs in registration order for `before_dispatch`, and in
/// reverse order for `after_dispatch`.
pub struct ActionPipeline {
    dispatcher: ActionDispatcher,
    middlewares: Vec<Arc<dyn ActionMiddleware>>,
}

impl ActionPipeline {
    /// Create a new pipeline wrapping the given dispatcher (no middleware by default).
    #[must_use]
    pub fn new(dispatcher: ActionDispatcher) -> Self {
        Self {
            dispatcher,
            middlewares: Vec::new(),
        }
    }

    /// Add a middleware to the end of the chain.
    pub fn add_middleware<M: ActionMiddleware + 'static>(&mut self, middleware: M) {
        self.middlewares.push(Arc::new(middleware));
    }

    /// Return the number of registered middleware.
    #[must_use]
    pub fn middleware_count(&self) -> usize {
        self.middlewares.len()
    }

    /// Return the names of all registered middleware (in order).
    #[must_use]
    pub fn middleware_names(&self) -> Vec<&'static str> {
        self.middlewares.iter().map(|m| m.name()).collect()
    }

    /// Dispatch an action through the middleware pipeline.
    ///
    /// 1. Build a [`MiddlewareContext`] from `action_name` and `params`.
    /// 2. Run each middleware's `before_dispatch` in order; abort on first error.
    /// 3. Invoke the underlying [`ActionDispatcher`].
    /// 4. Run each middleware's `after_dispatch` in **reverse** order.
    /// 5. Return the dispatch result.
    pub fn dispatch(
        &self,
        action_name: &str,
        params: Value,
    ) -> Result<DispatchResult, DispatchError> {
        let mut ctx = MiddlewareContext::new(action_name, params.clone());

        // Run before_dispatch in registration order
        for middleware in &self.middlewares {
            middleware.before_dispatch(&mut ctx)?;
        }

        // Use (possibly mutated) params from context
        let dispatch_params = ctx.params.clone();
        let result = self.dispatcher.dispatch(action_name, dispatch_params);

        // Run after_dispatch in reverse order
        for middleware in self.middlewares.iter().rev() {
            match &result {
                Ok(ok) => middleware.after_dispatch(&ctx, Ok(ok)),
                Err(err) => middleware.after_dispatch(&ctx, Err(err)),
            }
        }

        result
    }

    /// Access the underlying dispatcher.
    #[must_use]
    pub fn dispatcher(&self) -> &ActionDispatcher {
        &self.dispatcher
    }
}
