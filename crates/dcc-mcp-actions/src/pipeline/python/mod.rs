//! PyO3 bindings for the ToolPipeline and its built-in middleware.
//!
//! Exposed Python classes:
//!
//! - [`PyLoggingMiddleware`]    — emits tracing log lines before/after each action
//! - [`PyTimingMiddleware`]     — measures per-action latency (queryable from Python)
//! - [`PyAuditMiddleware`]      — accumulates an in-memory audit log (queryable from Python)
//! - [`PyRateLimitMiddleware`]  — fixed-window rate limiter per action name
//! - [`PyActionPipeline`]       — middleware-wrapped ActionDispatcher
//!
//! ## Design
//!
//! `PyAuditMiddleware` and `PyRateLimitMiddleware` expose mutable state to Python;
//! they are stored behind `Arc` so the pipeline and the Python handle share the
//! same instance.
//!
//! `PyActionPipeline` reuses the `PyActionDispatcher` from `crate::python` for
//! handler registration, then delegates through the Rust `ActionPipeline` for
//! middleware processing.
//!
//! ## Maintainer layout
//!
//! `python.rs` is a thin facade; implementation lives in focused siblings:
//!
//! - [`helpers`] — [`PyCallableHook`] struct + `value_to_py` recursive converter
//! - [`middleware`] — [`PyLoggingMiddleware`], [`PyTimingMiddleware`],
//!   [`PyAuditMiddleware`], [`PyRateLimitMiddleware`] (inner fields are
//!   `pub(super)` so the pipeline module can construct them)
//! - [`shared`] — `SharedTimingMiddleware` / `SharedAuditMiddleware` /
//!   `SharedRateLimitMiddleware` `Arc` newtypes that implement the
//!   `ActionMiddleware` trait
//! - [`pipeline`] — [`PyActionPipeline`] (the Python-facing `ToolPipeline`)

use pyo3::prelude::*;

mod helpers;
mod middleware;
mod pipeline;
mod shared;

#[cfg(test)]
mod tests;

pub use middleware::{
    PyAuditMiddleware, PyLoggingMiddleware, PyRateLimitMiddleware, PyTimingMiddleware,
};
pub use pipeline::PyActionPipeline;

// Re-export the Shared* newtypes so the `python_tests.rs` module in this
// crate can continue to reference `super::python::Shared*` unchanged.
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use shared::{SharedAuditMiddleware, SharedRateLimitMiddleware, SharedTimingMiddleware};

/// Register all pipeline Python classes on the given module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLoggingMiddleware>()?;
    m.add_class::<PyTimingMiddleware>()?;
    m.add_class::<PyAuditMiddleware>()?;
    m.add_class::<PyRateLimitMiddleware>()?;
    m.add_class::<PyActionPipeline>()?;
    Ok(())
}
