//! Axum request handlers for the MCP Streamable HTTP transport.
//!
//! - `POST /mcp`   — client sends JSON-RPC messages; response is JSON or SSE
//! - `GET  /mcp`   — client opens a long-lived SSE stream for server-push events
//! - `DELETE /mcp` — client closes its session
//!
//! ## Maintainer layout
//!
//! `handler.rs` is a thin facade; the implementation lives in focused sibling files:
//!
//! - [`state`] — [`AppState`] struct + lifecycle helpers + timeout constants
//! - [`routes`] — the three axum entry points (`handle_post` / `handle_get` / `handle_delete`)
//! - [`notifications`] — notification + response-message routing
//! - [`dispatch`] — JSON-RPC method router + `initialize` + `tools/list`
//!
//! Request-specific handlers (e.g. `tools/call`, `resources/*`, `prompts/*`,
//! `elicitation/create`) live in [`crate::handlers`] and are pulled in via the
//! `pub(crate) use crate::handlers::*;` re-export below so existing call sites
//! can keep referencing them through `crate::handler::`.

#[path = "handler_state.rs"]
mod state;

#[path = "handler_routes.rs"]
mod routes;

#[path = "handler_notifications.rs"]
mod notifications;

#[path = "handler_dispatch.rs"]
mod dispatch;

pub use routes::{handle_delete, handle_get, handle_post};
pub use state::AppState;

// Re-exports below are a facade for in-crate call sites — even when the
// current source has no direct consumer, downstream modules and tests
// reach these symbols through `crate::handler::*`.
#[allow(unused_imports)]
pub(crate) use dispatch::{dispatch_request, handle_initialize, handle_tools_list};
#[allow(unused_imports)]
pub(crate) use notifications::handle_response_message;
pub(crate) use state::{CANCELLED_REQUEST_TTL, ELICITATION_TIMEOUT, ROOTS_REFRESH_TIMEOUT};

// Re-export every `handlers::*` item into `crate::handler` so existing
// call sites that use `crate::handler::handle_tools_call` etc. keep
// working unchanged.
#[allow(unused_imports)]
pub(crate) use crate::handlers::*;
