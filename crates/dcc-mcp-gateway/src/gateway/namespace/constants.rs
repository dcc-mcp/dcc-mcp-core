//! Namespace constants + trivial classifier predicates.
//!
//! Holds the three canonical name lists (`GATEWAY_LOCAL_TOOLS`,
//! `CORE_TOOL_NAMES`), the SEP-986 separator constants, and the small
//! prefix helpers used by every other namespace module.

use uuid::Uuid;

/// Tools that are answered by the gateway itself (never fanned out
/// to a backend).
///
/// Includes the skill management verbs (`list_skills`, `load_skill`,
/// …) and the issue #655 dynamic-capability wrappers
/// (`search_tools`, `describe_tool`, `call_tool`). The dispatch
/// handler short-circuits on these names so the fan-out path can
/// stay free of carve-outs.
pub const GATEWAY_LOCAL_TOOLS: &[&str] = &[
    "acquire_dcc_instance",
    "release_dcc_instance",
    "list_skills",
    "search_skills",
    "get_skill_info",
    "load_skill",
    "unload_skill",
    // #655 dynamic-capability wrappers (shared service with the REST
    // API). Registered as local so the fan-out path never tries to
    // forward them to a backend — the wrappers route through the
    // gateway's capability index instead.
    "search_tools",
    "describe_tool",
    "call_tool",
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

/// Length of the truncated instance UUID prefix used in encoded
/// tool names (e.g. `maya.abcdef01.create_sphere`). 8 hex chars
/// give 32 bits of entropy — enough to disambiguate among the
/// dozens of instances a gateway will ever see live, while staying
/// short enough to stay readable in log lines and error messages.
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

/// Cursor-safe gateway tool-name prefix (issue #656).
///
/// Some MCP clients — notably Cursor — filter out tool names that
/// contain anything other than `[A-Za-z0-9_]`, which excludes the
/// SEP-986-legal `.` and `-` separators the gateway has historically
/// emitted. The cursor-safe form `i_<id8>__<escaped_tool>` keeps every
/// published byte inside that stricter alphabet while staying
/// reversible thanks to the escape vocabulary in
/// [`encode::escape_cursor_safe`](super::encode).
///
/// The leading `i_` (for *instance*) exists to disambiguate encoded
/// names from bare backend tools such as `create_sphere` without
/// requiring callers to peek at the id byte itself — an 8-hex-char
/// string like `abcdef01` is a perfectly valid bare tool name on its
/// own.
pub const CURSOR_SAFE_PREFIX: &str = "i_";

/// Separator between the cursor-safe instance prefix and the escaped
/// backend tool name (issue #656). Chosen as `__` because the outer
/// tool-name regex allows `_`, and a double-underscore is cheap to
/// `split_once` while remaining visually distinct from the single
/// underscores used inside the escape vocabulary (`_U_` / `_D_` /
/// `_H_`).
pub const CURSOR_SAFE_SEP: &str = "__";

/// `true` when `name` is a gateway-local tool that must never be
/// forwarded to a backend (cf. [`GATEWAY_LOCAL_TOOLS`]).
pub fn is_local_tool(name: &str) -> bool {
    GATEWAY_LOCAL_TOOLS.contains(&name)
}

/// `true` when `name` is a per-DCC core tool that keeps a bare name
/// even after skill prefixing (cf. [`CORE_TOOL_NAMES`]).
pub fn is_core_tool(name: &str) -> bool {
    CORE_TOOL_NAMES.contains(&name)
}

/// Truncate a UUID to its first [`ID_PREFIX_LEN`] hex chars — the
/// canonical short form used inside encoded gateway tool names.
pub fn instance_short(id: &Uuid) -> String {
    let mut s = id.simple().to_string();
    s.truncate(ID_PREFIX_LEN);
    s
}

pub(crate) fn is_instance_prefix(s: &str) -> bool {
    s.len() == ID_PREFIX_LEN && s.chars().all(|c| c.is_ascii_hexdigit())
}
