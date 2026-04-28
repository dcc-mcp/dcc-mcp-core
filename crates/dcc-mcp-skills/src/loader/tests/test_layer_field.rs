//! Tests for metadata.dcc-mcp.layer field parsing.
//!
//! Before the fix, `metadata.dcc-mcp.layer` produced a DEBUG warning
//! "unknown metadata.dcc-mcp.layer key — ignoring" because the loader's
//! apply_dcc_mcp_metadata_overrides() had no match arm for "layer".
use super::fixtures::write_skill;
use super::*;

#[test]
fn layer_field_is_parsed_flat_form() {
    // Skill authors (dcc-mcp-maya uses this key on all 14 skills) would
    // see spurious warnings and the field was silently dropped.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("layered");
    write_skill(
        &dir,
        r#"---
name: layered
description: A domain skill for Maya geometry.
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.layer: domain
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(meta.is_spec_compliant());
    assert_eq!(
        meta.layer.as_deref(),
        Some("domain"),
        "dcc-mcp.layer must be parsed into SkillMetadata::layer"
    );
}

#[test]
fn layer_field_is_parsed_nested_form() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("nested_layer");
    write_skill(
        &dir,
        r#"---
name: nested-layer
description: An infrastructure skill.
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(meta.is_spec_compliant());
    assert_eq!(
        meta.layer.as_deref(),
        Some("infrastructure"),
        "nested dcc-mcp.layer must be parsed"
    );
}

#[test]
fn layer_field_none_when_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("no_layer");
    write_skill(
        &dir,
        "---\nname: no-layer\ndescription: no layer key set\n---\n",
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(
        meta.layer.is_none(),
        "layer must be None when not declared in SKILL.md"
    );
}
