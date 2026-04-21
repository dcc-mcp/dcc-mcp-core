//! Unit tests for the workflow skeleton.

use dcc_mcp_actions::ActionRegistry;
use dcc_mcp_naming::validate_tool_name;

use crate::catalog::{METADATA_KEY_WORKFLOWS, WorkflowCatalog};
use crate::error::ValidationError;
use crate::spec::{StepKind, WorkflowSpec, WorkflowStatus};
use crate::tools::{self, register_builtin_workflow_tools};

// ── spec: parse + validate ──────────────────────────────────────────────

const VALID_YAML: &str = r#"
name: vendor_intake
description: "Import vendor Maya files, QC, export FBX, push to Unreal."
inputs:
  date: { type: string, format: date }
steps:
  - id: list
    tool: vendor_intake__list_sftp
    args: { date: "{{inputs.date}}" }
  - id: per_file
    kind: foreach
    items: "$.list.files"
    as: file
    steps:
      - id: import
        tool: maya__import_scene
      - id: qc
        tool: maya_qc__run_all
      - id: gate
        kind: branch
        on: "$.qc.passed"
        then:
          - id: export
            tool: maya__export_fbx
          - id: handoff
            kind: tool_remote
            dcc: unreal
            tool: unreal__ingest_fbx
"#;

#[test]
fn parses_valid_spec_and_validates() {
    let spec = WorkflowSpec::from_yaml(VALID_YAML).unwrap();
    assert_eq!(spec.name, "vendor_intake");
    assert_eq!(spec.steps.len(), 2);
    assert!(spec.validate().is_ok());
}

#[test]
fn rejects_empty_steps() {
    let yaml = "name: empty\nsteps: []\n";
    let spec = WorkflowSpec::from_yaml(yaml).unwrap();
    assert!(matches!(spec.validate(), Err(ValidationError::NoSteps)));
}

#[test]
fn rejects_duplicate_step_ids() {
    let yaml = r#"
name: dup
steps:
  - id: a
    tool: foo
  - id: a
    tool: bar
"#;
    let spec = WorkflowSpec::from_yaml(yaml).unwrap();
    assert!(matches!(
        spec.validate(),
        Err(ValidationError::DuplicateStepId(ref id)) if id == "a"
    ));
}

#[test]
fn rejects_bad_tool_name() {
    let yaml = r#"
name: bad
steps:
  - id: a
    tool: "bad/tool"
"#;
    let spec = WorkflowSpec::from_yaml(yaml).unwrap();
    assert!(matches!(
        spec.validate(),
        Err(ValidationError::InvalidToolName { .. })
    ));
}

#[test]
fn rejects_bad_jsonpath_in_branch() {
    let yaml = r#"
name: bad_path
steps:
  - id: gate
    kind: branch
    on: "not a jsonpath"
    then:
      - id: inner
        tool: ok_tool
"#;
    let spec = WorkflowSpec::from_yaml(yaml).unwrap();
    assert!(matches!(
        spec.validate(),
        Err(ValidationError::InvalidJsonPath { .. })
    ));
}

#[test]
fn yaml_parse_error_surfaces_as_yaml_variant() {
    let yaml = "name: broken\nsteps: [not closed\n";
    let err = WorkflowSpec::from_yaml(yaml).unwrap_err();
    assert!(matches!(err, crate::WorkflowError::Yaml(_)));
}

#[test]
fn status_is_terminal() {
    assert!(!WorkflowStatus::Pending.is_terminal());
    assert!(!WorkflowStatus::Running.is_terminal());
    assert!(WorkflowStatus::Completed.is_terminal());
    assert!(WorkflowStatus::Failed.is_terminal());
    assert!(WorkflowStatus::Cancelled.is_terminal());
    assert!(WorkflowStatus::Interrupted.is_terminal());
}

#[test]
fn step_shorthand_tool_becomes_tool_kind() {
    // No explicit `kind:`, but `tool:` is present → StepKind::Tool.
    let yaml = r#"
name: shorthand
steps:
  - id: a
    tool: some_tool
"#;
    let spec = WorkflowSpec::from_yaml(yaml).unwrap();
    assert!(matches!(spec.steps[0].kind, StepKind::Tool { .. }));
}

// ── job placeholder ─────────────────────────────────────────────────────

#[test]
fn workflow_job_start_returns_not_implemented() {
    let spec = WorkflowSpec::from_yaml(VALID_YAML).unwrap();
    let mut job = crate::WorkflowJob::pending(spec);
    let err = job.start().unwrap_err();
    assert!(matches!(err, crate::WorkflowError::NotImplemented(_)));
}

// ── tools ───────────────────────────────────────────────────────────────

#[test]
fn builtin_tool_names_pass_sep986_validation() {
    for name in [
        tools::names::RUN,
        tools::names::GET_STATUS,
        tools::names::CANCEL,
        tools::names::LOOKUP,
    ] {
        validate_tool_name(name).unwrap_or_else(|e| panic!("{name:?} rejected by SEP-986: {e}"));
    }
}

#[test]
fn register_builtin_tools_populates_registry() {
    let reg = ActionRegistry::new();
    register_builtin_workflow_tools(&reg);
    assert!(reg.get_action(tools::names::RUN, None).is_some());
    assert!(reg.get_action(tools::names::GET_STATUS, None).is_some());
    assert!(reg.get_action(tools::names::CANCEL, None).is_some());
    assert!(reg.get_action(tools::names::LOOKUP, None).is_some());
}

#[test]
fn builtin_tool_metadata_has_annotations() {
    let reg = ActionRegistry::new();
    register_builtin_workflow_tools(&reg);
    let run = reg.get_action(tools::names::RUN, None).unwrap();
    assert_eq!(run.annotations.destructive_hint, Some(true));
    let get = reg.get_action(tools::names::GET_STATUS, None).unwrap();
    assert_eq!(get.annotations.read_only_hint, Some(true));
    let cancel = reg.get_action(tools::names::CANCEL, None).unwrap();
    assert_eq!(cancel.annotations.destructive_hint, Some(true));
    let lookup = reg.get_action(tools::names::LOOKUP, None).unwrap();
    assert_eq!(lookup.annotations.read_only_hint, Some(true));
}

#[tokio::test]
async fn register_workflow_handlers_wires_run_and_cancel() {
    use std::sync::Arc;

    use dcc_mcp_actions::dispatcher::ActionDispatcher;
    use serde_json::json;

    use crate::callers::test_support::MockToolCaller;
    use crate::executor::WorkflowExecutor;
    use crate::host::WorkflowHost;
    use crate::tools::register_workflow_handlers;

    let caller = Arc::new(MockToolCaller::new());
    caller.add("scene.echo", Ok);
    let executor = WorkflowExecutor::builder().tool_caller(caller).build();
    let host = WorkflowHost::new(executor);

    let reg = ActionRegistry::new();
    register_builtin_workflow_tools(&reg);
    let dispatcher = ActionDispatcher::new(reg);
    register_workflow_handlers(&dispatcher, &host);

    let yaml = "name: t\nsteps:\n  - id: s1\n    tool: scene.echo\n    args: {x: 1}\n";
    let out = dispatcher
        .dispatch(tools::names::RUN, json!({"spec": yaml, "inputs": {}}))
        .unwrap()
        .output;
    let wid = out["workflow_id"].as_str().unwrap();
    assert_eq!(out["status"], "pending");

    // Cancel via dispatcher.
    let cancelled = dispatcher
        .dispatch(tools::names::CANCEL, json!({"workflow_id": wid}))
        .unwrap()
        .output;
    assert_eq!(cancelled["cancelled"], true);
}

#[test]
fn not_implemented_result_has_stable_shape() {
    let r = tools::not_implemented_result("workflows.run");
    assert_eq!(r["success"], serde_json::Value::Bool(false));
    assert_eq!(r["error"], "not_implemented");
    assert_eq!(r["issue"], "#348");
    assert!(r["message"].as_str().unwrap().contains("pending"));
}

// ── catalog glob reader ─────────────────────────────────────────────────

#[test]
fn catalog_reads_glob_from_skill_metadata() {
    use std::fs;

    use dcc_mcp_models::SkillMetadata;

    let tmp = tempfile::tempdir().unwrap();
    let skill_root = tmp.path();
    let wf_dir = skill_root.join("workflows");
    fs::create_dir_all(&wf_dir).unwrap();
    fs::write(
        wf_dir.join("vendor_intake.workflow.yaml"),
        "name: vendor_intake\ndescription: Intake vendor files\n",
    )
    .unwrap();
    fs::write(
        wf_dir.join("nightly.workflow.yaml"),
        "name: nightly_cleanup\ndescription: Clean staging each night\n",
    )
    .unwrap();

    let mut meta = SkillMetadata {
        name: "vendor-intake".to_string(),
        ..Default::default()
    };
    meta.metadata = serde_json::json!({
        METADATA_KEY_WORKFLOWS: "workflows/*.workflow.yaml",
    });

    let cat = WorkflowCatalog::from_skill(&meta, skill_root).unwrap();
    let names: Vec<&str> = cat.entries().iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"vendor_intake"), "got: {names:?}");
    assert!(names.contains(&"nightly_cleanup"), "got: {names:?}");

    let hit = cat.search("nightly");
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].name, "nightly_cleanup");
}

#[test]
fn catalog_handles_missing_metadata_key_gracefully() {
    use dcc_mcp_models::SkillMetadata;

    let tmp = tempfile::tempdir().unwrap();
    let meta = SkillMetadata {
        name: "no-workflows".to_string(),
        ..Default::default()
    };
    let cat = WorkflowCatalog::from_skill(&meta, tmp.path()).unwrap();
    assert!(cat.entries().is_empty());
}

#[test]
fn catalog_comma_separated_globs() {
    use std::fs;

    use dcc_mcp_models::SkillMetadata;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a/one.yaml"), "name: one\n").unwrap();
    fs::write(root.join("b/two.yaml"), "name: two\n").unwrap();

    let mut meta = SkillMetadata {
        name: "multi".to_string(),
        ..Default::default()
    };
    meta.metadata = serde_json::json!({
        METADATA_KEY_WORKFLOWS: "a/*.yaml, b/*.yaml",
    });

    let cat = WorkflowCatalog::from_skill(&meta, root).unwrap();
    assert_eq!(cat.entries().len(), 2);
}
