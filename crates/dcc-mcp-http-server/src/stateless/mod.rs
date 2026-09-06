//! Stateless MCP service path for protocol version 2026-07-28 (ADR-010 Phase 1).
//!
//! This module is compiled only when the `mcp-2026-07-28` feature flag is
//! enabled.  It provides a request-handling layer that:
//!
//! - Does **not** create sessions or read `Mcp-Session-Id`.
//! - Handles `server/discover` in place of `initialize`.
//! - Routes tool calls to the existing [`crate::rmcp_tool_call_dispatch`]
//!   pipeline with `session_id = None` (Tool Registry / Dispatcher / Catalog
//!   are not modified).
//! - Reads `_meta` on every request for per-request client context.
//!
//! ## Usage
//!
//! The caller (typically an axum handler or integration test) detects the
//! `MCP-Protocol-Version: 2026-07-28` request header, constructs a
//! [`StatelessMcpService`] with the shared [`ServerState`] and
//! [`RegistryContext`], and calls [`StatelessMcpService::handle_request`].
//!
//! ```rust,ignore
//! # use std::sync::Arc;
//! # use dcc_mcp_http_server::stateless::StatelessMcpService;
//! # use dcc_mcp_http_server::server_state::ServerState;
//! # use dcc_mcp_http_server::rmcp_registry_context::RegistryContext;
//! let service = StatelessMcpService::new(state, registry_context);
//! let response = service.handle_request(&jsonrpc_req).await;
//! ```

mod meta;
mod service;

pub use service::StatelessMcpService;
