//! Unit tests for [`crate::prompts`].

use super::*;
use dcc_mcp_jsonrpc::McpPromptContent;
use dcc_mcp_models::SkillMetadata;
use std::collections::HashMap;
use std::fs;

fn skill_metadata(
    name: &str,
    skill_path: &std::path::Path,
    metadata: serde_json::Value,
) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        skill_path: skill_path.to_string_lossy().to_string(),
        metadata,
        ..Default::default()
    }
}

fn argmap(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn render_basic() {
    let t = "hello {{name}}";
    let out = render_template(t, &argmap(&[("name", "world")])).unwrap();
    assert_eq!(out, "hello world");
}

#[test]
fn render_no_args() {
    let out = render_template("plain string", &HashMap::new()).unwrap();
    assert_eq!(out, "plain string");
}

#[test]
fn render_multi_arg() {
    let t = "{{a}}+{{b}}={{c}}";
    let out = render_template(t, &argmap(&[("a", "1"), ("b", "2"), ("c", "3")])).unwrap();
    assert_eq!(out, "1+2=3");
}

#[test]
fn render_missing_arg_errors() {
    let t = "hi {{who}}";
    let err = render_template(t, &HashMap::new()).unwrap_err();
    assert!(matches!(err, PromptError::MissingArg(ref s) if s == "who"));
}

#[test]
fn render_duplicate_arg() {
    let t = "{{x}}-{{x}}-{{x}}";
    let out = render_template(t, &argmap(&[("x", "42")])).unwrap();
    assert_eq!(out, "42-42-42");
}

#[test]
fn render_tolerates_whitespace_inside_braces() {
    let t = "v={{  k  }}";
    let out = render_template(t, &argmap(&[("k", "ok")])).unwrap();
    assert_eq!(out, "v=ok");
}

#[test]
fn render_preserves_unmatched_open_braces() {
    // Dangling `{{` with no closing `}}` is kept verbatim.
    let t = "weird {{ no close";
    let out = render_template(t, &HashMap::new()).unwrap();
    assert_eq!(out, "weird {{ no close");
}

#[test]
fn render_preserves_non_placeholder_brace_content() {
    // `{{ }}` containing invalid placeholder chars is left intact.
    let t = "code: {{ 1 + 1 }} done";
    let out = render_template(t, &HashMap::new()).unwrap();
    assert_eq!(out, "code: {{ 1 + 1 }} done");
}

#[test]
fn render_empty_template() {
    let out = render_template("", &HashMap::new()).unwrap();
    assert_eq!(out, "");
}

#[test]
fn registry_disabled_returns_empty() {
    let reg = PromptRegistry::new(false);
    let list = reg.list(|_| {});
    assert!(list.is_empty());
    let err = reg.get("foo", &HashMap::new(), |_| {}).unwrap_err();
    assert!(matches!(err, PromptError::NotFound(_)));
}

#[test]
fn promptsspec_from_yaml_parses_both_sections() {
    let yaml = r#"
prompts:
  - name: one
    description: first
    arguments:
      - name: x
        required: true
    template: "x is {{x}}"
workflows:
  - file: wf/bake.yaml
    prompt_name: bake_summary
"#;
    let spec = PromptsSpec::from_yaml(yaml).unwrap();
    assert_eq!(spec.prompts.len(), 1);
    assert_eq!(spec.workflows.len(), 1);
    assert_eq!(spec.prompts[0].name, "one");
    assert_eq!(
        spec.workflows[0].prompt_name.as_deref(),
        Some("bake_summary")
    );
}

#[test]
fn registry_derives_prompt_from_examples_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let skill_root = tmp.path().join("example-skill");
    let refs = skill_root.join("references");
    fs::create_dir_all(&refs).unwrap();
    fs::write(
        refs.join("EXAMPLES.md"),
        "Example: call `example_skill__inspect_scene` before editing.",
    )
    .unwrap();

    let md = skill_metadata(
        "example-skill",
        &skill_root,
        serde_json::json!({"dcc-mcp.examples": "references/EXAMPLES.md"}),
    );
    let reg = PromptRegistry::new(true);

    let prompts = reg.list(|visit| visit(&md));
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].name, "example-skill.examples");
    assert_eq!(
        prompts[0]
            .meta
            .as_ref()
            .and_then(|m| m.get("dcc.prompt_source"))
            .and_then(|s| s.get("source"))
            .and_then(serde_json::Value::as_str),
        Some("examples")
    );

    let rendered = reg
        .get("example-skill.examples", &HashMap::new(), |visit| {
            visit(&md)
        })
        .unwrap();
    let McpPromptContent::Text { text } = &rendered.messages[0].content;
    assert!(text.contains("example_skill__inspect_scene"));
}

#[test]
fn registry_derives_prompt_from_workflow_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let skill_root = tmp.path().join("workflow-skill");
    let workflows = skill_root.join("workflows");
    fs::create_dir_all(&workflows).unwrap();
    fs::write(
        workflows.join("review.workflow.yaml"),
        r#"
name: review_scene
description: Review the active scene before export.
steps:
  - id: inspect
    tool: maya_scene__inspect
  - id: export
    tool: maya_scene__export
"#,
    )
    .unwrap();

    let md = skill_metadata(
        "workflow-skill",
        &skill_root,
        serde_json::json!({"dcc-mcp.workflows": "workflows/*.workflow.yaml"}),
    );
    let reg = PromptRegistry::new(true);

    let prompts = reg.list(|visit| visit(&md));
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].name, "workflow-skill.review_scene");

    let rendered = reg
        .get("workflow-skill.review_scene", &HashMap::new(), |visit| {
            visit(&md)
        })
        .unwrap();
    let McpPromptContent::Text { text } = &rendered.messages[0].content;
    assert!(text.contains("maya_scene__inspect"));
    assert!(text.contains("workflows_run"));
}

#[test]
fn empty_prompt_list_reports_diagnostics() {
    let tmp = tempfile::tempdir().unwrap();
    let md = skill_metadata("plain-skill", tmp.path(), serde_json::json!({}));
    let reg = PromptRegistry::new(true);

    let prompts = reg.list(|visit| visit(&md));
    assert!(prompts.is_empty());
    let diagnostics = reg.diagnostics(|visit| visit(&md));
    assert!(diagnostics.enabled);
    assert_eq!(diagnostics.loaded_skill_count, 1);
    assert_eq!(diagnostics.prompt_count, 0);
    assert_eq!(diagnostics.prompt_capable_skill_count, 0);
    assert!(
        diagnostics
            .notes
            .iter()
            .any(|note| note.contains("did not declare metadata.dcc-mcp.prompts"))
    );
}
