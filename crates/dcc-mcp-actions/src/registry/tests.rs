//! ActionRegistry unit tests.

use super::*;

fn make_action(name: &str, dcc: &str) -> ActionMeta {
    ActionMeta {
        name: name.into(),
        description: format!("{name} description"),
        category: "geometry".into(),
        tags: vec!["test".into()],
        dcc: dcc.into(),
        version: "1.0.0".into(),
        ..Default::default()
    }
}

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

// ── Serialization ───────────────────────────────────────────────────────────

#[test]
fn test_action_meta_serde_round_trip() {
    let meta = ActionMeta {
        name: "render_scene".into(),
        description: "Renders the active scene".into(),
        category: "rendering".into(),
        tags: vec!["render".into(), "output".into()],
        dcc: "houdini".into(),
        version: "3.1.0".into(),
        input_schema: serde_json::json!({"type": "object"}),
        output_schema: serde_json::json!({"type": "string"}),
        source_file: Some("render.py".into()),
        skill_name: None,
        group: String::new(),
        enabled: true,
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: ActionMeta = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, back);
}

#[test]
fn test_action_meta_default_serialization() {
    let meta = ActionMeta::default();
    let json = serde_json::to_string(&meta).unwrap();
    let back: ActionMeta = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, back);
}

// ── Concurrency ─────────────────────────────────────────────────────────────

#[test]
fn test_registry_thread_safety() {
    use std::sync::Arc;
    use std::thread;

    let reg = Arc::new(ActionRegistry::new());
    let mut handles = vec![];

    for i in 0..10 {
        let reg = Arc::clone(&reg);
        handles.push(thread::spawn(move || {
            reg.register_action(ActionMeta {
                name: format!("action_{i}"),
                description: format!("Action {i}"),
                dcc: "test".into(),
                ..Default::default()
            });
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(reg.len(), 10);
}

#[test]
fn test_registry_concurrent_reads_while_writing() {
    use std::sync::Arc;
    use std::thread;

    let reg = Arc::new(ActionRegistry::new());
    // Pre-populate
    for i in 0..5 {
        reg.register_action(make_action(&format!("pre_{i}"), "maya"));
    }

    let mut handles = vec![];
    // Readers
    for _ in 0..4 {
        let r = Arc::clone(&reg);
        handles.push(thread::spawn(move || {
            for _ in 0..20 {
                let _ = r.list_actions(None);
                let _ = r.get_all_dccs();
            }
        }));
    }
    // Writer
    {
        let r = Arc::clone(&reg);
        handles.push(thread::spawn(move || {
            for i in 0..5 {
                r.register_action(make_action(&format!("new_{i}"), "blender"));
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
    // At least 5 pre-populated + up to 5 new
    assert!(reg.len() >= 5);
}

// ── search_actions ──────────────────────────────────────────────────────────

fn make_rich_action(name: &str, dcc: &str, category: &str, tags: Vec<&str>) -> ActionMeta {
    ActionMeta {
        name: name.into(),
        description: format!("{name} desc"),
        category: category.into(),
        tags: tags.into_iter().map(String::from).collect(),
        dcc: dcc.into(),
        version: "1.0.0".into(),
        ..Default::default()
    }
}

fn populate_search_registry() -> ActionRegistry {
    let reg = ActionRegistry::new();
    reg.register_action(make_rich_action(
        "create_sphere",
        "maya",
        "geometry",
        vec!["create", "mesh"],
    ));
    reg.register_action(make_rich_action(
        "delete_mesh",
        "maya",
        "geometry",
        vec!["delete", "mesh"],
    ));
    reg.register_action(make_rich_action(
        "render_scene",
        "maya",
        "rendering",
        vec!["render", "output"],
    ));
    reg.register_action(make_rich_action(
        "create_cube",
        "blender",
        "geometry",
        vec!["create", "mesh"],
    ));
    reg.register_action(make_rich_action(
        "bake_texture",
        "blender",
        "rendering",
        vec!["bake", "texture", "render"],
    ));
    reg
}

#[test]
fn search_by_category_returns_matching() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some("geometry"), &[], None);
    assert_eq!(
        results.len(),
        3,
        "should find 3 geometry actions across all DCCs"
    );
    assert!(results.iter().all(|m| m.category == "geometry"));
}

#[test]
fn search_by_tag_returns_matching() {
    let reg = populate_search_registry();
    let results = reg.search_actions(None, &["mesh"], None);
    assert_eq!(results.len(), 3, "create_sphere, delete_mesh, create_cube");
    assert!(results.iter().all(|m| m.tags.contains(&"mesh".to_string())));
}

#[test]
fn search_by_multiple_tags_all_required() {
    let reg = populate_search_registry();
    let results = reg.search_actions(None, &["create", "mesh"], None);
    assert_eq!(results.len(), 2, "create_sphere + create_cube");
}

#[test]
fn search_by_category_and_dcc() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some("geometry"), &[], Some("maya"));
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|m| m.dcc == "maya"));
}

#[test]
fn search_by_category_tag_and_dcc() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some("geometry"), &["create"], Some("blender"));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "create_cube");
}

#[test]
fn search_with_no_filters_returns_all() {
    let reg = populate_search_registry();
    let results = reg.search_actions(None, &[], None);
    assert_eq!(results.len(), 5);
}

#[test]
fn search_with_empty_category_matches_all_categories() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some(""), &[], None);
    // Empty string means no category filter
    assert_eq!(results.len(), 5);
}

#[test]
fn search_no_match_returns_empty() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some("nonexistent_category"), &[], None);
    assert!(results.is_empty());
}

#[test]
fn search_tag_not_found_returns_empty() {
    let reg = populate_search_registry();
    let results = reg.search_actions(None, &["nonexistent_tag"], None);
    assert!(results.is_empty());
}

#[test]
fn search_on_empty_registry_returns_empty() {
    let reg = ActionRegistry::new();
    let results = reg.search_actions(Some("geometry"), &["mesh"], Some("maya"));
    assert!(results.is_empty());
}

// ── get_categories ──────────────────────────────────────────────────────────

#[test]
fn get_categories_returns_sorted_deduped() {
    let reg = populate_search_registry();
    let cats = reg.get_categories(None);
    assert_eq!(cats, vec!["geometry", "rendering"]);
}

#[test]
fn get_categories_scoped_to_dcc() {
    let reg = populate_search_registry();
    let cats = reg.get_categories(Some("maya"));
    assert_eq!(cats, vec!["geometry", "rendering"]);
    let blender_cats = reg.get_categories(Some("blender"));
    assert_eq!(blender_cats, vec!["geometry", "rendering"]);
}

#[test]
fn get_categories_empty_registry_returns_empty() {
    let reg = ActionRegistry::new();
    assert!(reg.get_categories(None).is_empty());
}

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

#[test]
fn get_categories_skips_blank_category() {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "no_cat".into(),
        dcc: "maya".into(),
        category: String::new(), // blank category
        ..Default::default()
    });
    assert!(reg.get_categories(None).is_empty());
}

// ── get_tags ────────────────────────────────────────────────────────────────

#[test]
fn get_tags_returns_sorted_deduped() {
    let reg = populate_search_registry();
    let mut tags = reg.get_tags(None);
    tags.sort(); // already sorted, but just to be explicit
    // Expected: bake, create, delete, mesh, output, render, texture
    assert!(tags.contains(&"mesh".to_string()));
    assert!(tags.contains(&"create".to_string()));
    assert!(tags.contains(&"render".to_string()));
    // No duplicates
    let before_dedup = tags.len();
    let mut deduped = tags.clone();
    deduped.dedup();
    assert_eq!(before_dedup, deduped.len());
}

#[test]
fn get_tags_scoped_to_dcc() {
    let reg = populate_search_registry();
    let maya_tags = reg.get_tags(Some("maya"));
    // Maya has: create, mesh, delete, render, output
    assert!(maya_tags.contains(&"create".to_string()));
    assert!(maya_tags.contains(&"output".to_string()));
    // "texture" is only in blender
    assert!(!maya_tags.contains(&"texture".to_string()));
}

#[test]
fn get_tags_empty_registry_returns_empty() {
    let reg = ActionRegistry::new();
    assert!(reg.get_tags(None).is_empty());
}

// ── count_actions ───────────────────────────────────────────────────────────

#[test]
fn count_actions_matches_search_results() {
    let reg = populate_search_registry();
    assert_eq!(
        reg.count_actions(Some("geometry"), &["create"], None),
        reg.search_actions(Some("geometry"), &["create"], None)
            .len()
    );
    assert_eq!(reg.count_actions(None, &[], None), reg.len());
}
