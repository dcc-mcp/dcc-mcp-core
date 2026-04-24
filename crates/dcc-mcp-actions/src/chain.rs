//! ActionChain — native multi-step operation orchestration.
//!
//! Provides a fluent builder API for composing sequences of action dispatches
//! where each step can receive the accumulated context from previous steps.
//!
//! # Design
//!
//! - Each step specifies an action name and either static params or a
//!   context-driven closure that computes params at runtime.
//! - `{key}` placeholders in static params are interpolated from the
//!   accumulated context (consistent with the `workflow` skill).
//! - An optional `on_error` handler can inspect failures and decide whether
//!   to abort or continue the chain.
//! - All dispatch is synchronous (matching the DCC main-thread constraint).
//!
//! # Example
//!
//! ```rust
//! use dcc_mcp_actions::chain::ActionChain;
//! use dcc_mcp_actions::dispatcher::{ActionDispatcher, DispatchError};
//! use dcc_mcp_actions::registry::{ActionMeta, ActionRegistry};
//! use serde_json::{json, Value};
//!
//! let registry = ActionRegistry::new();
//! registry.register_action(ActionMeta { name: "step_a".into(), dcc: "mock".into(), ..Default::default() });
//! registry.register_action(ActionMeta { name: "step_b".into(), dcc: "mock".into(), ..Default::default() });
//!
//! let dispatcher = ActionDispatcher::new(registry);
//! dispatcher.register_handler("step_a", |_| Ok(json!({"result_a": 42})));
//! dispatcher.register_handler("step_b", |params| Ok(json!({"got": params["result_a"]})));
//!
//! let result = ActionChain::new()
//!     .step("step_a", json!({}))
//!     .step("step_b", json!({"result_a": "{result_a}"}))
//!     .run(&dispatcher, json!({}))
//!     .unwrap();
//!
//! assert!(result.success);
//! assert_eq!(result.steps.len(), 2);
//! ```
//!
//! # Maintainer layout
//!
//! This module is a **thin facade**. The implementation is split into focused
//! siblings so each file stays small and testable:
//!
//! - [`chain_types`](super::chain_types) — `ChainStepResult`, `ChainResult`,
//!   `ErrorAction`, and the crate-internal `ChainStep` / `StepParams` /
//!   `ErrorHandlerFn` types.
//! - [`chain_interpolate`](super::chain_interpolate) — `{key}` placeholder
//!   interpolation, context merge, and the public `context()` builder helper.
//! - [`chain_exec`](super::chain_exec) — the `ActionChain` fluent builder and
//!   its `run()` executor.
//! - [`chain_tests`](super::chain_tests) — unit tests for the three modules
//!   above (gated on `#[cfg(test)]`).

#[path = "chain_types.rs"]
mod types;

#[path = "chain_interpolate.rs"]
mod interpolate_impl;

#[path = "chain_exec.rs"]
mod exec;

#[cfg(test)]
#[path = "chain_tests.rs"]
mod tests;

pub use exec::ActionChain;
pub use interpolate_impl::context;
pub use types::{ChainResult, ChainStepResult, ErrorAction};
