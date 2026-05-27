//! Tool-name namespace facade for the aggregating gateway.
//!
//! The pure naming domain — every bare-name, skill-qualified, and
//! cursor-safe encoder / decoder — now lives in
//! [`dcc_mcp_gateway_core::naming`]. This module is a thin source-
//! compatibility shim that re-exports the historical
//! `crate::gateway::namespace::*` paths so adapter code, tests, and
//! external consumers keep compiling unchanged.
//!
//! ## Per-DCC server: proactive `<skill>__<name>` namespacing (#238)
//!
//! Non-core tools registered from a skill use `<skill-name>__<tool-name>` format
//! (e.g. `maya-animation__set_keyframe`) so the AI agent immediately sees which
//! skill a tool belongs to.
//!
//! ## Per-DCC server: bare-name mode (#307)
//!
//! When enabled via [`crate::McpHttpConfig::bare_tool_names`] (default `true`),
//! the server publishes tools under their **bare action name** whenever no
//! other skill on the same instance registers the same bare name. Collisions
//! fall back to `<skill>__<action>` and log a one-shot warning. This cuts the
//! `tools/list` token footprint by ~40% on Maya-sized skill sets without
//! breaking routing.
//!
//! ## Gateway: `i_<id8>__<escaped>` Cursor-safe encoding (#656)
//!
//! The aggregating gateway emits an 8-hex-char instance id so duplicate
//! tool names across multiple DCC backends remain addressable. The
//! cursor-safe `i_<id8>__<escaped_tool>` form stays inside the
//! `[A-Za-z0-9_]` alphabet by escaping `.` / `-` / `_` with the
//! reversible `_D_` / `_H_` / `_U_` triples.
//!
//! | Form | Status |
//! |------|--------|
//! | `i_{id8}__{escaped_tool}` | **Wire form** for MCP `tools/call` and `prompts/get` routing |
//! | `{id8}.{tool}` | Used in capability / REST slugs only — not decoded by [`decode_tool_name`] |

mod bare;
mod constants;
mod encode;
mod resource_uri;

pub use bare::{BareNameInput, resolve_bare_names, warn_skill_qualified_once};
pub use constants::{
    CORE_TOOL_NAMES, CURSOR_SAFE_PREFIX, CURSOR_SAFE_SEP, GATEWAY_LOCAL_TOOLS, ID_PREFIX_LEN,
    INSTANCE_SEP, SKILL_TOOL_SEP, instance_short, is_core_tool, is_local_tool,
};
pub use encode::{
    assert_gateway_tool_name, decode_skill_tool_name, decode_tool_name, encode_tool_name,
    encode_tool_name_cursor_safe, escape_cursor_safe, extract_bare_tool_name,
    is_cursor_safe_alphabet, skill_tool_name, unescape_cursor_safe,
};
pub use resource_uri::{decode_resource_uri, encode_resource_uri};
