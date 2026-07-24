//! Bounded semantic deltas between two JSON state snapshots.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default maximum number of changes returned for one state transition.
pub const DEFAULT_STATE_DELTA_MAX_CHANGES: usize = 128;

/// One changed JSON-pointer path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateChange {
    /// RFC 6901 JSON pointer. `/` denotes the root value.
    pub path: String,
    /// Whether the value was added, removed, or changed.
    pub kind: StateChangeKind,
    /// Previous value when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    /// Current value when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}

/// Kind of one semantic state change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateChangeKind {
    /// A path is new in the current state.
    Added,
    /// A path no longer exists in the current state.
    Removed,
    /// A path exists in both states with a different value.
    Changed,
}

/// Bounded delta between two state snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateDelta {
    /// Contract version for additive evolution.
    pub schema_version: u8,
    /// True when this is the first observed state and therefore a baseline.
    pub baseline: bool,
    /// Changed paths in deterministic order.
    pub changes: Vec<StateChange>,
    /// True when more changed paths existed than the configured bound.
    pub truncated: bool,
}

impl StateDelta {
    /// Whether the transition contains at least one semantic change.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.changes.is_empty()
    }
}

/// Compute a deterministic, bounded semantic delta.
#[must_use]
pub fn diff_json_state(before: Option<&Value>, after: &Value, max_changes: usize) -> StateDelta {
    let baseline = before.is_none();
    let mut delta = StateDelta {
        schema_version: 1,
        baseline,
        changes: Vec::new(),
        truncated: false,
    };
    if let Some(before) = before {
        walk(
            Some(before),
            Some(after),
            "",
            max_changes.max(1),
            &mut delta,
        );
    }
    delta
}

fn walk(
    before: Option<&Value>,
    after: Option<&Value>,
    path: &str,
    max_changes: usize,
    delta: &mut StateDelta,
) {
    if before == after {
        return;
    }
    if delta.changes.len() >= max_changes {
        delta.truncated = true;
        return;
    }
    match (before, after) {
        (Some(Value::Object(before)), Some(Value::Object(after))) => {
            let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
            for key in keys {
                walk(
                    before.get(key),
                    after.get(key),
                    &child_path(path, key),
                    max_changes,
                    delta,
                );
            }
        }
        (Some(Value::Array(before)), Some(Value::Array(after))) => {
            for index in 0..before.len().max(after.len()) {
                walk(
                    before.get(index),
                    after.get(index),
                    &child_path(path, &index.to_string()),
                    max_changes,
                    delta,
                );
            }
        }
        (None, Some(Value::Object(after))) if !after.is_empty() => {
            for (key, value) in after {
                walk(
                    None,
                    Some(value),
                    &child_path(path, key),
                    max_changes,
                    delta,
                );
            }
        }
        (Some(Value::Object(before)), None) if !before.is_empty() => {
            for (key, value) in before {
                walk(
                    Some(value),
                    None,
                    &child_path(path, key),
                    max_changes,
                    delta,
                );
            }
        }
        (None, Some(Value::Array(after))) if !after.is_empty() => {
            for (index, value) in after.iter().enumerate() {
                walk(
                    None,
                    Some(value),
                    &child_path(path, &index.to_string()),
                    max_changes,
                    delta,
                );
            }
        }
        (Some(Value::Array(before)), None) if !before.is_empty() => {
            for (index, value) in before.iter().enumerate() {
                walk(
                    Some(value),
                    None,
                    &child_path(path, &index.to_string()),
                    max_changes,
                    delta,
                );
            }
        }
        (before, after) => delta.changes.push(StateChange {
            path: if path.is_empty() {
                "/".to_owned()
            } else {
                path.to_owned()
            },
            kind: match (before, after) {
                (None, Some(_)) => StateChangeKind::Added,
                (Some(_), None) => StateChangeKind::Removed,
                _ => StateChangeKind::Changed,
            },
            before: before.cloned(),
            after: after.cloned(),
        }),
    }
}

fn child_path(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_state_delta_is_bounded_and_uses_stable_pointer_paths() {
        let before = json!({"focus": "a", "panel/name": {"open": false}, "items": [1, 2]});
        let after =
            json!({"focus": "b", "panel/name": {"open": true}, "items": [1, 3], "playing": true});
        let delta = diff_json_state(Some(&before), &after, 3);

        assert_eq!(delta.changes.len(), 3);
        assert!(delta.truncated);
        assert_eq!(delta.changes[0].path, "/focus");
        assert_eq!(delta.changes[1].path, "/items/1");
        assert_eq!(delta.changes[2].path, "/panel~1name/open");
    }

    #[test]
    fn first_state_is_a_baseline_and_equal_states_have_no_changes() {
        let state = json!({"scene": "arena", "playing": true});
        assert!(diff_json_state(None, &state, 8).baseline);
        let delta = diff_json_state(Some(&state), &state, 8);
        assert!(!delta.baseline);
        assert!(!delta.changed());
    }

    #[test]
    fn exact_bound_is_not_truncated_when_remaining_paths_are_equal() {
        let before = json!({"a": 1, "b": 2});
        let after = json!({"a": 3, "b": 2});
        let delta = diff_json_state(Some(&before), &after, 1);
        assert_eq!(delta.changes.len(), 1);
        assert!(!delta.truncated);
    }
}
