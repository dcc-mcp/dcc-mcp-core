//! Tool-name namespace helpers for the aggregating gateway.
//!
//! ## Per-DCC server: proactive `<skill>.<name>` namespacing (#238)
//!
//! Non-core tools registered from a skill use `<skill-name>.<tool-name>` format
//! (e.g. `maya-animation.set_keyframe`) so the AI agent immediately sees which
//! skill a tool belongs to.
//!
//! ## Per-DCC server: bare-name mode (#307)
//!
//! When enabled via [`crate::McpHttpConfig::bare_tool_names`] (default `true`),
//! the server publishes tools under their **bare action name** whenever no
//! other skill on the same instance registers the same bare name. Collisions
//! fall back to `<skill>.<action>` and log a one-shot warning. This cuts the
//! `tools/list` token footprint by ~40% on Maya-sized skill sets without
//! breaking routing — [`handle_tools_call`](crate::handler) accepts both forms
//! for one release cycle (same policy as SEP-986 #258/#261).
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

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
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

// ── Bare-name resolver (#307) ────────────────────────────────────────────────

/// Reference to an action for the purposes of bare-name collision analysis.
///
/// Borrows strings from the caller so the resolver stays allocation-light —
/// `resolve_bare_names` is called on every `tools/list` response.
#[derive(Debug, Clone, Copy)]
pub struct BareNameInput<'a> {
    /// The owning skill's name (empty when the action is not skill-scoped).
    pub skill_name: &'a str,
    /// The registry-level action name (e.g. `maya_animation__set_keyframe`).
    pub action_name: &'a str,
}

/// Decide which actions may publish under their **bare action name** on a
/// single DCC instance.
///
/// An action is eligible when:
/// * it belongs to a skill, AND
/// * its bare name (stripped of the `<skill>__` prefix, when present) is
///   unique across **all** skill-scoped actions on the instance, AND
/// * the bare name is not a reserved core-tool name (those already carry
///   first-class positions in `tools/list`), AND
/// * the bare name contains no `.` (which would create an ambiguous
///   `{id8}.a.b` gateway encoding).
///
/// Returns the set of `(skill_name, action_name)` tuples that should be
/// published bare. Callers that find a tuple in the set emit
/// `meta.name.strip_prefix(...)`; callers that don't, fall back to the
/// `<skill>.<action>` form produced by [`skill_tool_name`].
///
/// Collisions (same bare name from two different skills) are logged once
/// per process via [`warn_bare_collision_once`].
///
/// # Examples
/// ```
/// # use dcc_mcp_http::gateway::namespace::{resolve_bare_names, BareNameInput};
/// let inputs = [
///     BareNameInput { skill_name: "maya-anim", action_name: "maya_anim__set_keyframe" },
///     BareNameInput { skill_name: "maya-geo",  action_name: "maya_geo__create_sphere" },
/// ];
/// let bare = resolve_bare_names(&inputs);
/// assert!(bare.contains(&("maya-anim".to_string(), "maya_anim__set_keyframe".to_string())));
/// assert!(bare.contains(&("maya-geo".to_string(),  "maya_geo__create_sphere".to_string())));
/// ```
#[must_use]
pub fn resolve_bare_names(inputs: &[BareNameInput<'_>]) -> HashSet<(String, String)> {
    // Count how many distinct skills register each bare name.
    let mut counts: HashMap<String, Vec<&str>> = HashMap::new();
    for inp in inputs {
        if inp.skill_name.is_empty() {
            continue;
        }
        let bare = extract_bare_tool_name(inp.skill_name, inp.action_name);
        if is_core_tool(bare) || bare.contains(SKILL_TOOL_SEP) {
            continue;
        }
        counts
            .entry(bare.to_string())
            .or_default()
            .push(inp.skill_name);
    }

    let mut out: HashSet<(String, String)> = HashSet::new();
    for inp in inputs {
        if inp.skill_name.is_empty() {
            continue;
        }
        let bare = extract_bare_tool_name(inp.skill_name, inp.action_name);
        let Some(skills) = counts.get(bare) else {
            continue;
        };
        // Unique within the instance when every entry refers to the same skill.
        let first = skills.first().copied().unwrap_or("");
        let unique = skills.iter().all(|s| *s == first);
        if unique {
            out.insert((inp.skill_name.to_string(), inp.action_name.to_string()));
        } else {
            warn_bare_collision_once(bare, skills);
        }
    }
    out
}

static BARE_COLLISIONS_WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn warned_bare_slot() -> &'static Mutex<HashSet<String>> {
    BARE_COLLISIONS_WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Emit a one-shot warning for a bare-name collision.
///
/// Each distinct `bare` string is logged at most once per process to keep
/// the hot path quiet when multiple skills intentionally overlap
/// (e.g. both `maya-anim` and `blender-anim` expose `set_keyframe`).
fn warn_bare_collision_once(bare: &str, skills: &[&str]) {
    let Ok(mut slot) = warned_bare_slot().lock() else {
        return;
    };
    if slot.insert(bare.to_string()) {
        let unique: Vec<&&str> = {
            let mut s: Vec<&&str> = skills.iter().collect();
            s.sort();
            s.dedup();
            s
        };
        tracing::warn!(
            tool = bare,
            skills = ?unique,
            "bare tool name collision — falling back to `<skill>.<action>` form; \
             set bare_tool_names=false to silence, or rename one action in SKILL.md"
        );
    }
}

static LEGACY_PREFIXED_WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn warned_prefixed_slot() -> &'static Mutex<HashSet<String>> {
    LEGACY_PREFIXED_WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Emit a one-shot deprecation warning when a client calls a tool using the
/// legacy `<skill>.<action>` form that could also have been reached via its
/// bare name.
///
/// Used by `handle_tools_call` so operators learn which integrations still
/// hard-code the prefixed form without drowning production logs when a
/// dashboard retries the same tool every few seconds.
pub fn warn_legacy_prefixed_once(prefixed: &str) {
    let Ok(mut slot) = warned_prefixed_slot().lock() else {
        return;
    };
    if slot.insert(prefixed.to_string()) {
        tracing::warn!(
            tool = prefixed,
            "legacy `<skill>.<action>` tool name accepted — prefer the bare name; \
             the prefix fallback will be removed in one release"
        );
    }
}

#[cfg(test)]
#[doc(hidden)]
pub fn __reset_warn_state_for_tests() {
    if let Ok(mut s) = warned_bare_slot().lock() {
        s.clear();
    }
    if let Ok(mut s) = warned_prefixed_slot().lock() {
        s.clear();
    }
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

    // ── #307 bare-name resolver ──────────────────────────────────────────────

    #[test]
    fn bare_name_when_unique_within_instance() {
        __reset_warn_state_for_tests();
        let inputs = [
            BareNameInput {
                skill_name: "maya-anim",
                action_name: "maya_anim__set_keyframe",
            },
            BareNameInput {
                skill_name: "maya-geo",
                action_name: "maya_geo__create_sphere",
            },
        ];
        let bare = resolve_bare_names(&inputs);
        assert!(bare.contains(&(
            "maya-anim".to_string(),
            "maya_anim__set_keyframe".to_string()
        )));
        assert!(bare.contains(&(
            "maya-geo".to_string(),
            "maya_geo__create_sphere".to_string()
        )));
        assert_eq!(bare.len(), 2);
    }

    #[test]
    fn falls_back_to_skill_prefix_on_collision() {
        __reset_warn_state_for_tests();
        // Both skills expose a `set_keyframe` action → collision; neither
        // should be emitted bare.
        let inputs = [
            BareNameInput {
                skill_name: "maya-anim",
                action_name: "maya_anim__set_keyframe",
            },
            BareNameInput {
                skill_name: "blender-anim",
                action_name: "blender_anim__set_keyframe",
            },
            BareNameInput {
                skill_name: "maya-geo",
                action_name: "maya_geo__create_sphere",
            },
        ];
        let bare = resolve_bare_names(&inputs);
        assert!(bare.contains(&(
            "maya-geo".to_string(),
            "maya_geo__create_sphere".to_string()
        )));
        assert!(!bare.contains(&(
            "maya-anim".to_string(),
            "maya_anim__set_keyframe".to_string()
        )));
        assert!(!bare.contains(&(
            "blender-anim".to_string(),
            "blender_anim__set_keyframe".to_string()
        )));
    }

    #[test]
    fn same_skill_registering_same_bare_twice_is_not_a_collision() {
        __reset_warn_state_for_tests();
        // Re-registering the same (skill, action) shape twice must not be
        // mistaken for a cross-skill collision.
        let inputs = [
            BareNameInput {
                skill_name: "maya-anim",
                action_name: "maya_anim__set_keyframe",
            },
            BareNameInput {
                skill_name: "maya-anim",
                action_name: "maya_anim__set_keyframe",
            },
        ];
        let bare = resolve_bare_names(&inputs);
        assert!(bare.contains(&(
            "maya-anim".to_string(),
            "maya_anim__set_keyframe".to_string()
        )));
    }

    #[test]
    fn core_tool_names_are_never_bare_eligible() {
        __reset_warn_state_for_tests();
        // `load_skill` is reserved and already has a first-class position
        // in tools/list; emitting it bare from a skill would cause a dispatch
        // ambiguity against the meta-tool.
        let inputs = [BareNameInput {
            skill_name: "rogue-skill",
            action_name: "rogue_skill__load_skill",
        }];
        assert!(resolve_bare_names(&inputs).is_empty());
    }

    #[test]
    fn actions_without_skill_are_skipped_by_resolver() {
        __reset_warn_state_for_tests();
        // Actions not registered from a skill keep their canonical name;
        // the resolver simply ignores them rather than asserting they are
        // unique.
        let inputs = [BareNameInput {
            skill_name: "",
            action_name: "standalone_action",
        }];
        assert!(resolve_bare_names(&inputs).is_empty());
    }

    #[test]
    fn warn_legacy_prefixed_once_is_one_shot_per_name() {
        __reset_warn_state_for_tests();
        // Two calls with the same name should not panic or repeatedly warn;
        // we can only verify the API surface is idempotent here — actual
        // log output is observed via `cargo test --nocapture` if needed.
        warn_legacy_prefixed_once("maya-anim.set_keyframe");
        warn_legacy_prefixed_once("maya-anim.set_keyframe");
        warn_legacy_prefixed_once("maya-geo.create_sphere");
    }
}
