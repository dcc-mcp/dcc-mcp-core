//! Namespace constants + trivial classifier predicates.
//!
//! Holds the three canonical name lists (`GATEWAY_LOCAL_TOOLS`,
//! `CORE_TOOL_NAMES`), the SEP-986 separator constants, and the small
//! prefix helpers used by every other namespace module.

use uuid::Uuid;

pub const GATEWAY_LOCAL_TOOLS: &[&str] = &[
    "list_dcc_instances",
    "get_dcc_instance",
    "connect_to_dcc",
    "acquire_dcc_instance",
    "release_dcc_instance",
    "list_skills",
    "search_skills",
    "get_skill_info",
    "load_skill",
    "unload_skill",
];

/// Core per-DCC tools that keep bare names (no skill prefix).
pub const CORE_TOOL_NAMES: &[&str] = &[
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

pub(crate) fn is_instance_prefix(s: &str) -> bool {
    s.len() == ID_PREFIX_LEN && s.chars().all(|c| c.is_ascii_hexdigit())
}
