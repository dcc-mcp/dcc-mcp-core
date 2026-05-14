//! MCP transport adapter layer.
//!
//! The rmcp SDK's `StreamableHttpService` handles HTTP transport (POST/GET/DELETE
//! on `/mcp`). This module bridges our `ServerState` + `ResourceRegistry` +
//! `PromptRegistry` to rmcp's `ServerHandler` interface via provider traits.
//!
//! ## Maintainer layout
//!
//! - [`state`] — [`AppState`] struct + lifecycle helpers
//! - [`rmcp_mount`] — wires `StreamableHttpService` into the axum router
//! - [`rmcp_providers_impl`] — concrete `ResourceProvider` / `PromptProvider` implementations

mod state;

pub mod rmcp_mount;
pub(crate) mod rmcp_providers_impl;

pub use state::AppState;
