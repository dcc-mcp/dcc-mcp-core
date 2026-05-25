//! Tests for issue #356: metadata.dcc-mcp.* parsing and strict
//! rejection of legacy top-level extension keys.
//!
//! The pre-0.15 flat-form shorthand (`metadata: { "dcc-mcp.dcc": ... }`)
//! is no longer recognised; only the nested form
//! (`metadata: { dcc-mcp: { dcc: ... } }`) drives the typed
//! `SkillMetadata` extensions.
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
fn legacy_flat_form_does_not_populate_typed_fields() {
    // The pre-0.15 flat shorthand is no longer parsed into
    // `SkillMetadata` typed fields; the skill parses (the YAML itself
    // is valid) but every typed override stays at its default so the
    // author notices the gap and migrates.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("flat_legacy");
    let body = r#"---
name: flat-legacy
description: pre-0.15 shorthand
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "2.0.0"
  dcc-mcp.tags: "a, b"
---
# body
"#;
    write_skill(&dir, body);
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_ne!(
        meta.dcc, "maya",
        "flat-form `dcc-mcp.dcc: maya` must NOT populate the typed field; \
         it should fall back to the serde default"
    );
    assert_ne!(
        meta.version, "2.0.0",
        "flat-form `dcc-mcp.version` must NOT populate the typed field"
    );
    assert!(
        meta.tags.is_empty(),
        "flat-form tags must NOT be recognised"
    );
}

#[test]
fn nested_form_parses_successfully() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("new");
    let body = r#"---
name: new
description: new-form skill
metadata:
  dcc-mcp:
    dcc: maya
    version: "2.0.0"
    tags: "a, b"
    search-hint: "hint words"
    search-aliases: [make sphere, primitive ball]
    depends: "other-skill"
---
# body
"#;
    write_skill(&dir, body);
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.dcc, "maya");
    assert_eq!(meta.version, "2.0.0");
    assert_eq!(meta.tags, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(meta.search_hint, "hint words");
    assert_eq!(
        meta.search_aliases,
        vec!["make sphere".to_string(), "primitive ball".to_string()]
    );
    assert_eq!(meta.depends, vec!["other-skill".to_string()]);
}

#[test]
fn nested_form_with_inline_tools_list_parses() {
    // Canonical agentskills.io shape — `metadata.dcc-mcp` is a nested map.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("nested");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tools.yaml"),
        "tools:\n  - name: create_sphere\n    description: make a sphere\n    search_aliases: [primitive ball, mesh globe]\n",
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
    assert_eq!(
        meta.tools[0].search_aliases,
        vec!["primitive ball".to_string(), "mesh globe".to_string()]
    );
}

#[test]
fn nested_form_sibling_tools_yaml_resolves() {
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
  dcc-mcp:
    dcc: maya
    tools: tools.yaml
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
fn nested_form_parses_optional_runtime_descriptors() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("runtime");
    let body = r#"---
name: runtime
metadata:
  dcc-mcp:
    runtimes:
      - name: usd-core
        type: python_package
        package: usd-core
        module: pxr
        optional: true
        feature_level: full-usd
        install_hint: "pip install dcc-mcp-openusd[usd-core]"
      - name: usdcat
        type: binary
        binary: usdcat
        optional: true
        guidance: "Install OpenUSD command-line tools."
      - name: HFS
        type: env_var
        env: HFS
        optional: true
        description: Houdini Solaris runtime.
---
# body
"#;
    write_skill(&dir, body);
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.runtimes.len(), 3);
    assert_eq!(meta.runtimes[0].name, "usd-core");
    assert_eq!(
        meta.runtimes[0].kind,
        dcc_mcp_models::SkillRuntimeKind::PythonPackage
    );
    assert_eq!(
        meta.runtimes[0].guidance.as_deref(),
        Some("pip install dcc-mcp-openusd[usd-core]")
    );
    assert_eq!(meta.runtimes[1].binary.as_deref(), Some("usdcat"));
    assert_eq!(meta.runtimes[2].env.as_deref(), Some("HFS"));
}

#[test]
fn nested_form_parses_runtime_descriptors_from_sibling_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("runtime-sidecar");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("runtimes.yaml"),
        "runtimes:\n  - name: usdcat\n    type: binary\n    binary: usdcat\n    optional: true\n",
    )
    .unwrap();
    let body = r#"---
name: runtime-sidecar
metadata:
  dcc-mcp:
    runtimes: runtimes.yaml
---
# body
"#;
    std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.runtimes.len(), 1);
    assert_eq!(meta.runtimes[0].name, "usdcat");
    assert_eq!(meta.runtimes[0].binary.as_deref(), Some("usdcat"));
}

#[test]
fn nested_form_products_and_implicit_invocation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("policy");
    let body = r#"---
name: policy
metadata:
  dcc-mcp:
    products: "maya, houdini"
    allow-implicit-invocation: "false"
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
