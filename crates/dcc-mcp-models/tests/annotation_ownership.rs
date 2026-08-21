use dcc_mcp_models::SkillToolAnnotations;

#[test]
fn skill_annotations_accept_authoring_aliases_and_serialize_as_snake_case() {
    let annotations: SkillToolAnnotations = serde_json::from_value(serde_json::json!({
        "readOnlyHint": true,
        "destructive-hint": false
    }))
    .expect("authoring aliases should deserialize");

    assert_eq!(annotations.read_only_hint, Some(true));
    assert_eq!(annotations.destructive_hint, Some(false));
    assert_eq!(
        serde_json::to_value(annotations).expect("annotations should serialize"),
        serde_json::json!({
            "read_only_hint": true,
            "destructive_hint": false
        })
    );
}

#[test]
#[allow(deprecated)]
fn legacy_model_name_is_a_source_compatible_alias() {
    let legacy = dcc_mcp_models::ToolAnnotations {
        title: Some("Inspect Scene".to_string()),
        ..Default::default()
    };
    let canonical: SkillToolAnnotations = legacy;

    assert_eq!(canonical.title.as_deref(), Some("Inspect Scene"));
}
