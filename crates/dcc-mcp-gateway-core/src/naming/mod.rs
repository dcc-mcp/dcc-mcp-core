//! Pure naming domain for the DCC MCP gateway.
//!
//! Layered inside-out:
//!
//! | Layer | Module | Responsibility |
//! |-------|--------|----------------|
//! | bedrock      | [`primitives`]    | UUID truncation + cursor-safe alphabet predicate |
//! | vocabulary   | [`constants`]     | Canonical tool name lists, separators, classifiers |
//! | codec        | [`encode`]        | Skill-tool and gateway-instance name encode/decode |
//! | domain logic | [`bare`]          | Pure bare-name resolver (no side effects) |
//! | observability| [`observability`] | One-shot warn state for collision diagnostics |
//!
//! The dependency direction is strictly inward:
//!
//! ```text
//! observability  bare ─────┐
//!                          │
//!         encode ──────────┤
//!                          ▼
//!    constants ─────► primitives
//! ```
//!
//! All third-party deps are pure: `uuid`, `tracing` (facade only),
//! `dcc-mcp-naming` (validator only). No HTTP, no async, no `GatewayState`.
//!
//! Consumers should import directly from `dcc_mcp_gateway_core::naming::*`.
//! The `dcc-mcp-gateway` crate keeps a thin `gateway::namespace` facade so
//! historical import paths in adapter code continue to work; new code should
//! prefer this crate.

mod bare;
mod constants;
mod encode;
mod observability;
mod primitives;

pub use bare::{BareNameInput, resolve_bare_names};
pub use constants::{
    CORE_TOOL_NAMES, CURSOR_SAFE_PREFIX, CURSOR_SAFE_SEP, GATEWAY_LOCAL_TOOLS, INSTANCE_SEP,
    SKILL_TOOL_SEP, is_core_tool, is_local_tool,
};
pub use encode::{
    assert_gateway_tool_name, decode_skill_tool_name, decode_tool_name, encode_tool_name,
    encode_tool_name_cursor_safe, escape_cursor_safe, extract_bare_tool_name, skill_tool_name,
    unescape_cursor_safe,
};
pub use observability::warn_skill_qualified_once;
pub use primitives::{ID_PREFIX_LEN, instance_short, is_cursor_safe_alphabet};

#[cfg(test)]
pub use observability::__reset_warn_state_for_tests;

#[cfg(test)]
mod tests;
