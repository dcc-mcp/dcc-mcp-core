//! Tests for issue #356: metadata.dcc-mcp.* parsing and strict
//! rejection of legacy top-level extension keys.
use super::fixtures::write_skill;
use super::*;

#[test]
fn legacy_top_level_keys_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("legacy");
    write_skill(
        &dir,
        "---\nname: legacy\ndcc: maya\nversion: \"2.0.0\"\ntags: [a, b]\n---\n# body\n",
    );
    assert!(
        parse_skill_md(&dir).is_none(),
        "legacy top-level keys must cause the skill to be rejected"
    );
}

#[test]
fn new_form_parses_successfully() {
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
    assert_eq!(meta.dcc, "maya");
    assert_eq!(meta.version, "2.0.0");
    assert_eq!(meta.tags, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(meta.search_hint, "hint words");
    assert_eq!(meta.depends, vec!["other-skill".to_string()]);
}

#[test]
fn nested_form_parses_successfully() {
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
