//! Bare-name resolver (#307) — pure function, no global state.
//!
//! Decides — on every `tools/list` response — which skill actions may
//! publish under their **bare name** instead of `<skill>__<action>`.
//!
//! Collision diagnostics are emitted through [`super::observability`]
//! so this module stays a pure mathematical function over its inputs.

use std::collections::{HashMap, HashSet};

use super::constants::{SKILL_TOOL_SEP, is_core_tool};
use super::encode::extract_bare_tool_name;
use super::observability::warn_bare_collision_once;

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
/// * the bare name is not itself skill-qualified.
///
/// Returns the set of `(skill_name, action_name)` tuples that should be
/// published bare. Callers that find a tuple in the set emit
/// `meta.name.strip_prefix(...)`; callers that don't, fall back to the
/// `<skill>__<action>` form produced by [`super::skill_tool_name`].
///
/// Collisions (same bare name from two different skills) are logged once
/// per process via [`warn_bare_collision_once`].
///
/// # Examples
/// ```
/// # use dcc_mcp_gateway_core::naming::{resolve_bare_names, BareNameInput};
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
