//! Tests for issue #356: metadata.dcc-mcp.* compatibility (basic form tests).
use super::fixtures::write_skill;
use super::*;

#[test]
fn legacy_form_flags_non_compliant() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("legacy");
    write_skill(
        &dir,
        "---\nname: legacy\ndcc: maya\nversion: \"2.0.0\"\ntags: [a, b]\n---\n# body\n",
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.dcc, "maya");
    assert_eq!(meta.version, "2.0.0");
    assert_eq!(meta.tags, vec!["a".to_string(), "b".to_string()]);
    assert!(!meta.is_spec_compliant());
    assert!(meta.legacy_extension_fields.iter().any(|s| s == "dcc"));
}

#[test]
fn new_form_is_spec_compliant() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("new");
    let body = r#"---
name: new
description: new-form skill
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "2.0.0"
  dcc-mcp.tags: "a, b"
  dcc-mcp.search-hint: "hint words"
  dcc-mcp.depends: "other-skill"
---
# body
"#;
    write_skill(&dir, body);
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(meta.is_spec_compliant(), "expected spec compliant");
    assert_eq!(meta.dcc, "maya");
    assert_eq!(meta.version, "2.0.0");
    assert_eq!(meta.tags, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(meta.search_hint, "hint words");
    assert_eq!(meta.depends, vec!["other-skill".to_string()]);
}

#[test]
fn new_form_overrides_legacy_when_both_present() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("both");
    let body = r#"---
name: both
dcc: blender
metadata:
  dcc-mcp.dcc: maya
---
# body
"#;
    write_skill(&dir, body);
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.dcc, "maya", "metadata.dcc-mcp.dcc must win");
    // still marked legacy because top-level dcc was present
    assert!(!meta.is_spec_compliant());
}

#[test]
fn nested_form_is_spec_compliant() {
    // Canonical agentskills.io shape — `metadata.dcc-mcp` is a nested map.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("nested");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tools.yaml"),
        "tools:\n  - name: create_sphere\n    description: make a sphere\n",
    )
    .unwrap();
    let body = r#"---
name: nested
description: nested metadata form
metadata:
  dcc-mcp:
    dcc: maya
    version: "1.0.0"
    tags: [maya, animation]
    search-hint: "keyframe, timeline"
    tools: tools.yaml
---
# body
"#;
    std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(meta.is_spec_compliant(), "nested form must be compliant");
    assert_eq!(meta.dcc, "maya");
    assert_eq!(meta.version, "1.0.0");
    assert_eq!(meta.tags, vec!["maya".to_string(), "animation".to_string()]);
    assert_eq!(meta.search_hint, "keyframe, timeline");
    assert_eq!(meta.tools.len(), 1);
    assert_eq!(meta.tools[0].name, "create_sphere");
}

#[test]
fn new_form_sibling_tools_yaml_resolves() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("sidecar");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tools.yaml"),
        "tools:\n  - name: create_sphere\n    description: make a sphere\n  - ping\ngroups:\n  - name: advanced\n    default-active: false\n    tools: [create_sphere]\n",
    )
    .unwrap();
    let body = r#"---
name: sidecar
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.tools: tools.yaml
---
# body
"#;
    std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(meta.is_spec_compliant());
    assert_eq!(meta.tools.len(), 2);
    assert_eq!(meta.tools[0].name, "create_sphere");
    assert_eq!(meta.tools[0].description, "make a sphere");
    assert_eq!(meta.tools[1].name, "ping");
    assert_eq!(meta.groups.len(), 1);
    assert_eq!(meta.groups[0].name, "advanced");
    assert!(!meta.groups[0].default_active);
}

#[test]
fn new_form_products_and_implicit_invocation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("policy");
    let body = r#"---
name: policy
metadata:
  dcc-mcp.products: "maya, houdini"
  dcc-mcp.allow-implicit-invocation: "false"
---
# body
"#;
    write_skill(&dir, body);
    let meta = parse_skill_md(&dir).expect("parsed");
    let policy = meta.policy.expect("policy must be set");
    assert_eq!(
        policy.products,
        vec!["maya".to_string(), "houdini".to_string()]
    );
    assert_eq!(policy.allow_implicit_invocation, Some(false));
}

#[test]
fn both_forms_produce_same_values() {
    let tmp = tempfile::tempdir().unwrap();

    let legacy_dir = tmp.path().join("legacy");
    write_skill(
        &legacy_dir,
        "---\nname: same\ndcc: maya\nversion: \"1.2.3\"\ntags: [x, y]\nsearch-hint: hello\n---\n",
    );
    let legacy = parse_skill_md(&legacy_dir).expect("parsed");

    let new_dir = tmp.path().join("new");
    write_skill(
        &new_dir,
        r#"---
name: same
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "1.2.3"
  dcc-mcp.tags: "x, y"
  dcc-mcp.search-hint: hello
---
"#,
    );
    let newf = parse_skill_md(&new_dir).expect("parsed");

    assert_eq!(legacy.dcc, newf.dcc);
    assert_eq!(legacy.version, newf.version);
    assert_eq!(legacy.tags, newf.tags);
    assert_eq!(legacy.search_hint, newf.search_hint);
    assert!(!legacy.is_spec_compliant());
    assert!(newf.is_spec_compliant());
}
