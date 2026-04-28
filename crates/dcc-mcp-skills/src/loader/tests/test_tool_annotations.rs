//! Tests for issue #344: ToolAnnotations from sibling tools.yaml.
use super::*;

/// Canonical nested `annotations:` map on a per-tool entry parses
/// into `ToolDeclaration::annotations` with every hint set.
#[test]
fn annotations_canonical_nested_map_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("canon");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tools.yaml"),
        "tools:\n  - name: delete_keyframes\n    description: danger\n    annotations:\n      read_only_hint: false\n      destructive_hint: true\n      idempotent_hint: true\n      open_world_hint: false\n      deferred_hint: false\n",
    )
    .unwrap();
    let body = "---\nname: canon\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n";
    std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();

    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.tools.len(), 1);
    let ann = &meta.tools[0].annotations;
    assert_eq!(ann.read_only_hint, Some(false));
    assert_eq!(ann.destructive_hint, Some(true));
    assert_eq!(ann.idempotent_hint, Some(true));
    assert_eq!(ann.open_world_hint, Some(false));
    assert_eq!(ann.deferred_hint, Some(false));
}

/// Shorthand flat hint keys on the tool entry still parse — backward compat.
#[test]
fn annotations_shorthand_flat_keys_parse() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("short");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tools.yaml"),
        "tools:\n  - name: get_keyframes\n    read_only_hint: true\n    idempotent_hint: true\n",
    )
    .unwrap();
    let body = "---\nname: short\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n";
    std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();

    let meta = parse_skill_md(&dir).expect("parsed");
    assert_eq!(meta.tools.len(), 1);
    let ann = &meta.tools[0].annotations;
    assert_eq!(ann.read_only_hint, Some(true));
    assert_eq!(ann.idempotent_hint, Some(true));
    // Undeclared hints stay None.
    assert_eq!(ann.destructive_hint, None);
    assert_eq!(ann.open_world_hint, None);
    assert_eq!(ann.deferred_hint, None);
}

/// When both nested `annotations:` and shorthand flat keys are present,
/// the nested map wins entirely (whole-map replacement, not per-field merge).
#[test]
fn annotations_nested_wins_over_shorthand() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("mixed");
    std::fs::create_dir_all(&dir).unwrap();
    // Shorthand declares read_only=true, idempotent=true.
    // Nested declares destructive=true only.  After merge, the nested
    // map wins whole-map: read_only_hint/idempotent_hint MUST be None.
    std::fs::write(
        dir.join("tools.yaml"),
        "tools:\n  - name: risky\n    read_only_hint: true\n    idempotent_hint: true\n    annotations:\n      destructive_hint: true\n",
    )
    .unwrap();
    let body = "---\nname: mixed\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n";
    std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();

    let meta = parse_skill_md(&dir).expect("parsed");
    let ann = &meta.tools[0].annotations;
    assert_eq!(ann.destructive_hint, Some(true));
    assert_eq!(
        ann.read_only_hint, None,
        "nested map wins whole-map; shorthand read_only_hint must be dropped"
    );
    assert_eq!(
        ann.idempotent_hint, None,
        "nested map wins whole-map; shorthand idempotent_hint must be dropped"
    );
}

/// Tools without any annotations declared leave the field empty.
#[test]
fn annotations_absent_is_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("bare");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tools.yaml"),
        "tools:\n  - name: plain\n    description: nothing special\n",
    )
    .unwrap();
    let body = "---\nname: bare\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n";
    std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();

    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(meta.tools[0].annotations.is_empty());
}
