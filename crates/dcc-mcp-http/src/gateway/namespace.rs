//! Tool-name namespace helpers for the aggregating gateway.
//!
//! The gateway aggregates `tools/list` from every live backend and exposes
//! the union through its own MCP endpoint.  Backends can share tool names
//! (two Maya instances both loading `maya-geometry` end up advertising a tool
//! called `maya_geometry__create_sphere`), so the gateway prefixes every
//! *backend-provided* tool name with a short instance token:
//!
//! ```text
//! {id8}__{original_tool_name}
//! └─┬─┘
//!   └── first 8 hex characters of the backend's instance UUID
//! ```
//!
//! The `__` separator is the same double-underscore convention skills already
//! use, so prefixed names remain valid JSON-Schema tool identifiers and agents
//! can still visually split them into "instance / skill / tool" chunks.
//!
//! Reserved names that the gateway handles **locally** and therefore never
//! prefixes: every entry in [`GATEWAY_LOCAL_TOOLS`].  Callers should consult
//! [`is_local_tool`] before routing to a backend.

use uuid::Uuid;

/// Tool names that the gateway handles itself without forwarding to a backend.
///
/// The first three are gateway discovery meta-tools; the remaining six are the
/// core skill-management tools proxied / fanned-out by the gateway with
/// aggregated semantics.
pub const GATEWAY_LOCAL_TOOLS: &[&str] = &[
    // Gateway discovery meta-tools
    "list_dcc_instances",
    "get_dcc_instance",
    "connect_to_dcc",
    // Skill-management tools (fanned out or routed to a target instance)
    "list_skills",
    "find_skills",
    "search_skills",
    "get_skill_info",
    "load_skill",
    "unload_skill",
];

/// Length of the instance-id prefix the gateway encodes into tool names.
///
/// Eight hex characters (~32 bits of randomness) is enough to disambiguate a
/// handful of concurrent DCC instances without bloating every tool name.
pub const ID_PREFIX_LEN: usize = 8;

/// Separator between the instance prefix and the original tool name.
pub const NAMESPACE_SEP: &str = "__";

/// Return `true` when the tool is handled directly by the gateway (no routing).
pub fn is_local_tool(name: &str) -> bool {
    GATEWAY_LOCAL_TOOLS.contains(&name)
}

/// Short identifier for a backend instance (first [`ID_PREFIX_LEN`] hex chars
/// of the UUID).
pub fn instance_short(id: &Uuid) -> String {
    let mut s = id.simple().to_string();
    s.truncate(ID_PREFIX_LEN);
    s
}

/// Prefix a backend-provided tool name with the instance short id.
pub fn encode_tool_name(id: &Uuid, original: &str) -> String {
    format!("{}{NAMESPACE_SEP}{original}", instance_short(id))
}

/// Strip the gateway prefix.  Returns `(instance_short, original_name)` when
/// the name is a prefixed backend tool; `None` for local / malformed names.
pub fn decode_tool_name(prefixed: &str) -> Option<(&str, &str)> {
    // Early reject for local tools (they never contain the prefix).
    if is_local_tool(prefixed) {
        return None;
    }

    let (prefix, rest) = prefixed.split_once(NAMESPACE_SEP)?;
    if prefix.len() != ID_PREFIX_LEN || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((prefix, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_decode_roundtrips() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let encoded = encode_tool_name(&id, "maya_geometry__create_sphere");
        assert_eq!(encoded, "abcdef01__maya_geometry__create_sphere");
        let (prefix, original) = decode_tool_name(&encoded).unwrap();
        assert_eq!(prefix, "abcdef01");
        assert_eq!(original, "maya_geometry__create_sphere");
    }

    #[test]
    fn local_tools_decode_to_none() {
        for name in GATEWAY_LOCAL_TOOLS {
            assert!(decode_tool_name(name).is_none(), "{name} should be local");
        }
    }

    #[test]
    fn decodes_skill_stub_name() {
        let id = Uuid::parse_str("11111111222233334444555566667777").unwrap();
        let encoded = encode_tool_name(&id, "__skill__maya-geometry");
        // encoded looks like "11111111____skill__maya-geometry"
        let (prefix, original) = decode_tool_name(&encoded).unwrap();
        assert_eq!(prefix, "11111111");
        assert_eq!(original, "__skill__maya-geometry");
    }

    #[test]
    fn rejects_non_hex_prefix() {
        assert!(decode_tool_name("abcdefgz__tool").is_none());
        assert!(decode_tool_name("short__tool").is_none());
        assert!(decode_tool_name("no_separator").is_none());
    }

    #[test]
    fn instance_short_is_exactly_eight_chars() {
        let id = Uuid::new_v4();
        assert_eq!(instance_short(&id).len(), ID_PREFIX_LEN);
    }

    #[test]
    fn instance_short_is_deterministic() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        assert_eq!(instance_short(&id), "abcdef01");
        // Stable across repeated calls.
        assert_eq!(instance_short(&id), instance_short(&id));
    }

    #[test]
    fn is_local_tool_recognizes_meta_and_skill_management() {
        // Discovery meta-tools.
        assert!(is_local_tool("list_dcc_instances"));
        assert!(is_local_tool("get_dcc_instance"));
        assert!(is_local_tool("connect_to_dcc"));
        // Skill-management tools.
        assert!(is_local_tool("search_skills"));
        assert!(is_local_tool("load_skill"));
        assert!(is_local_tool("unload_skill"));
        // Non-local.
        assert!(!is_local_tool("create_sphere"));
        assert!(!is_local_tool("abcdef01__create_sphere"));
    }

    #[test]
    fn different_uuids_produce_different_prefixes() {
        let a = Uuid::parse_str("aaaaaaaa0000000000000000aaaaaaaa").unwrap();
        let b = Uuid::parse_str("bbbbbbbb0000000000000000bbbbbbbb").unwrap();
        assert_ne!(instance_short(&a), instance_short(&b));
        assert_ne!(encode_tool_name(&a, "foo"), encode_tool_name(&b, "foo"));
    }

    #[test]
    fn decode_rejects_empty_string_and_separator_only() {
        assert!(decode_tool_name("").is_none());
        assert!(decode_tool_name("__").is_none());
        assert!(decode_tool_name("abcdef01__").is_some()); // valid prefix, empty suffix
        // NOTE: Empty suffix is accepted by the decoder — callers are
        // responsible for treating it as malformed where relevant.
    }
}
