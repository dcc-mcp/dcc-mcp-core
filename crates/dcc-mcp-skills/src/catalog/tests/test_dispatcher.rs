//! Tests for Skills-First dispatcher integration.
use super::fixtures::{make_catalog_with_dispatcher, make_test_skill};
use super::*;

#[test]
fn test_load_skill_registers_dispatcher_handler_for_scripts() {
    let (catalog, dispatcher) = make_catalog_with_dispatcher();

    // Skill with no tool declarations — script-only path
    let mut skill = make_test_skill("echo-skill", "python", &[]);
    skill.scripts = vec!["/fake/echo.py".to_string()];
    catalog.add_skill(skill);

    let actions = catalog.load_skill("echo-skill").unwrap();
    assert_eq!(actions.len(), 1);
    // Handler auto-registered in dispatcher
    assert!(dispatcher.has_handler("echo_skill__echo"));
}

#[test]
fn test_unload_skill_removes_dispatcher_handlers() {
    let (catalog, dispatcher) = make_catalog_with_dispatcher();

    let mut skill = make_test_skill("rm-skill", "python", &[]);
    skill.scripts = vec!["/fake/run.py".to_string()];
    catalog.add_skill(skill);

    catalog.load_skill("rm-skill").unwrap();
    assert!(dispatcher.has_handler("rm_skill__run"));

    catalog.unload_skill("rm-skill").unwrap();
    assert!(!dispatcher.has_handler("rm_skill__run"));
}

#[test]
fn test_load_skill_with_tool_decl_and_source_file() {
    let (catalog, dispatcher) = make_catalog_with_dispatcher();

    let skill = SkillMetadata {
        name: "explicit-skill".to_string(),
        description: "Explicit source file".to_string(),
        tools: vec![dcc_mcp_models::ToolDeclaration {
            name: "do_thing".to_string(),
            source_file: "/fake/do_thing.py".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    catalog.add_skill(skill);

    let actions = catalog.load_skill("explicit-skill").unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0], "explicit_skill__do_thing");
    assert!(dispatcher.has_handler("explicit_skill__do_thing"));
    // Verify source_file propagated to ActionMeta
    let meta = dispatcher
        .registry()
        .get_action("explicit_skill__do_thing", None)
        .unwrap();
    assert_eq!(meta.source_file, Some("/fake/do_thing.py".to_string()));
}
