//! Tests for register_batch and unregister operations.

use super::fixtures::make_action;
use super::*;

// ── register_batch ──────────────────────────────────────────────────────────

#[test]
fn register_batch_empty_slice_is_noop() {
    let reg = ActionRegistry::new();
    reg.register_batch(std::iter::empty::<ActionMeta>());
    assert_eq!(reg.len(), 0);
}

#[test]
fn register_batch_inserts_all_actions() {
    let reg = ActionRegistry::new();
    let actions = vec![
        make_action("action_a", "maya"),
        make_action("action_b", "maya"),
        make_action("action_c", "blender"),
    ];
    reg.register_batch(actions);
    assert_eq!(reg.len(), 3);
    assert!(reg.get_action("action_a", None).is_some());
    assert!(reg.get_action("action_b", None).is_some());
    assert!(reg.get_action("action_c", None).is_some());
}

#[test]
fn register_batch_respects_dcc_scope() {
    let reg = ActionRegistry::new();
    reg.register_batch([
        make_action("op1", "maya"),
        make_action("op2", "blender"),
        make_action("op3", "maya"),
    ]);
    assert_eq!(reg.list_actions_for_dcc("maya").len(), 2);
    assert_eq!(reg.list_actions_for_dcc("blender").len(), 1);
}

#[test]
fn register_batch_overwrites_existing() {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "duplicate".into(),
        description: "original".into(),
        dcc: "maya".into(),
        ..Default::default()
    });
    reg.register_batch([ActionMeta {
        name: "duplicate".into(),
        description: "replaced".into(),
        dcc: "maya".into(),
        ..Default::default()
    }]);
    let meta = reg.get_action("duplicate", None).unwrap();
    assert_eq!(meta.description, "replaced");
    // Len should still be 1 (overwrite, not append).
    assert_eq!(reg.len(), 1);
}

// ── unregister ──────────────────────────────────────────────────────────────

#[test]
fn unregister_returns_true_when_found() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("to_remove", "maya"));
    assert!(reg.unregister("to_remove", None));
}

#[test]
fn unregister_returns_false_when_not_found() {
    let reg = ActionRegistry::new();
    assert!(!reg.unregister("nonexistent", None));
}

#[test]
fn unregister_global_removes_from_all_dcc_maps() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("shared", "maya"));
    reg.register_action(make_action("shared", "blender"));
    // Global unregister — removes from both DCC maps.
    assert!(reg.unregister("shared", None));
    assert_eq!(reg.len(), 0);
    assert!(reg.list_actions_for_dcc("maya").is_empty());
    assert!(reg.list_actions_for_dcc("blender").is_empty());
}

#[test]
fn unregister_scoped_removes_only_target_dcc() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("op", "maya"));
    reg.register_action(make_action("op", "blender"));
    // Remove only from maya.
    assert!(reg.unregister("op", Some("maya")));
    // Blender still has it.
    assert!(reg.get_action("op", Some("blender")).is_some());
    // Global entry removed because blender still references it? No —
    // the global entry stays as long as any DCC references it.
    assert!(reg.get_action("op", None).is_some());
}

#[test]
fn unregister_scoped_removes_global_when_last_dcc() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("only_maya", "maya"));
    // Only registered in one DCC — removing it should clear global too.
    assert!(reg.unregister("only_maya", Some("maya")));
    assert!(reg.get_action("only_maya", None).is_none());
    assert_eq!(reg.len(), 0);
}

#[test]
fn unregister_scoped_nonexistent_dcc_returns_false() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("op", "maya"));
    assert!(!reg.unregister("op", Some("blender")));
    // Original still present.
    assert!(reg.get_action("op", None).is_some());
}

#[test]
fn unregister_idempotent_second_call_returns_false() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("once", "maya"));
    assert!(reg.unregister("once", None));
    assert!(!reg.unregister("once", None));
}
