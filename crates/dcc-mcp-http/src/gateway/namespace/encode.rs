//! Tool-name encoder / decoder helpers.
//!
//! Covers the three naming surfaces of the gateway namespace:
//!
//! * **Skill → tool** (`<skill>.<tool>`): [`extract_bare_tool_name`],
//!   [`skill_tool_name`], [`decode_skill_tool_name`].
//! * **Gateway instance prefix** (`{id8}.{tool}`): [`encode_tool_name`],
//!   [`decode_tool_name`], [`assert_gateway_tool_name`].
//!
//! Backward compatibility: [`decode_tool_name`] still accepts two
//! deprecated separator forms (`/` and `__`) and emits a
//! `tracing::warn!` for each.

use uuid::Uuid;

use super::constants::{
    DEPRECATED_SLASH_SEP, INSTANCE_SEP, LEGACY_NAMESPACE_SEP, SKILL_TOOL_SEP, instance_short,
    is_core_tool, is_instance_prefix, is_local_tool,
};

/// Extract the bare tool name from an internal action name.
///
/// # Examples
/// ```
/// # use dcc_mcp_http::gateway::namespace::extract_bare_tool_name;
/// assert_eq!(extract_bare_tool_name("maya-animation", "maya_animation__set_keyframe"),
///            "set_keyframe");
/// assert_eq!(extract_bare_tool_name("", "get_scene_info"), "get_scene_info");
/// ```
pub fn extract_bare_tool_name<'a>(skill_name: &str, action_name: &'a str) -> &'a str {
    if skill_name.is_empty() {
        return action_name;
    }
    let prefix = format!("{}__", skill_name.replace('-', "_"));
    action_name
        .strip_prefix(prefix.as_str())
        .unwrap_or(action_name)
}

/// Build the proactive `<skill-name>.<tool-name>` MCP name.
///
/// # Examples
/// ```
/// # use dcc_mcp_http::gateway::namespace::skill_tool_name;
/// assert_eq!(skill_tool_name("maya-animation", "maya_animation__set_keyframe"),
///            Some("maya-animation.set_keyframe".to_string()));
/// assert_eq!(skill_tool_name("", "set_keyframe"), None);
/// ```
pub fn skill_tool_name(skill_name: &str, action_name: &str) -> Option<String> {
    if skill_name.is_empty() {
        return None;
    }
    let bare = extract_bare_tool_name(skill_name, action_name);
    if is_core_tool(bare) || bare.contains(SKILL_TOOL_SEP) {
        return None;
    }
    Some(format!("{skill_name}{SKILL_TOOL_SEP}{bare}"))
}

/// Decode a `<skill>.<tool>` pair from a per-DCC tool name.
///
/// Rejects gateway-encoded names (`{id8}.<rest>` with an 8-hex prefix) and
/// skill stubs (`__skill__...`).
pub fn decode_skill_tool_name(namespaced: &str) -> Option<(&str, &str)> {
    if namespaced.starts_with("__") || namespaced.contains('/') {
        return None;
    }
    // Reject gateway-encoded form — the gateway prefix owns the first dot.
    if let Some((head, _)) = namespaced.split_once(SKILL_TOOL_SEP) {
        if is_instance_prefix(head) {
            return None;
        }
    }
    namespaced.split_once(SKILL_TOOL_SEP)
}

/// Encode a tool name for gateway aggregation: `{id8}.{original}`.
///
/// # Panics (debug builds only)
///
/// In debug builds the result is checked against
/// [`dcc_mcp_naming::validate_tool_name`]; the gateway never emits a name that
/// fails SEP-986. Release builds skip the check for zero overhead — invalid
/// names would have been caught at registration time (see
/// [`assert_gateway_tool_name`]).
pub fn encode_tool_name(id: &Uuid, original: &str) -> String {
    let encoded = format!("{}{INSTANCE_SEP}{original}", instance_short(id));
    debug_assert!(
        dcc_mcp_naming::validate_tool_name(&encoded).is_ok(),
        "gateway emitted tool name {encoded:?} that violates SEP-986"
    );
    encoded
}

/// Validate a tool name the gateway is about to publish.
///
/// Used by the registration path as a hard gate: if the composed name would
/// be rejected by a compliant MCP client, we refuse to register it rather
/// than ship it and watch the LLM client 400 at runtime.
///
/// # Errors
///
/// Propagates [`dcc_mcp_naming::NamingError`] unchanged.
pub fn assert_gateway_tool_name(name: &str) -> Result<(), dcc_mcp_naming::NamingError> {
    dcc_mcp_naming::validate_tool_name(name)
}

/// Decode a gateway-encoded tool name into `(id8, original)`.
///
/// Accepts the current `.` separator plus two deprecated encodings for
/// backward compat (`/` and `__`); both emit a `tracing::warn!` so operators
/// notice leftover clients.
pub fn decode_tool_name(prefixed: &str) -> Option<(&str, &str)> {
    if is_local_tool(prefixed) {
        return None;
    }
    // 1. Preferred: `{id8}.{tool}`.
    if let Some((p, r)) = prefixed.split_once(INSTANCE_SEP) {
        if is_instance_prefix(p) {
            return Some((p, r));
        }
    }
    // 2. Deprecated: `{id8}/{tool}` — the unreleased format fixed in #261.
    if let Some((p, r)) = prefixed.split_once(DEPRECATED_SLASH_SEP) {
        if is_instance_prefix(p) {
            tracing::warn!(
                tool = prefixed,
                "Deprecated `/` gateway separator (pre-#261). Use `{{id8}}.{{tool}}`."
            );
            return Some((p, r));
        }
    }
    // 3. Legacy: `{id8}__{tool}` — pre-#258.
    if let Some((p, r)) = prefixed.split_once(LEGACY_NAMESPACE_SEP) {
        if is_instance_prefix(p) {
            tracing::warn!(
                tool = prefixed,
                "Deprecated `__` gateway separator (pre-#258). Use `{{id8}}.{{tool}}`."
            );
            return Some((p, r));
        }
    }
    None
}
