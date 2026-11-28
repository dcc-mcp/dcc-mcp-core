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
    // Verify source_file propagated to ToolMeta
    let meta = dispatcher
        .registry()
        .get_action("explicit_skill__do_thing", None)
        .unwrap();
    assert_eq!(meta.source_file, Some("/fake/do_thing.py".to_string()));
}

/// Regression for the inline/declarative builtin ownership race:
/// `register_diagnostic_mcp_tools` installs `dcc_diagnostics__*` with an
/// inline handler and no `source_file`. Loading a YAML skill that declares
/// the same action names must borrow those registry entries without
/// replacing the handler (GH-1696). Unloading the skill must not remove the
/// borrowed inline handler, otherwise the action stays visible in
/// `tools/list` but is no longer callable, and reload cannot restore it.
#[test]
fn test_unload_reload_preserves_borrowed_inline_builtin_handler() {
    let (catalog, dispatcher) = make_catalog_with_dispatcher();

    // Simulate an inline builtin: registry metadata owned by nobody (no
    // skill_name), with a live handler in the shared dispatcher.
    dispatcher.registry().register_action(ToolMeta {
        name: "dcc_diagnostics__audit_log".to_string(),
        description: "inline builtin".to_string(),
        source_file: Some(String::new()),
        skill_name: None,
        ..Default::default()
    });
    dispatcher.register_handler("dcc_diagnostics__audit_log", |_| {
        Ok(serde_json::json!({"owner": "builtin"}))
    });

    let mut skill = make_test_skill("dcc-diagnostics", "python", &[]);
    skill.tools = vec![dcc_mcp_models::ToolDeclaration {
        name: "dcc_diagnostics__audit_log".to_string(),
        source_file: "scripts/audit_log.py".to_string(),
        ..Default::default()
    }];
    catalog.add_skill(skill);

    // Load borrows the builtin entry; the inline handler survives.
    let loaded = catalog.load_skill("dcc-diagnostics").unwrap();
    assert_eq!(loaded, vec!["dcc_diagnostics__audit_log".to_string()]);
    assert!(dispatcher.has_handler("dcc_diagnostics__audit_log"));

    // Unload must not remove the borrowed handler, and the builtin registry
    // entry must stay intact (skill_name is still None).
    let removed = catalog.unload_skill("dcc-diagnostics").unwrap();
    assert_eq!(removed, 0, "the borrowed builtin entry is not owned by the skill");
    assert!(dispatcher.has_handler("dcc_diagnostics__audit_log"));
    let meta = dispatcher
        .registry()
        .get_action("dcc_diagnostics__audit_log", None)
        .unwrap();
    assert!(meta.skill_name.is_none());
    assert_eq!(meta.source_file, Some(String::new()));

    // An arbitrary number of reloads keeps the builtin callable and its
    // ownership unchanged — no visible-but-not-callable residue.
    for _ in 0..2 {
        catalog.load_skill("dcc-diagnostics").unwrap();
        assert!(dispatcher.has_handler("dcc_diagnostics__audit_log"));
        catalog.unload_skill("dcc-diagnostics").unwrap();
        assert!(dispatcher.has_handler("dcc_diagnostics__audit_log"));
    }

    let output = dispatcher
        .dispatch("dcc_diagnostics__audit_log", serde_json::json!({}), None)
        .unwrap();
    assert_eq!(output.output, serde_json::json!({"owner": "builtin"}));
}

/// A skill that owns its actions (no pre-existing registry entry) still has
/// its handler removed on unload and restored on reload.
#[test]
fn test_unload_reload_owned_skill_handler_lifecycle() {
    let (catalog, dispatcher) = make_catalog_with_dispatcher();

    let mut skill = make_test_skill("owned-skill", "python", &[]);
    skill.scripts = vec!["/fake/run.py".to_string()];
    catalog.add_skill(skill);

    catalog.load_skill("owned-skill").unwrap();
    assert!(dispatcher.has_handler("owned_skill__run"));
    catalog.unload_skill("owned-skill").unwrap();
    assert!(!dispatcher.has_handler("owned_skill__run"));
    assert!(dispatcher
        .registry()
        .get_action("owned_skill__run", None)
        .is_none());

    catalog.load_skill("owned-skill").unwrap();
    assert!(dispatcher.has_handler("owned_skill__run"));
    assert!(dispatcher
        .registry()
        .get_action("owned_skill__run", None)
        .is_some());
}
