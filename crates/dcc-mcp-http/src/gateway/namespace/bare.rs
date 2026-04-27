//! Bare-name resolver (#307) + one-shot deprecation warning helpers.
//!
//! The resolver decides — on every `tools/list` response — which skill
//! actions may publish under their **bare name** instead of
//! `<skill>.<action>`. Collisions are logged once per process through
//! [`warn_bare_collision_once`]; clients that still call the legacy
//! prefixed form are nudged through [`warn_legacy_prefixed_once`].

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use super::constants::{SKILL_TOOL_SEP, is_core_tool};
use super::encode::extract_bare_tool_name;

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
/// `<skill>.<action>` form produced by [`super::skill_tool_name`].
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
