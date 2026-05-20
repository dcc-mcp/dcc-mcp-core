//! HTTP client used by the gateway to talk to each backend DCC server.
//!
//! ## Architecture after #818 phase 2
//!
//! Backends are `McpHttpServer` instances listening on
//! `http://{host}:{port}`.  The gateway historically spoke MCP JSON-RPC
//! (`/mcp`) to them for every operation.  After #818 phase 2 every
//! per-backend call goes through the per-DCC REST surface (`/v1/*`)
//! instead:
//!
//! | Operation            | Was (MCP JSON-RPC)      | Now (REST)              |
//! |----------------------|-------------------------|-------------------------|
//! | list tools           | `tools/list`            | `POST /v1/search`       |
//! | call a tool          | `tools/call`            | `POST /v1/call`         |
//! | list prompts         | `prompts/list`          | `GET  /v1/prompts`      |
//! | render a prompt      | `prompts/get`           | `GET  /v1/prompts/{n}`  |
//! | list resources       | `resources/list`        | `GET  /v1/resources`    |
//! | read a resource      | `resources/read`        | `GET  /v1/resources/{u}`|
//! | liveness             | `GET /health`           | `GET /health` + legacy `/healthz` fallback |
//! | readiness            | `GET /v1/readyz`        | `GET /v1/readyz` (unchanged)|
//!
//! The gateway MCP client face (`/mcp`) is **unchanged** — this file
//! only affects how the gateway contacts *backends*.
//!
//! `subscribe_resource` (backed by the SSE subscriber pool) is retained
//! until #818 phase 3 when `sse_subscriber.rs` is retired.

pub(crate) mod error;
pub(crate) mod http;
pub(crate) mod ops;
pub(crate) mod probe;
pub(crate) mod urls;

#[cfg(test)]
pub(crate) use probe::probe_mcp_health;
#[allow(unused_imports)]
pub(crate) use probe::{
    ProbeOutcome, probe_mcp_readiness, probe_mcp_readiness_once, probe_readiness,
};

#[allow(unused_imports)]
pub(crate) use urls::{health_url_from_mcp_url, readyz_url_from_mcp_url, rest_base_from_mcp_url};

pub use ops::{
    call_backend, fetch_prompts, fetch_resources, fetch_tools, forward_prompts_get,
    forward_tools_call, read_resource, subscribe_resource, try_describe_tool, try_fetch_prompts,
    try_fetch_resources, try_fetch_tools,
};

#[cfg(test)]
mod tests;
