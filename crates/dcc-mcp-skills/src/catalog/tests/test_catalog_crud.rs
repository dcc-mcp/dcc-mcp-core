use super::fixtures::{make_catalog_with_dispatcher, make_test_catalog, make_test_skill};
use super::*;
use dcc_mcp_actions::dispatcher::{DispatchError, with_thread_affinity};
use dcc_mcp_models::{SkillGroup, ThreadAffinity, ToolDeclaration};

fn write_skill_dir(root: &std::path::Path, name: &str, dcc: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(crate::constants::SKILL_METADATA_FILE),
        format!(
            "---\nname: {name}\ndescription: test skill\nmetadata:\n  dcc-mcp:\n    dcc: {dcc}\n---\n# {name}\n"
        ),
    )
    .unwrap();
}

#[test]
fn test_catalog_new_is_empty() {
    let catalog = make_test_catalog();
    assert!(catalog.is_empty());
    assert_eq!(catalog.len(), 0);
    assert_eq!(catalog.loaded_count(), 0);
}

#[test]
fn test_add_skill() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill(
        "modeling-bevel",
        "maya",
        &["bevel", "chamfer"],
    ));
    assert_eq!(catalog.len(), 1);
    assert!(!catalog.is_loaded("modeling-bevel"));
}

#[test]
fn test_add_skill_marks_missing_soft_dependencies_pending() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("shot-publish", "maya", &["publish"]);
    skill.depends = vec!["maya-dev".to_string()];
    catalog.add_skill(skill);

    let pending = catalog.list_skills(Some("pending_deps"));
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].name, "shot-publish");
    assert_eq!(pending[0].status, "pending_deps");
    assert_eq!(pending[0].missing_dependencies, vec!["maya-dev"]);

    let search = catalog.search_skills(Some("publish"), &[], Some("maya"), None, None);
    assert_eq!(search.len(), 1, "pending skills must remain discoverable");
    assert_eq!(search[0].status, "pending_deps");
}

#[test]
fn test_add_dependency_clears_pending_dependency_state() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("shot-publish", "maya", &["publish"]);
    skill.depends = vec!["maya-dev".to_string()];
    catalog.add_skill(skill);
    assert_eq!(catalog.list_skills(Some("pending_deps")).len(), 1);

    catalog.add_skill(make_test_skill("maya-dev", "maya", &["inspect"]));

    assert!(catalog.list_skills(Some("pending_deps")).is_empty());
    let info = catalog.get_skill_info("shot-publish").unwrap();
    assert_eq!(info.state, "discovered");
    assert!(info.missing_dependencies.is_empty());
}

#[test]
fn test_rediscover_removes_missing_skill_and_registered_tools() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill_dir(tmp.path(), "fresh-skill", "maya");

    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("stale-skill", "maya", &["old_tool"]));
    catalog.load_skill("stale-skill").unwrap();
    assert!(catalog.is_loaded("stale-skill"));
    assert_eq!(catalog.registry().len(), 1);

    let paths = vec![tmp.path().to_string_lossy().to_string()];
    let changed = catalog.rediscover(Some(&paths), Some("maya"));

    assert_eq!(changed, 2, "expected one add and one remove");
    assert!(catalog.get_skill_info("fresh-skill").is_some());
    assert!(catalog.get_skill_info("stale-skill").is_none());
    assert!(!catalog.is_loaded("stale-skill"));
    assert_eq!(catalog.registry().len(), 0);
}

#[test]
fn test_load_skill_registers_tools() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill(
        "modeling-bevel",
        "maya",
        &["bevel", "chamfer"],
    ));

    let actions = catalog.load_skill("modeling-bevel").unwrap();
    assert_eq!(actions.len(), 2);
    assert!(actions.contains(&"modeling_bevel__bevel".to_string()));
    assert!(actions.contains(&"modeling_bevel__chamfer".to_string()));
    assert!(catalog.is_loaded("modeling-bevel"));
    assert_eq!(catalog.loaded_count(), 1);

    let registry = catalog.registry();
    assert_eq!(registry.len(), 2);
    assert!(registry.get_action("modeling_bevel__bevel", None).is_some());
}

#[test]
fn test_load_skill_auto_loads_discovered_dependencies_first() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-dev", "maya", &["inspect"]));
    let mut skill = make_test_skill("shot-publish", "maya", &["publish"]);
    skill.depends = vec!["maya-dev".to_string()];
    catalog.add_skill(skill);

    let actions = catalog.load_skill("shot-publish").unwrap();

    assert_eq!(actions, vec!["shot_publish__publish".to_string()]);
    assert!(catalog.is_loaded("maya-dev"));
    assert!(catalog.is_loaded("shot-publish"));
    assert!(
        catalog
            .registry()
            .get_action("maya_dev__inspect", None)
            .is_some(),
        "dependency tools should be registered before the dependent skill"
    );
}

#[test]
fn test_load_skill_missing_dependency_error_is_actionable() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("shot-publish", "maya", &["publish"]);
    skill.depends = vec!["maya-dev".to_string()];
    catalog.add_skill(skill);

    let err = catalog
        .load_skill("shot-publish")
        .expect_err("missing dependency should block activation");

    assert!(err.contains("pending dependencies"), "{err}");
    assert!(err.contains("maya-dev"), "{err}");
    assert!(err.contains("retry load_skill('shot-publish')"), "{err}");
}

#[test]
fn test_load_skill_with_action_meta_skill_name() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("my-skill", "maya", &["tool1"]));

    catalog.load_skill("my-skill").unwrap();
    let meta = catalog
        .registry()
        .get_action("my_skill__tool1", None)
        .unwrap();
    assert_eq!(meta.skill_name, Some("my-skill".to_string()));
}

#[test]
fn test_load_skill_activates_declared_groups_by_default() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("maya-scene", "maya", &[]);
    skill.groups = vec![SkillGroup {
        name: "scene-management".to_string(),
        default_active: false,
        ..Default::default()
    }];
    skill.tools = vec![ToolDeclaration {
        name: "new_scene".to_string(),
        group: "scene-management".to_string(),
        ..Default::default()
    }];
    catalog.add_skill(skill);

    catalog.load_skill("maya-scene").unwrap();

    let meta = catalog
        .registry()
        .get_action("maya_scene__new_scene", None)
        .expect("grouped action registered");
    assert!(
        meta.enabled,
        "load_skill should make grouped actions callable"
    );
    assert!(
        catalog
            .active_groups()
            .contains(&"scene-management".to_string()),
        "declared group should be active after default load"
    );
}

#[test]
fn test_load_skill_can_leave_groups_inactive_when_requested() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("maya-scene", "maya", &[]);
    skill.groups = vec![SkillGroup {
        name: "scene-management".to_string(),
        default_active: false,
        ..Default::default()
    }];
    skill.tools = vec![ToolDeclaration {
        name: "new_scene".to_string(),
        group: "scene-management".to_string(),
        ..Default::default()
    }];
    catalog.add_skill(skill);

    catalog
        .load_skill_with_options("maya-scene", false)
        .expect("skill loads with inactive groups");

    let meta = catalog
        .registry()
        .get_action("maya_scene__new_scene", None)
        .expect("grouped action registered");
    assert!(
        !meta.enabled,
        "activate_groups=false should preserve laziness"
    );
}

#[test]
fn test_load_skill_propagates_thread_affinity_enforcement() {
    let (catalog, dispatcher) = make_catalog_with_dispatcher();
    catalog.set_in_process_executor(|_, _, _| Ok(serde_json::json!({"ok": true})));
    let mut skill = make_test_skill("main-thread", "maya", &[]);
    skill.tools = vec![ToolDeclaration {
        name: "execute_python".to_string(),
        source_file: "scripts/execute_python.py".to_string(),
        thread_affinity: ThreadAffinity::Main,
        enforce_thread_affinity: true,
        ..Default::default()
    }];
    catalog.add_skill(skill);
    catalog.load_skill("main-thread").unwrap();

    let err = dispatcher
        .dispatch("main_thread__execute_python", serde_json::json!({}))
        .expect_err("worker-thread dispatch must be rejected");
    assert!(matches!(err, DispatchError::ThreadAffinityViolation { .. }));

    let ok = with_thread_affinity(ThreadAffinity::Main, || {
        dispatcher.dispatch("main_thread__execute_python", serde_json::json!({}))
    })
    .expect("main-thread dispatch should pass enforcement");
    assert_eq!(ok.output, serde_json::json!({"ok": true}));
}

#[test]
fn test_load_main_affinity_script_requires_in_process_executor() {
    let (catalog, _dispatcher) = make_catalog_with_dispatcher();
    let mut skill = make_test_skill("main-thread", "maya", &[]);
    skill.tools = vec![ToolDeclaration {
        name: "execute_python".to_string(),
        source_file: "scripts/execute_python.py".to_string(),
        thread_affinity: dcc_mcp_models::ThreadAffinity::Main,
        ..Default::default()
    }];
    catalog.add_skill(skill);

    let err = catalog
        .load_skill("main-thread")
        .expect_err("main-affined script tools need an in-process executor");
    assert!(
        err.contains("requires thread_affinity='main'"),
        "error should explain the main-thread requirement: {err}"
    );
    assert!(
        err.contains("set_in_process_executor()"),
        "error should tell DCC adapters how to fix the setup: {err}"
    );
    assert!(
        catalog
            .registry()
            .get_action("main_thread__execute_python", None)
            .is_none(),
        "failed loads must not leave partially registered actions"
    );
}

#[test]
fn test_unload_skill_removes_tools() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("modeling-bevel", "maya", &["bevel"]));
    catalog.load_skill("modeling-bevel").unwrap();
    assert_eq!(catalog.registry().len(), 1);

    let removed = catalog.unload_skill("modeling-bevel").unwrap();
    assert_eq!(removed, 1);
    assert!(!catalog.is_loaded("modeling-bevel"));
    assert_eq!(catalog.registry().len(), 0);
}

#[test]
fn test_load_nonexistent_skill_fails() {
    let catalog = make_test_catalog();
    let result = catalog.load_skill("no-such-skill");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn test_unload_not_loaded_skill_fails() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("test", "maya", &[]));
    let result = catalog.unload_skill("test");
    assert!(result.is_err());
}

#[test]
fn test_load_skill_propagates_next_tools_and_drops_invalid() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("modeling", "maya", &[]);
    skill.tools = vec![ToolDeclaration {
        name: "bevel".to_string(),
        next_tools: dcc_mcp_models::NextTools {
            on_success: vec![
                "maya_geometry__assign_material".to_string(),
                "bad/name".to_string(),
            ],
            on_failure: vec!["diagnostics__screenshot".to_string()],
        },
        ..Default::default()
    }];
    catalog.add_skill(skill);

    catalog.load_skill("modeling").unwrap();
    let meta = catalog
        .registry()
        .get_action("modeling__bevel", None)
        .expect("action registered");
    assert_eq!(
        meta.next_tools.on_success,
        vec!["maya_geometry__assign_material".to_string()],
        "invalid entries must be filtered",
    );
    assert_eq!(
        meta.next_tools.on_failure,
        vec!["diagnostics__screenshot".to_string()],
    );
}

#[test]
fn test_load_skill_idempotent() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("test", "maya", &["tool1"]));

    let actions1 = catalog.load_skill("test").unwrap();
    let actions2 = catalog.load_skill("test").unwrap();
    assert_eq!(actions1, actions2);
    assert_eq!(catalog.registry().len(), 1);
}

#[test]
fn test_search_skills_by_query() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("modeling-bevel", "maya", &[]));
    catalog.add_skill(make_test_skill("rendering-batch", "blender", &[]));

    let results = catalog.search_skills(Some("bevel"), &[], None, None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "modeling-bevel");
}

#[test]
fn test_search_skills_by_dcc() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("skill-a", "maya", &[]));
    catalog.add_skill(make_test_skill("skill-b", "blender", &[]));

    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].dcc, "maya");
}

#[test]
fn test_search_skills_by_tags() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("tagged", "maya", &[]);
    skill.tags = vec!["modeling".to_string(), "polygon".to_string()];
    catalog.add_skill(skill);
    catalog.add_skill(make_test_skill("untagged", "maya", &[]));

    let results = catalog.search_skills(None, &["modeling"], None, None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "tagged");
}

#[test]
fn test_list_skills_filter_by_status() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("loaded-skill", "maya", &["tool1"]));
    catalog.add_skill(make_test_skill("unloaded-skill", "maya", &[]));
    catalog.load_skill("loaded-skill").unwrap();

    let loaded = catalog.list_skills(Some("loaded"));
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "loaded-skill");
    assert!(loaded[0].loaded);

    let unloaded = catalog.list_skills(Some("unloaded"));
    assert_eq!(unloaded.len(), 1);
    assert_eq!(unloaded[0].name, "unloaded-skill");
    assert!(!unloaded[0].loaded);
}

#[test]
fn test_get_skill_info() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("test-skill", "maya", &["tool1", "tool2"]));

    let info = catalog.get_skill_info("test-skill").unwrap();
    assert_eq!(info.name, "test-skill");
    assert_eq!(info.tools.len(), 2);
    assert_eq!(info.state, "discovered");
}

#[test]
fn test_get_skill_info_includes_skill_markdown() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill_dir(tmp.path(), "review-skill", "maya");

    let catalog = make_test_catalog();
    let paths = vec![tmp.path().to_string_lossy().to_string()];
    assert_eq!(catalog.rediscover(Some(&paths), Some("maya")), 1);

    let info = catalog.get_skill_info("review-skill").unwrap();
    assert!(
        info.skill_md_path
            .unwrap()
            .ends_with(crate::constants::SKILL_METADATA_FILE)
    );
    assert!(info.markdown.unwrap().contains("# review-skill"));
}

#[test]
fn test_get_skill_info_nonexistent() {
    let catalog = make_test_catalog();
    assert!(catalog.get_skill_info("nope").is_none());
}

#[test]
fn test_remove_skill() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("removable", "maya", &["tool1"]));
    catalog.load_skill("removable").unwrap();

    assert!(catalog.remove_skill("removable"));
    assert_eq!(catalog.len(), 0);
    assert_eq!(catalog.registry().len(), 0);
}

#[test]
fn test_clear() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("a", "maya", &["t1"]));
    catalog.add_skill(make_test_skill("b", "maya", &["t2"]));
    catalog.load_skill("a").unwrap();

    catalog.clear();
    assert!(catalog.is_empty());
    assert_eq!(catalog.registry().len(), 0);
}

#[test]
fn test_skill_with_scripts_no_tools() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("scripted", "maya", &[]);
    skill.scripts = vec!["/path/to/run.py".to_string()];
    catalog.add_skill(skill);

    let actions = catalog.load_skill("scripted").unwrap();
    assert_eq!(actions.len(), 1);
    assert!(actions[0].contains("scripted__run"));
}

#[test]
fn test_add_skill_does_not_overwrite_loaded() {
    let catalog = make_test_catalog();
    let skill = make_test_skill("keep", "maya", &["tool1"]);
    catalog.add_skill(skill);
    catalog.load_skill("keep").unwrap();

    let updated = SkillMetadata {
        name: "keep".to_string(),
        description: "Updated description".to_string(),
        tools: vec![ToolDeclaration {
            name: "tool1".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    catalog.add_skill(updated);

    assert!(catalog.is_loaded("keep"));
    let info = catalog.get_skill_info("keep").unwrap();
    assert_eq!(info.description, "Test skill: keep");
}
