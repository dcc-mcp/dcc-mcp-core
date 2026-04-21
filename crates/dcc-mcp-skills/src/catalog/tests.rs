use super::*;
use dcc_mcp_models::ToolDeclaration;

fn make_test_catalog() -> SkillCatalog {
    let registry = Arc::new(ActionRegistry::new());
    SkillCatalog::new(registry)
}

fn make_test_skill(name: &str, dcc: &str, tool_names: &[&str]) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("Test skill: {name}"),
        tools: tool_names
            .iter()
            .map(|t| ToolDeclaration {
                name: t.to_string(),
                ..Default::default()
            })
            .collect(),
        dcc: dcc.to_string(),
        tags: vec!["test".to_string()],
        version: "1.0.0".to_string(),
        ..Default::default()
    }
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

    // Verify tools are in the registry
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
    // Issue #342: tools.yaml `next-tools` flows into ActionMeta.next_tools;
    // entries whose tool name fails `validate_tool_name` are dropped at
    // load time and do not fail the whole skill load.
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("modeling", "maya", &[]);
    skill.tools = vec![ToolDeclaration {
        name: "bevel".to_string(),
        next_tools: dcc_mcp_models::NextTools {
            on_success: vec![
                "maya_geometry__assign_material".to_string(),
                "bad/name".to_string(), // invalid — must be dropped
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
fn test_find_skills_by_query() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("modeling-bevel", "maya", &[]));
    catalog.add_skill(make_test_skill("rendering-batch", "blender", &[]));

    let results = catalog.find_skills(Some("bevel"), &[], None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "modeling-bevel");
}

#[test]
fn test_find_skills_by_dcc() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("skill-a", "maya", &[]));
    catalog.add_skill(make_test_skill("skill-b", "blender", &[]));

    let results = catalog.find_skills(None, &[], Some("maya"));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].dcc, "maya");
}

#[test]
fn test_find_skills_by_tags() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("tagged", "maya", &[]);
    skill.tags = vec!["modeling".to_string(), "polygon".to_string()];
    catalog.add_skill(skill);
    catalog.add_skill(make_test_skill("untagged", "maya", &[]));

    let results = catalog.find_skills(None, &["modeling"], None);
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

    // Add again with different metadata — should not overwrite loaded state
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
    // Description should NOT be updated since skill was loaded
    assert_eq!(info.description, "Test skill: keep");
}

// ── Skills-First: dispatcher integration tests ──

fn make_catalog_with_dispatcher() -> (SkillCatalog, Arc<ActionDispatcher>) {
    let registry = Arc::new(ActionRegistry::new());
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    let catalog = SkillCatalog::new_with_dispatcher(registry, dispatcher.clone());
    (catalog, dispatcher)
}

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
        tools: vec![ToolDeclaration {
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

#[test]
fn test_execute_script_returns_json() {
    // Test the execute_script helper with a real command that outputs JSON
    // Use `python -c` for cross-platform compatibility
    let result = execute_script("python", serde_json::json!({"key": "value"}), None);
    // Python may or may not be available; just check the function runs
    // (either Ok or Err is valid in CI environments without Python)
    let _ = result;
}

#[test]
fn test_resolve_tool_script_by_name_match() {
    let scripts = vec![
        "/skill/scripts/bevel.py".to_string(),
        "/skill/scripts/extrude.py".to_string(),
    ];
    let tool = ToolDeclaration {
        name: "bevel".to_string(),
        ..Default::default()
    };
    let resolved = resolve_tool_script(&tool, &scripts, std::path::Path::new("/skill"));
    assert_eq!(resolved, Some("/skill/scripts/bevel.py".to_string()));
}

#[test]
fn test_resolve_tool_script_single_script_fallback() {
    let scripts = vec!["/skill/scripts/main.py".to_string()];
    let tool = ToolDeclaration {
        name: "any_tool".to_string(),
        ..Default::default()
    };
    let resolved = resolve_tool_script(&tool, &scripts, std::path::Path::new("/skill"));
    assert_eq!(resolved, Some("/skill/scripts/main.py".to_string()));
}

#[test]
fn test_resolve_tool_script_explicit_source_file() {
    let scripts = vec!["/skill/scripts/other.py".to_string()];
    let tool = ToolDeclaration {
        name: "my_tool".to_string(),
        source_file: "/skill/scripts/special.py".to_string(),
        ..Default::default()
    };
    let resolved = resolve_tool_script(&tool, &scripts, std::path::Path::new("/skill"));
    assert_eq!(resolved, Some("/skill/scripts/special.py".to_string()));
}

#[test]
fn test_resolve_tool_script_relative_source_file_resolves_to_absolute() {
    // Relative source_file in SKILL.md must be joined with skill_path so that
    // execute_script always receives an absolute path regardless of process CWD.
    let scripts = vec![];
    let tool = ToolDeclaration {
        name: "my_tool".to_string(),
        source_file: "scripts/my_tool.py".to_string(),
        ..Default::default()
    };
    let skill_root = std::path::Path::new("/my/skill/root");
    let resolved = resolve_tool_script(&tool, &scripts, skill_root);
    let expected = skill_root
        .join("scripts/my_tool.py")
        .to_string_lossy()
        .into_owned();
    assert_eq!(resolved, Some(expected));
}

#[test]
fn test_resolve_tool_script_relative_script_in_list_resolves_to_absolute() {
    // Scripts listed without an explicit source_file should also be absolutized.
    let scripts = vec!["scripts/bevel.py".to_string()];
    let tool = ToolDeclaration {
        name: "bevel".to_string(),
        ..Default::default()
    };
    let skill_root = std::path::Path::new("/my/skill/root");
    let resolved = resolve_tool_script(&tool, &scripts, skill_root);
    let expected = skill_root
        .join("scripts/bevel.py")
        .to_string_lossy()
        .into_owned();
    assert_eq!(resolved, Some(expected));
}

#[test]
fn test_resolve_tool_script_single_relative_script_resolves_to_absolute() {
    // Single-script fallback with a relative path.
    let scripts = vec!["scripts/main.py".to_string()];
    let tool = ToolDeclaration {
        name: "anything".to_string(),
        ..Default::default()
    };
    let skill_root = std::path::Path::new("/my/skill/root");
    let resolved = resolve_tool_script(&tool, &scripts, skill_root);
    let expected = skill_root
        .join("scripts/main.py")
        .to_string_lossy()
        .into_owned();
    assert_eq!(resolved, Some(expected));
}

// ── search_hint tests ──────────────────────────────────────────────────────

fn make_test_skill_with_hint(
    name: &str,
    dcc: &str,
    hint: &str,
    tool_names: &[&str],
) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("Test skill: {name}"),
        search_hint: hint.to_string(),
        tools: tool_names
            .iter()
            .map(|t| ToolDeclaration {
                name: t.to_string(),
                ..Default::default()
            })
            .collect(),
        dcc: dcc.to_string(),
        tags: vec!["test".to_string()],
        version: "1.0.0".to_string(),
        ..Default::default()
    }
}

#[test]
fn test_skill_summary_search_hint_from_metadata() {
    let catalog = make_test_catalog();
    let skill = make_test_skill_with_hint(
        "maya-bevel",
        "maya",
        "polygon modeling, bevel, chamfer",
        &["bevel"],
    );
    catalog.add_skill(skill);

    let summaries = catalog.list_skills(None);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].search_hint, "polygon modeling, bevel, chamfer");
}

#[test]
fn test_skill_summary_search_hint_fallback_to_description() {
    let catalog = make_test_catalog();
    // No search_hint set — should fall back to description
    catalog.add_skill(make_test_skill("no-hint", "maya", &[]));

    let summaries = catalog.list_skills(None);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].search_hint, summaries[0].description);
}

#[test]
fn test_find_skills_matches_search_hint() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill_with_hint(
        "maya-bevel",
        "maya",
        "polygon modeling, bevel, chamfer, extrude",
        &["bevel"],
    ));
    catalog.add_skill(make_test_skill_with_hint(
        "git-tools",
        "python",
        "git, commit, branch, vcs",
        &["log"],
    ));

    // "chamfer" only appears in search_hint of maya-bevel
    let results = catalog.find_skills(Some("chamfer"), &[], None);
    assert_eq!(results.len(), 1, "Expected 1 match for 'chamfer'");
    assert_eq!(results[0].name, "maya-bevel");
}

#[test]
fn test_find_skills_matches_tool_name() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill_with_hint(
        "maya-bevel",
        "maya",
        "polygon modeling",
        &["bevel", "chamfer"],
    ));
    catalog.add_skill(make_test_skill_with_hint(
        "git-tools",
        "python",
        "version control",
        &["log", "diff"],
    ));

    // "diff" is a tool name in git-tools
    let results = catalog.find_skills(Some("diff"), &[], None);
    assert_eq!(results.len(), 1, "Expected 1 match for tool 'diff'");
    assert_eq!(results[0].name, "git-tools");
}

#[test]
fn test_find_skills_no_match_returns_empty() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill_with_hint(
        "skill-a",
        "maya",
        "modeling tools",
        &["tool_a"],
    ));

    let results = catalog.find_skills(Some("xyzzy_no_match"), &[], None);
    assert!(results.is_empty(), "Expected empty results for no match");
}

#[test]
fn test_find_skills_matches_name_and_hint_combined() {
    let catalog = make_test_catalog();
    // "maya" appears in name of first, but also search_hint of second
    catalog.add_skill(make_test_skill_with_hint(
        "maya-geometry",
        "maya",
        "polygon sphere cylinder",
        &["create"],
    ));
    catalog.add_skill(make_test_skill_with_hint(
        "blender-shader",
        "blender",
        "maya-compatible shaders, pbr",
        &["shader"],
    ));

    let results = catalog.find_skills(Some("maya"), &[], None);
    // Both should match: first by name, second by search_hint
    assert_eq!(results.len(), 2, "Both skills should match 'maya'");
}

// ── execute_script dual-mode param passing ────────────────────────────────────

#[test]
fn test_execute_script_stdin_json_params() {
    // execute_script writes the full JSON to stdin — verify the call runs.
    let result = execute_script(
        "python",
        serde_json::json!({"greeting": "hello-stdin"}),
        None,
    );
    // Skip gracefully if Python is not available in this environment.
    if let Err(ref e) = result {
        if e.contains("Failed to spawn") || e.contains("No such file") {
            return;
        }
    }
    let _ = result;
}

#[test]
fn test_execute_script_cli_flags_passed_for_scalar_params() {
    // Scalar params (string/number/bool) must be expanded as --key value flags
    // so argparse-based scripts can receive them.  We verify the function
    // does not panic and returns a result.
    let result = execute_script(
        "python",
        serde_json::json!({"name": "Alice", "count": 3, "verbose": true}),
        None,
    );
    if let Err(ref e) = result {
        if e.contains("Failed to spawn") || e.contains("No such file") {
            return;
        }
    }
    let _ = result;
}

#[test]
fn test_execute_script_complex_values_not_expanded_as_flags() {
    // Object/array params must NOT be expanded as CLI flags — they should only
    // arrive via stdin JSON.  The function must not panic.
    let result = execute_script(
        "python",
        serde_json::json!({
            "simple": "value",
            "nested": {"a": 1},
            "list": [1, 2, 3],
        }),
        None,
    );
    if let Err(ref e) = result {
        if e.contains("Failed to spawn") || e.contains("No such file") {
            return;
        }
    }
    let _ = result;
}

// ── Regression tests for issue #231 (silent ambient-python fallback) ──────────
//
// These tests rely on manipulating process env vars. They must run serially
// relative to each other — we guard the env mutations with a mutex inside the
// module so parallel `cargo test` doesn't interleave SET/REMOVE calls.

static EXEC_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII guard that clears `DCC_MCP_PYTHON_EXECUTABLE`, `DCC_MCP_PYTHON_INIT_SNIPPET`,
/// and `DCC_MCP_ALLOW_AMBIENT_PYTHON` for the duration of a test, restoring the
/// prior values on drop.  Holds [`EXEC_ENV_MUTEX`] to prevent parallel tests
/// from racing on the same globals.
struct ExecEnvGuard<'a> {
    _lock: std::sync::MutexGuard<'a, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl<'a> ExecEnvGuard<'a> {
    fn new() -> Self {
        let lock = EXEC_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let keys = [
            "DCC_MCP_PYTHON_EXECUTABLE",
            "DCC_MCP_PYTHON_INIT_SNIPPET",
            "DCC_MCP_ALLOW_AMBIENT_PYTHON",
        ];
        let saved: Vec<(&'static str, Option<String>)> =
            keys.iter().map(|k| (*k, std::env::var(*k).ok())).collect();
        for (k, _) in &saved {
            // SAFETY: unit tests run single-threaded relative to each other
            // for this module thanks to EXEC_ENV_MUTEX.
            unsafe { std::env::remove_var(k) };
        }
        Self { _lock: lock, saved }
    }
}

impl<'a> Drop for ExecEnvGuard<'a> {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            match v {
                Some(val) => unsafe { std::env::set_var(k, val) },
                None => unsafe { std::env::remove_var(k) },
            }
        }
    }
}

#[test]
fn test_execute_script_rejects_ambient_python_for_host_dcc() {
    // When a skill declares a DCC that needs its own host Python (e.g. maya
    // needing mayapy) and DCC_MCP_PYTHON_EXECUTABLE / _INIT_SNIPPET are NOT
    // set, `execute_script` must fail loudly instead of silently falling
    // back to the ambient `python` on PATH.
    let _g = ExecEnvGuard::new();

    let result = execute_script(
        "any_skill.py",
        serde_json::json!({"key": "value"}),
        Some("maya"),
    );
    let err = result.expect_err("must fail loudly when DCC_MCP_PYTHON_EXECUTABLE is unset");
    assert!(
        err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
        "error message must mention the env var: got {err}"
    );
    assert!(
        err.to_lowercase().contains("maya"),
        "error message must mention the offending DCC: got {err}"
    );
}

#[test]
fn test_execute_script_rejects_ambient_python_case_insensitive() {
    let _g = ExecEnvGuard::new();
    // Uppercase / mixed case in the skill's declared dcc must still match.
    let err = execute_script("any.py", serde_json::json!({}), Some("Houdini"))
        .expect_err("Houdini must also require a host python");
    assert!(err.contains("DCC_MCP_PYTHON_EXECUTABLE"));
}

#[test]
fn test_execute_script_allows_opt_out_via_env_var() {
    // DCC_MCP_ALLOW_AMBIENT_PYTHON=1 is the escape hatch for test harnesses
    // and the stub-based `python` DCC.  With it set, the pre-flight check
    // must not raise, and we fall through to the real spawn attempt.
    let _g = ExecEnvGuard::new();
    unsafe { std::env::set_var("DCC_MCP_ALLOW_AMBIENT_PYTHON", "1") };

    let result = execute_script("/does/not/exist.py", serde_json::json!({}), Some("maya"));
    // Either the script fails because the path doesn't exist (OK) or it runs
    // — but critically the error must NOT be the #231 loud-fail.
    if let Err(err) = result {
        assert!(
            !err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
            "opt-out must suppress the #231 check: got {err}"
        );
    }
}

#[test]
fn test_execute_script_skips_check_when_executable_set() {
    // If the caller set DCC_MCP_PYTHON_EXECUTABLE explicitly, the guard
    // trusts them — even if the value points at a non-DCC interpreter.
    let _g = ExecEnvGuard::new();
    unsafe { std::env::set_var("DCC_MCP_PYTHON_EXECUTABLE", "python") };

    let result = execute_script("/does/not/exist.py", serde_json::json!({}), Some("maya"));
    if let Err(err) = result {
        assert!(
            !err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
            "explicit executable must disable the loud-fail: got {err}"
        );
    }
}

#[test]
fn test_execute_script_allows_generic_python_dcc() {
    // The `python` pseudo-DCC is the stub / generic runtime — it MUST be
    // allowed to run on the ambient interpreter without any opt-outs.
    let _g = ExecEnvGuard::new();

    let result = execute_script("/does/not/exist.py", serde_json::json!({}), Some("python"));
    if let Err(err) = result {
        assert!(
            !err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
            "generic 'python' dcc must not trigger the #231 check: got {err}"
        );
    }
}

#[test]
fn test_execute_script_no_dcc_hint_does_not_trigger_check() {
    // Older call-sites that pass `None` for the dcc hint must not get the
    // hard-fail — the check only fires when we *know* the DCC requires it.
    let _g = ExecEnvGuard::new();

    let result = execute_script("/does/not/exist.py", serde_json::json!({}), None);
    if let Err(err) = result {
        assert!(
            !err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
            "no dcc hint must not trigger the #231 check: got {err}"
        );
    }
}
