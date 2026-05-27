//! Pluggable BeforeCall/AfterCall middleware chain for the gateway.
//!
//! Operators register middlewares via [`MiddlewareChain`] to apply cross-cutting
//! policies (audit, quota, redaction, transformation) without forking core.
//!
//! # Example
//!
//! ```rust,ignore
//! use dcc_mcp_gateway::gateway::middleware::{
//!     MiddlewareChain, AuditMiddleware, QuotaMiddleware, RedactionMiddleware,
//! };
//! use std::sync::Arc;
//!
//! let chain = MiddlewareChain::new()
//!     .with_before(Arc::new(AuditMiddleware::default()))
//!     .with_before(Arc::new(QuotaMiddleware::new(100)))
//!     .with_before(Arc::new(RedactionMiddleware::new(vec!["api_key", "token"])));
//! ```

mod audit;
mod chain;
mod context;
mod error;
mod event;
mod governance;
mod quota;
mod redaction;
mod traits;

#[cfg(test)]
mod tests;

pub use audit::{AuditEntry, AuditMiddleware, AuditSink, DefaultAuditSink};
pub use chain::MiddlewareChain;
pub use context::{CallContext, CallResult};
pub use error::MiddlewareError;
pub use event::record_gateway_event;
pub use governance::{MiddlewareGovernanceControl, MiddlewareGovernanceSnapshot};
pub use quota::QuotaMiddleware;
pub use redaction::RedactionMiddleware;
pub use traits::{AfterCallMiddleware, BeforeCallMiddleware};
