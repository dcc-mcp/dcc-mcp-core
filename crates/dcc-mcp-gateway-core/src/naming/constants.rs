//! Naming vocabulary: canonical tool name lists, separators, classifiers.
//!
//! Holds the published name lists (`GATEWAY_LOCAL_TOOLS`,
//! `CORE_TOOL_NAMES`), the tool-name separator constants, and the small
//! classifier predicates layered on top of [`super::primitives`].
//!
//! This module is intentionally free of UUID encoding logic — that lives
//! in [`super::encode`] — so changes to the encoded wire form do not
//! ripple through the vocabulary.

use super::primitives::ID_PREFIX_LEN;

/// Tools that are answered by the gateway itself (never fanned out
/// to a backend).
///
/// Includes the skill management verbs (`list_skills`, `load_skill`,
/// …) and the issue #655 dynamic-capability wrappers
/// (`search_tools`, `describe_tool`, `call_tool`). The dispatch
/// handler short-circuits on these names so the fan-out path can
/// stay free of carve-outs.
/// Minimal gateway MCP surface (RFC #998 follow-up — consolidated meta-tools).
///
/// Replaces the previous 13-tool split (`search_tools` + `search_skills` +
/// `call_tool` + `call_tools` + pooling + group helpers, …). Agents discover
/// backend work through [`search`] → [`describe`] → [`call`]; skill lifecycle
/// uses [`load_skill`] / [`unload_skill`]; multi-instance pooling uses [`lease`].
pub const GATEWAY_LOCAL_TOOLS: &[&str] = &[
    "lease",
    "search",
    "describe",
    "call",
    "load_skill",
    "unload_skill",
];

/// Core per-DCC tools that keep bare names (no skill prefix).
pub const CORE_TOOL_NAMES: &[&str] = &[
    // Consolidated gateway surface (also in GATEWAY_LOCAL_TOOLS).
    "search",
    "describe",
    "call",
    "lease",
    "load_skill",
    "unload_skill",
    // Legacy per-DCC / gateway aliases still echoed by some backends.
    "list_skills",
    "get_skill_info",
    "search_skills",
    "activate_tool_group",
    "deactivate_tool_group",
    "search_tools",
    "describe_tool",
    "call_tool",
    "call_tools",
    "acquire_dcc_instance",
    "release_dcc_instance",
    "jobs_get_status",
    "jobs_cleanup",
    "jobs_checkpoint_status",
    "jobs_resume_context",
    "project_save",
    "project_load",
    "project_resume",
    "project_status",
    "workflows_run",
    "workflows_get_status",
    "workflows_cancel",
    "workflows_lookup",
    "workflows_resume",
    "workflows_list",
    "workflows_describe",
];

/// Client-safe gateway instance separator for direct encoded names.
/// MCP aggregation surfaces prefer [`super::encode::encode_tool_name_cursor_safe`];
/// [`super::encode::decode_tool_name`] only accepts the `i_` form.
pub const INSTANCE_SEP: &str = "__";
/// Skill→tool separator for per-DCC proactive namespacing.
pub const SKILL_TOOL_SEP: &str = "__";

/// Cursor-safe gateway tool-name prefix (issue #656).
///
/// Some MCP clients — notably Cursor — filter out tool names that
/// contain anything other than `[A-Za-z0-9_]`. The cursor-safe form
/// `i_<id8>__<escaped_tool>` keeps every published byte inside that
/// stricter alphabet while staying
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
#[must_use]
pub fn is_local_tool(name: &str) -> bool {
    GATEWAY_LOCAL_TOOLS.contains(&name)
}

/// `true` when `name` is a per-DCC core tool that keeps a bare name
/// even after skill prefixing (cf. [`CORE_TOOL_NAMES`]).
#[must_use]
pub fn is_core_tool(name: &str) -> bool {
    CORE_TOOL_NAMES.contains(&name)
}

/// Return `true` when `s` is exactly an 8-hex-char instance prefix
/// (the shape produced by [`super::primitives::instance_short`]).
///
/// Used by the codec in [`super::encode`] to distinguish gateway-encoded
/// names from skill-qualified or bare backend names.
pub(super) fn is_instance_prefix(s: &str) -> bool {
    s.len() == ID_PREFIX_LEN && s.chars().all(|c| c.is_ascii_hexdigit())
}
