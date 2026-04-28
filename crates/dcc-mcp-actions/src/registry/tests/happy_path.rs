//! Happy-path and edge-case tests for ActionRegistry basic operations.

use super::fixtures::make_action;
use super::*;

// ── Happy path ─────────────────────────────────────────────────────────────

#[test]
fn test_registry_register_and_get() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("create_sphere", "maya"));

    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
    assert!(reg.get_action("create_sphere", None).is_some());
    assert!(reg.get_action("create_sphere", Some("maya")).is_some());
}

#[test]
fn test_registry_default_is_empty() {
    let reg = ActionRegistry::default();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn test_registry_get_returns_correct_metadata() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("delete_mesh", "blender"));

    let meta = reg.get_action("delete_mesh", None).unwrap();
    assert_eq!(meta.name, "delete_mesh");
    assert_eq!(meta.dcc, "blender");
    assert_eq!(meta.version, "1.0.0");
}

#[test]
fn test_registry_list_actions_all() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("a1", "maya"));
    reg.register_action(make_action("a2", "maya"));
    reg.register_action(make_action("a3", "blender"));

    let all = reg.list_actions(None);
    assert_eq!(all.len(), 3);
}

#[test]
fn test_registry_list_actions_for_dcc() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("ma1", "maya"));
    reg.register_action(make_action("ma2", "maya"));
    reg.register_action(make_action("bl1", "blender"));

    let maya_actions = reg.list_actions(Some("maya"));
    assert_eq!(maya_actions.len(), 2);
    let blender_actions = reg.list_actions(Some("blender"));
    assert_eq!(blender_actions.len(), 1);
}

#[test]
fn test_registry_list_actions_for_dcc_names() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("x", "houdini"));

    let names = reg.list_actions_for_dcc("houdini");
    assert_eq!(names, vec!["x"]);
}

#[test]
fn test_registry_get_all_dccs() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("a", "maya"));
    reg.register_action(make_action("b", "blender"));
    reg.register_action(make_action("c", "maya"));

    let mut dccs = reg.get_all_dccs();
    dccs.sort();
    assert_eq!(dccs, vec!["blender", "maya"]);
}

// ── Error / edge paths ──────────────────────────────────────────────────────

#[test]
fn test_registry_get_unknown_returns_none() {
    let reg = ActionRegistry::new();
    assert!(reg.get_action("nonexistent", None).is_none());
    assert!(reg.get_action("nonexistent", Some("maya")).is_none());
}

#[test]
fn test_registry_get_action_wrong_dcc_returns_none() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("create_sphere", "maya"));
    assert!(reg.get_action("create_sphere", Some("blender")).is_none());
}

#[test]
fn test_registry_list_for_unknown_dcc_empty() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("a", "maya"));
    assert!(reg.list_actions(Some("unknown_dcc")).is_empty());
    assert!(reg.list_actions_for_dcc("unknown_dcc").is_empty());
}

#[test]
fn test_registry_reset_clears_all() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("a", "maya"));
    reg.register_action(make_action("b", "blender"));
    assert_eq!(reg.len(), 2);

    reg.reset();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.list_actions(None).is_empty());
    assert!(reg.get_all_dccs().is_empty());
}

#[test]
fn test_registry_overwrite_existing_action() {
    let reg = ActionRegistry::new();
    reg.register_action(make_action("my_action", "maya"));
    let updated = ActionMeta {
        name: "my_action".into(),
        description: "updated description".into(),
        dcc: "maya".into(),
        version: "2.0.0".into(),
        ..Default::default()
    };
    reg.register_action(updated);

    // Latest version wins
    let meta = reg.get_action("my_action", None).unwrap();
    assert_eq!(meta.version, "2.0.0");
    assert_eq!(meta.description, "updated description");
    // len should still be 1
    assert_eq!(reg.len(), 1);
}

#[test]
fn test_registry_tags_preserved() {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "tagged_action".into(),
        dcc: "maya".into(),
        tags: vec!["sculpt".into(), "deform".into()],
        ..Default::default()
    });
    let meta = reg.get_action("tagged_action", None).unwrap();
    assert_eq!(meta.tags, vec!["sculpt", "deform"]);
}

#[test]
fn test_registry_source_file_optional() {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "no_source".into(),
        dcc: "blender".into(),
        source_file: None,
        ..Default::default()
    });
    let meta = reg.get_action("no_source", None).unwrap();
    assert!(meta.source_file.is_none());

    reg.register_action(ActionMeta {
        name: "with_source".into(),
        dcc: "blender".into(),
        source_file: Some("/path/to/action.py".into()),
        ..Default::default()
    });
    let meta2 = reg.get_action("with_source", None).unwrap();
    assert_eq!(meta2.source_file.as_deref(), Some("/path/to/action.py"));
}
