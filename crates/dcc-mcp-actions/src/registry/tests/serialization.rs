//! Serialization / serde round-trip tests for ActionMeta.

use super::*;

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
        required_capabilities: vec!["scene".into(), "render".into()],
        execution: dcc_mcp_models::ExecutionMode::Async,
        timeout_hint_secs: Some(900),
        thread_affinity: dcc_mcp_models::ThreadAffinity::Any,
        annotations: dcc_mcp_models::ToolAnnotations::default(),
        next_tools: dcc_mcp_models::NextTools::default(),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: ActionMeta = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, back);
}

#[test]
fn test_action_meta_execution_defaults_to_sync() {
    let meta = ActionMeta::default();
    assert_eq!(meta.execution, dcc_mcp_models::ExecutionMode::Sync);
    assert_eq!(meta.timeout_hint_secs, None);
}

#[test]
fn test_action_meta_execution_and_timeout_serde() {
    // Issue #317 — new fields must round-trip and be recognised on input.
    let json = r#"{
        "name": "render",
        "description": "Render",
        "execution": "async",
        "timeout_hint_secs": 600
    }"#;
    let meta: ActionMeta = serde_json::from_str(json).unwrap();
    assert_eq!(meta.execution, dcc_mcp_models::ExecutionMode::Async);
    assert_eq!(meta.timeout_hint_secs, Some(600));
}

#[test]
fn test_action_meta_default_serialization() {
    let meta = ActionMeta::default();
    let json = serde_json::to_string(&meta).unwrap();
    let back: ActionMeta = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, back);
}
