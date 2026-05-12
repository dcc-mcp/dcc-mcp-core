//! Tools-list aggregation and tools-call routing for the facade gateway.
//!
//! This module is the core of the "one endpoint, every DCC" façade:
//!
//! * `aggregate_tools_list` — return the **minimal** gateway MCP surface
//!   (discover + dispatch primitives). Backend per-action tools are
//!   intentionally NOT published here — agents discover them through
//!   `search_tools` / `describe_tool` / `call_tool` / `call_tools` (routed into the
//!   per-DCC REST `/v1/call`).
//! * `route_tools_call` — dispatch a `tools/call` to the matching local
//!   handler (meta-tools, skill management, or the dynamic dispatch
//!   wrappers). Any other name is rejected with a hint pointing at the
//!   dynamic wrappers.
//!
//! Prompts and resources still fan out through [`aggregate_prompts_list`]
//! / [`aggregate_resources_list`] because the MCP spec has no dynamic
//! wrapper for either primitive; the per-instance prefix comes from
//! [`super::namespace`].

mod call;
mod fingerprint;
mod helpers;
mod list;
mod prompts;
mod resources;
mod skill_mgmt;
#[cfg(test)]
mod tests;

pub use call::route_tools_call;
pub use fingerprint::compute_tools_fingerprint;
pub(crate) use fingerprint::compute_tools_fingerprint_with_own;
pub(crate) use helpers::{
    find_instance_by_prefix, inject_instance_metadata, live_backends, resolve_target,
    targets_for_fanout, to_text_result,
};
pub use list::aggregate_tools_list;
pub(crate) use prompts::compute_prompts_fingerprint_with_own;
pub use prompts::{
    PromptsGetError, aggregate_prompts_list, compute_prompts_fingerprint, route_prompts_get,
};
pub use resources::aggregate_resources_list;
pub(crate) use resources::compute_resources_fingerprint_with_own;
pub(crate) use skill_mgmt::{skill_management_tool_defs, skill_mgmt_dispatch};

use std::time::Duration;

use futures::future::join_all;
use serde_json::{Value, json};
use uuid::Uuid;

use super::backend_client::{call_backend, fetch_tools};
use super::namespace::instance_short;
use super::state::GatewayState;
use super::tools::{
    gateway_tool_defs, tool_acquire_instance, tool_call_tool, tool_call_tools, tool_describe_tool,
    tool_release_instance, tool_search_tools,
};
use dcc_mcp_jsonrpc::{TOOLS_LIST_PAGE_SIZE, decode_cursor, encode_cursor};
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};
