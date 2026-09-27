//! Tests for `metadata.dcc-mcp.recall-context` parsing (PIP-3701).
//!
//! `RecallContext` is the structured discovery signal `SkillMetadata`
//! carries — `app_type`, `domain`, `workflow_stage`, `task_category` — and
//! every field is optional, so "absent" and "unknown" are the same state.
//!
//! Before PIP-3701 the loader had no arm for this key at all. It was authored
//! in shipped SKILL.md files and fell through to the `unknown key` debug log,
//! which is why the skills benchmark measured 0% coverage on skills that
//! declared all four fields. These tests pin the parsing that the coverage
//! measurement depends on.

use super::fixtures::write_skill;
use super::*;

#[test]
fn recall_block_populates_the_typed_field() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("recall");
    write_skill(
        &dir,
        r#"---
name: recall
description: A skill that declares where it lives in the DCC universe.
metadata:
  dcc-mcp:
    dcc: maya
    recall-context:
      app_type: maya
      domain: modeling
      workflow_stage: authoring
      task_category: mutate
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    let context = meta
        .recall_context
        .as_ref()
        .expect("nested dcc-mcp.recall-context must be parsed into the typed field");
    assert_eq!(context.app_type.as_deref(), Some("maya"));
    assert_eq!(context.domain.as_deref(), Some("modeling"));
    assert_eq!(context.workflow_stage.as_deref(), Some("authoring"));
    assert_eq!(context.task_category.as_deref(), Some("mutate"));
}

#[test]
fn partial_block_leaves_the_rest_unknown() {
    // Every field is optional. A skill that names only its app has not
    // claimed a domain, so the field must stay `None` rather than being
    // inferred — a derived value would make the coverage number mean
    // something other than "authored".
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("partial");
    write_skill(
        &dir,
        r#"---
name: partial
description: Only one axis declared.
metadata:
  dcc-mcp:
    dcc: blender
    recall-context:
      app_type: blender
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    let context = meta.recall_context.as_ref().expect("parsed");
    assert_eq!(context.app_type.as_deref(), Some("blender"));
    assert_eq!(context.domain, None);
    assert_eq!(context.workflow_stage, None);
    assert_eq!(context.task_category, None);
}

#[test]
fn empty_block_keeps_the_field_unset() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("empty");
    write_skill(
        &dir,
        r#"---
name: empty
description: An explicitly blank recall block.
metadata:
  dcc-mcp:
    dcc: maya
    recall-context: {}
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(
        meta.recall_context.is_none(),
        "an empty recall-context block must stay None so callers can tell \
         `unset` from `declared`"
    );
}

#[test]
fn recall_context_is_none_when_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("absent");
    write_skill(
        &dir,
        "---\nname: absent\ndescription: no recall block at all\n---\n",
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(
        meta.recall_context.is_none(),
        "recall_context must be None when SKILL.md does not declare it"
    );
}

#[test]
fn snake_case_and_kebab_case_spellings_agree() {
    // The struct accepts both spellings; authors should not have to care.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("kebab");
    write_skill(
        &dir,
        r#"---
name: kebab
description: Uses the hyphenated key spelling.
metadata:
  dcc-mcp:
    dcc: houdini
    recall-context:
      app_type: houdini
      workflow-stage: simulation
---
"#,
    );
    let meta = parse_skill_md(&dir).expect("parsed");
    let context = meta.recall_context.as_ref().expect("parsed");
    assert_eq!(context.app_type.as_deref(), Some("houdini"));
    assert_eq!(
        context.workflow_stage.as_deref(),
        Some("simulation"),
        "the `workflow-stage` alias must land on `workflow_stage`"
    );
}
