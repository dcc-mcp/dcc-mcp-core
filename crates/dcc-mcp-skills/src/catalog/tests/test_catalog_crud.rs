use super::fixtures::{make_test_catalog, make_test_skill};
use super::*;
use dcc_mcp_models::ToolDeclaration;

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
