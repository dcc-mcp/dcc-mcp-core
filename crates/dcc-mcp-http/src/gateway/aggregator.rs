//! Tools-list aggregation and tools-call routing for the facade gateway.
//!
//! This module is the core of the "one endpoint, every DCC" façade:
//!
//! * `aggregate_tools_list` — fan out `tools/list` to every live backend and
//!   merge the results.  Backend-provided tools get an instance prefix so
//!   identical tool names across multiple DCCs never clash (see [`namespace`]).
//! * `route_tools_call` — dispatch a `tools/call` based on the tool name:
//!   - Meta / skill-management tools are handled locally or fanned-out with
//!     instance-scoped semantics.
//!   - Prefixed tools are forwarded to the backend that owns them.
//!
//! All network I/O goes through the stateless helpers in
//! [`super::backend_client`], so fan-out works concurrently via `join_all`.

mod call;
mod fingerprint;
mod helpers;
mod list;
mod skill_mgmt;
#[cfg(test)]
mod tests;
mod wait_terminal;

pub use call::route_tools_call;
pub use fingerprint::compute_tools_fingerprint;
pub(crate) use helpers::{
    envelope_to_text_result, extract_job_id, find_instance_by_prefix, inject_instance_metadata,
    live_backends, meta_signals_async_dispatch, meta_wants_wait_for_terminal, resolve_target,
    strip_gateway_meta_flags, targets_for_fanout, to_text_result,
};
pub use list::aggregate_tools_list;
pub(crate) use skill_mgmt::{skill_management_tool_defs, skill_mgmt_dispatch};
#[cfg(test)]
pub(crate) use wait_terminal::merge_job_update_into_envelope;
pub(crate) use wait_terminal::wait_for_terminal_reply;

use std::time::Duration;

use futures::future::join_all;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::backend_client::{call_backend, fetch_tools, forward_tools_call};
use super::namespace::{decode_tool_name, encode_tool_name, instance_short, is_local_tool};
use super::state::GatewayState;
use super::tools::{
    gateway_tool_defs, tool_connect_to_dcc, tool_get_instance, tool_list_instances,
};
use crate::protocol::{TOOLS_LIST_PAGE_SIZE, decode_cursor, encode_cursor};
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

/// Terminal job statuses that end a wait-for-terminal block (#321).
///
/// Mirrors the backend's [`crate::job::JobStatus`] terminal states; the
/// gateway does not import the enum directly to keep the dependency
/// graph flat.
const TERMINAL_JOB_STATUSES: &[&str] = &["completed", "failed", "cancelled", "interrupted"];
