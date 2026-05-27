//! One-shot warning helpers for the naming domain.
//!
//! These functions own the only piece of mutable global state in
//! [`crate::naming`]. They are intentionally separated from the pure
//! resolver in [`crate::naming::bare`] so the pure path stays free of
//! interior mutability and can be reasoned about as a mathematical
//! function.
//!
//! Each warning is emitted at most once per process so that overlapping
//! skills, or a polling dashboard that retries the same call every few
//! seconds, do not drown production logs.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

static BARE_COLLISIONS_WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static SKILL_QUALIFIED_WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn warned_bare_slot() -> &'static Mutex<HashSet<String>> {
    BARE_COLLISIONS_WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn warned_prefixed_slot() -> &'static Mutex<HashSet<String>> {
    SKILL_QUALIFIED_WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Emit a one-shot warning for a bare-name collision.
///
/// Each distinct `bare` string is logged at most once per process to
/// keep the hot path quiet when multiple skills intentionally overlap
/// (e.g. both `maya-anim` and `blender-anim` expose `set_keyframe`).
///
/// Intended for use by [`crate::naming::resolve_bare_names`]; not part
/// of the published surface because the collision shape is an
/// implementation detail of the resolver.
pub(super) fn warn_bare_collision_once(bare: &str, skills: &[&str]) {
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
            "bare tool name collision — falling back to `<skill>__<action>` form; \
             set bare_tool_names=false to silence, or rename one action in SKILL.md"
        );
    }
}

/// Emit a one-shot warning when a client calls a skill-qualified tool
/// that could also have been reached via its bare name.
///
/// Used by the gateway's tools/call handler so operators learn which
/// integrations still hard-code the prefixed form without drowning
/// production logs when a dashboard retries the same tool every few
/// seconds.
pub fn warn_skill_qualified_once(prefixed: &str) {
    let Ok(mut slot) = warned_prefixed_slot().lock() else {
        return;
    };
    if slot.insert(prefixed.to_string()) {
        tracing::warn!(
            tool = prefixed,
            "skill-qualified tool name accepted — prefer the bare name when it is unique"
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
