//! Source-compatibility facade for the naming vocabulary and primitives.
//!
//! The implementation lives in [`dcc_mcp_gateway_core::naming`]. This
//! module re-exports the historical
//! `crate::gateway::namespace::{CORE_TOOL_NAMES, CURSOR_SAFE_PREFIX,
//! CURSOR_SAFE_SEP, GATEWAY_LOCAL_TOOLS, ID_PREFIX_LEN, INSTANCE_SEP,
//! SKILL_TOOL_SEP, instance_short, is_core_tool, is_local_tool}` paths
//! so adapter code does not have to change.

pub use dcc_mcp_gateway_core::naming::{
    CORE_TOOL_NAMES, CURSOR_SAFE_PREFIX, CURSOR_SAFE_SEP, GATEWAY_LOCAL_TOOLS, ID_PREFIX_LEN,
    INSTANCE_SEP, SKILL_TOOL_SEP, instance_short, is_core_tool, is_local_tool,
};
