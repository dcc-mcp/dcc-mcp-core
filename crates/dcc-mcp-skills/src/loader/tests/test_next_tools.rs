//! Tests for issue #342: next-tools in sibling tools.yaml.
use super::fixtures::write_skill;
use super::*;

#[test]
fn sibling_tools_yaml_parses_next_tools() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("nt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tools.yaml"),
        r#"tools:
  - name: create_sphere
    description: make a sphere
    next-tools:
      on-success:
        - maya_geometry__bevel_edges
        - maya_geometry__assign_material
      on-failure:
        - diagnostics__screenshot
"#,
    )
    .unwrap();
    let body = r#"---
name: nt
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.tools: tools.yaml
---
"#;
    std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(meta.is_spec_compliant());
    assert_eq!(meta.tools.len(), 1);
    let nt = &meta.tools[0].next_tools;
    assert_eq!(
        nt.on_success,
        vec![
            "maya_geometry__bevel_edges".to_string(),
            "maya_geometry__assign_material".to_string(),
        ]
    );
    assert_eq!(nt.on_failure, vec!["diagnostics__screenshot".to_string()]);
}

#[test]
fn top_level_next_tools_is_legacy_and_non_compliant() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("legacy_nt");
    let body = r#"---
name: legacy_nt
dcc: maya
next-tools:
  on-success: [foo]
---
"#;
    write_skill(&dir, body);
    let meta = parse_skill_md(&dir).expect("parsed");
    assert!(
        !meta.is_spec_compliant(),
        "top-level next-tools must be flagged as legacy",
    );
    assert!(
        meta.legacy_extension_fields
            .iter()
            .any(|s| s == "next-tools"),
        "legacy_extension_fields must name next-tools; got {:?}",
        meta.legacy_extension_fields,
    );
}
