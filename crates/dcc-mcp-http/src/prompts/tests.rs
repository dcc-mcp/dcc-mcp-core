//! Unit tests for [`crate::prompts`].

use super::*;
use std::collections::HashMap;

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
