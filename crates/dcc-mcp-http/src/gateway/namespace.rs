//! Tool-name namespace helpers for the aggregating gateway.
//!
//! ## Per-DCC server: proactive `<skill>.<name>` namespacing (#238)
//!
//! Non-core tools registered from a skill use `<skill-name>.<tool-name>` format
//! (e.g. `maya-animation.set_keyframe`) so the AI agent immediately sees which
//! skill a tool belongs to.
//!
//! ## Gateway: `<id8>.<tool>` instance prefix (#261)
//!
//! The aggregating gateway prepends an 8-hex-char instance id so duplicate
//! tool names across multiple DCC backends remain addressable. The chosen
//! separator is **`.` (dot)** because [SEP-986](
//! https://github.com/modelcontextprotocol/modelcontextprotocol/pull/1603)
//! restricts MCP tool names to `[A-Za-z0-9_.-]`, 1–128 chars — `/` is **not**
//! legal. Major LLM clients (Anthropic, OpenAI, Cursor) apply even stricter
//! regexes and will reject names containing `/` outright.
//!
//! Decoder accepts three historical encodings for one-version backward
//! compatibility (each with a `tracing::warn!` on the legacy forms):
//!
//! | Form | Status |
//! |------|--------|
//! | `{id8}.{tool}` | **Preferred** — current emitter |
//! | `{id8}/{tool}` | Deprecated — previous unreleased build, decoded + warned |
//! | `{id8}__{tool}` | Legacy — pre-#258, decoded + warned |

use uuid::Uuid;

pub const GATEWAY_LOCAL_TOOLS: &[&str] = &[
    "list_dcc_instances",
    "get_dcc_instance",
    "connect_to_dcc",
    "list_skills",
    "find_skills",
    "search_skills",
    "get_skill_info",
    "load_skill",
    "unload_skill",
];

/// Core per-DCC tools that keep bare names (no skill prefix).
pub const CORE_TOOL_NAMES: &[&str] = &[
    "find_skills",
    "list_skills",
    "get_skill_info",
    "load_skill",
    "unload_skill",
    "search_skills",
    "activate_tool_group",
    "deactivate_tool_group",
    "search_tools",
];

pub const ID_PREFIX_LEN: usize = 8;

/// Current, SEP-986-compliant gateway instance separator.
pub const INSTANCE_SEP: &str = ".";
/// Deprecated separator from an unreleased build — still decoded for
/// one-version backward compat, never emitted.
pub const DEPRECATED_SLASH_SEP: &str = "/";
/// Legacy pre-#258 separator — still decoded for backward compat.
pub const LEGACY_NAMESPACE_SEP: &str = "__";
/// Skill→tool separator (unchanged; already SEP-986-compliant).
pub const SKILL_TOOL_SEP: &str = ".";

pub fn is_local_tool(name: &str) -> bool {
    GATEWAY_LOCAL_TOOLS.contains(&name)
}
pub fn is_core_tool(name: &str) -> bool {
    CORE_TOOL_NAMES.contains(&name)
}

pub fn instance_short(id: &Uuid) -> String {
    let mut s = id.simple().to_string();
    s.truncate(ID_PREFIX_LEN);
    s
}

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

fn is_instance_prefix(s: &str) -> bool {
    s.len() == ID_PREFIX_LEN && s.chars().all(|c| c.is_ascii_hexdigit())
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

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_naming::validate_tool_name;

    #[test]
    fn instance_short_deterministic() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        assert_eq!(instance_short(&id), "abcdef01");
    }

    #[test]
    fn encode_uses_dot_separator() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let enc = encode_tool_name(&id, "create_sphere");
        assert_eq!(enc, "abcdef01.create_sphere");
    }

    #[test]
    fn encode_never_contains_slash() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        for tool in [
            "create_sphere",
            "maya-animation.set_keyframe",
            "CamelCase",
            "x",
        ] {
            let enc = encode_tool_name(&id, tool);
            assert!(
                !enc.contains('/'),
                "gateway encoded {tool:?} -> {enc:?} which contains `/`"
            );
        }
    }

    #[test]
    fn encode_produces_sep986_compliant_names() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        for tool in ["create_sphere", "maya-animation.set_keyframe", "CamelCase"] {
            let enc = encode_tool_name(&id, tool);
            assert!(
                validate_tool_name(&enc).is_ok(),
                "gateway emitted {enc:?} which fails SEP-986 validation"
            );
        }
    }

    #[test]
    fn encode_then_decode_roundtrips() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let enc = encode_tool_name(&id, "maya-animation.set_keyframe");
        assert_eq!(enc, "abcdef01.maya-animation.set_keyframe");
        let (p, o) = decode_tool_name(&enc).unwrap();
        assert_eq!(p, "abcdef01");
        assert_eq!(o, "maya-animation.set_keyframe");
    }

    #[test]
    fn decode_accepts_preferred_dot_form() {
        let (p, n) = decode_tool_name("abcdef01.create_sphere").unwrap();
        assert_eq!(p, "abcdef01");
        assert_eq!(n, "create_sphere");
    }

    #[test]
    fn decode_accepts_deprecated_slash_form() {
        let (p, n) = decode_tool_name("abcdef01/create_sphere").unwrap();
        assert_eq!(p, "abcdef01");
        assert_eq!(n, "create_sphere");
    }

    #[test]
    fn decode_accepts_legacy_double_underscore_form() {
        let (p, n) = decode_tool_name("abcdef01__maya_geometry__create_sphere").unwrap();
        assert_eq!(p, "abcdef01");
        assert_eq!(n, "maya_geometry__create_sphere");
    }

    #[test]
    fn decode_rejects_non_hex_prefix() {
        // 8 chars but not hex → must not be mistaken for an instance prefix.
        assert!(decode_tool_name("zzzzzzzz.create").is_none());
    }

    #[test]
    fn decode_rejects_wrong_length_prefix() {
        assert!(decode_tool_name("abcdef.create").is_none()); // 6 chars
        assert!(decode_tool_name("abcdef012.create").is_none()); // 9 chars
    }

    #[test]
    fn local_tools_decode_to_none() {
        for name in GATEWAY_LOCAL_TOOLS {
            assert!(decode_tool_name(name).is_none());
        }
    }

    #[test]
    fn extract_bare_name_strips_prefix() {
        assert_eq!(
            extract_bare_tool_name("maya-animation", "maya_animation__set_keyframe"),
            "set_keyframe"
        );
        assert_eq!(
            extract_bare_tool_name("", "get_scene_info"),
            "get_scene_info"
        );
    }

    #[test]
    fn skill_tool_name_formats_correctly() {
        assert_eq!(
            skill_tool_name("maya-animation", "maya_animation__set_keyframe"),
            Some("maya-animation.set_keyframe".to_string())
        );
        assert_eq!(skill_tool_name("", "set_keyframe"), None);
    }

    #[test]
    fn skill_tool_name_none_for_core_tools() {
        for core in CORE_TOOL_NAMES {
            assert_eq!(skill_tool_name("s", &format!("s__{core}")), None);
        }
    }

    #[test]
    fn decode_skill_tool_name_round_trips() {
        let (skill, tool) = decode_skill_tool_name("maya-animation.set_keyframe").unwrap();
        assert_eq!(skill, "maya-animation");
        assert_eq!(tool, "set_keyframe");
    }

    #[test]
    fn decode_skill_tool_name_rejects_stubs() {
        assert!(decode_skill_tool_name("__skill__maya").is_none());
        assert!(decode_skill_tool_name("abcdef01/tool.name").is_none());
    }

    #[test]
    fn decode_skill_tool_name_rejects_gateway_encoded_form() {
        // `abcdef01.create_sphere` is a gateway-encoded tool, not a skill.tool
        // pair. `decode_skill_tool_name` must yield None so callers route it
        // through `decode_tool_name` instead.
        assert!(decode_skill_tool_name("abcdef01.create_sphere").is_none());
    }

    #[test]
    fn assert_gateway_tool_name_accepts_compliant() {
        assert!(assert_gateway_tool_name("abcdef01.create_sphere").is_ok());
    }

    #[test]
    fn assert_gateway_tool_name_rejects_slash() {
        assert!(assert_gateway_tool_name("abcdef01/create_sphere").is_err());
    }
}
