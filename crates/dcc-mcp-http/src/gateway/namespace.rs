//! Tool-name namespace helpers for the aggregating gateway.
//!
//! ## Per-DCC server: proactive `<skill>.<name>` namespacing (#238)
//!
//! Non-core tools registered from a skill use `<skill-name>.<tool-name>` format
//! (e.g. `maya-animation.set_keyframe`) so the AI agent immediately sees which
//! skill a tool belongs to.

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
pub const INSTANCE_SEP: &str = "/";
pub const LEGACY_NAMESPACE_SEP: &str = "__";
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

pub fn decode_skill_tool_name(namespaced: &str) -> Option<(&str, &str)> {
    if namespaced.starts_with("__") || namespaced.contains('/') {
        return None;
    }
    namespaced.split_once(SKILL_TOOL_SEP)
}

pub fn encode_tool_name(id: &Uuid, original: &str) -> String {
    format!("{}{INSTANCE_SEP}{original}", instance_short(id))
}

pub fn decode_tool_name(prefixed: &str) -> Option<(&str, &str)> {
    if is_local_tool(prefixed) {
        return None;
    }
    if let Some((p, r)) = prefixed.split_once(INSTANCE_SEP) {
        if p.len() == ID_PREFIX_LEN && p.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some((p, r));
        }
    }
    if let Some((p, r)) = prefixed.split_once(LEGACY_NAMESPACE_SEP) {
        if p.len() == ID_PREFIX_LEN && p.chars().all(|c| c.is_ascii_hexdigit()) {
            tracing::warn!(
                tool = prefixed,
                "Deprecated `__` prefix. Use `{{id8}}/{{tool}}`."
            );
            return Some((p, r));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn instance_short_deterministic() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        assert_eq!(instance_short(&id), "abcdef01");
    }
    #[test]
    fn encode_then_decode_roundtrips() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let enc = encode_tool_name(&id, "maya-animation.set_keyframe");
        assert_eq!(enc, "abcdef01/maya-animation.set_keyframe");
        let (p, o) = decode_tool_name(&enc).unwrap();
        assert_eq!(p, "abcdef01");
        assert_eq!(o, "maya-animation.set_keyframe");
    }
    #[test]
    fn decode_legacy_format() {
        let (p, n) = decode_tool_name("abcdef01__maya_geometry__create_sphere").unwrap();
        assert_eq!(p, "abcdef01");
        assert_eq!(n, "maya_geometry__create_sphere");
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
}
