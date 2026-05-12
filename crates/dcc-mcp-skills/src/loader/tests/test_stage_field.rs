//! Tests for `metadata.dcc-mcp.stage` field parsing.
//!
//! `stage` is a free-form string the loader writes into the typed
//! [`SkillMetadata::stage`] field whenever a SKILL.md frontmatter
//! declares the canonical nested `metadata.dcc-mcp.stage` key. The
//! field exists so DCC adapters (Maya, Blender, Houdini, …) can stop
//! maintaining hard-coded `{skill_name → stage}` shadow tables and
//! instead read the stage straight off the metadata that was parsed
//! once at scan time.
//!
//! The legacy flat-form shorthand (`metadata: { "dcc-mcp.stage": ... }`)
//! is intentionally **not** routed into the typed field — same rule as
//! `layer` (see `test_layer_field.rs`) — so adapters get a loud failure
//! instead of a silently stale value when a skill file is left on the
//! pre-0.15 shape.

use super::fixtures::write_skill;
use super::*;

#[test]
fn flat_form_stage_is_no_longer_parsed_into_typed_field() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("flat_legacy");
    write_skill(
        &dir,
        r#"---
name: flat-legacy
description: A pre-0.15 skill using the dropped flat-form shorthand.
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.stage: authoring
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(
        meta.stage.is_none(),
        "legacy flat-form dcc-mcp.stage must not populate the typed field; \
         migrate to `metadata: {{ dcc-mcp: {{ stage: authoring }} }}`",
    );
}

#[test]
fn stage_field_is_parsed_nested_form() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("nested_stage");
    write_skill(
        &dir,
        r#"---
name: nested-stage
description: An authoring-stage skill.
metadata:
  dcc-mcp:
    dcc: maya
    stage: authoring
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(
        meta.stage.as_deref(),
        Some("authoring"),
        "nested dcc-mcp.stage must be parsed into the typed field"
    );
}

#[test]
fn stage_field_none_when_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("no_stage");
    write_skill(
        &dir,
        "---\nname: no-stage\ndescription: no stage key set\n---\n",
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(
        meta.stage.is_none(),
        "stage must be None when not declared in SKILL.md"
    );
}

#[test]
fn stage_value_is_treated_as_opaque_string() {
    // Core deliberately does NOT validate the stage vocabulary — different
    // DCC adapters own different taxonomies (Maya: bootstrap / scene /
    // authoring / interchange / pipeline; Houdini might add `simulation`,
    // Photoshop might add `compositing`). The loader's job is to round-trip
    // the value verbatim; vocabulary policing belongs to the adapter that
    // consumes the field.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("custom_stage");
    write_skill(
        &dir,
        r#"---
name: custom-stage
description: An adapter-specific stage value the core has never heard of.
metadata:
  dcc-mcp:
    dcc: houdini
    stage: simulation-postprocess
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(
        meta.stage.as_deref(),
        Some("simulation-postprocess"),
        "core must round-trip arbitrary stage strings unmodified"
    );
}

#[test]
fn stage_and_layer_are_independent() {
    // `stage` (where in the pipeline) and `layer` (what kind of skill)
    // are orthogonal. Setting one must not leak into the other.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("stage_and_layer");
    write_skill(
        &dir,
        r#"---
name: stage-and-layer
description: Both axes set — stage and layer must not bleed.
metadata:
  dcc-mcp:
    dcc: maya
    layer: domain
    stage: interchange
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.layer.as_deref(), Some("domain"));
    assert_eq!(meta.stage.as_deref(), Some("interchange"));
}
