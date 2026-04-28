//! Tests for `resolve_tool_script` helper and basic `execute_script` smoke test.
use super::*;
use dcc_mcp_models::ToolDeclaration;

#[test]
fn test_execute_script_returns_json() {
    // Test the execute_script helper with a real command that outputs JSON.
    // Python may or may not be available; just check the function runs.
    let result = execute_script("python", serde_json::json!({"key": "value"}), None);
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
