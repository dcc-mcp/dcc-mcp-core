//! Source-compatibility facade for the tool-name codec.
//!
//! The implementation lives in [`dcc_mcp_gateway_core::naming::encode`]
//! (and `primitives` for `is_cursor_safe_alphabet`). This module
//! preserves the historical `crate::gateway::namespace::*` import
//! paths so adapter code does not have to change.

pub use dcc_mcp_gateway_core::naming::{
    assert_gateway_tool_name, decode_skill_tool_name, decode_tool_name, encode_tool_name,
    encode_tool_name_cursor_safe, escape_cursor_safe, extract_bare_tool_name,
    is_cursor_safe_alphabet, skill_tool_name, unescape_cursor_safe,
};
