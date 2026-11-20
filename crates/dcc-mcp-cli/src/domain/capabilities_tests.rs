use super::*;

#[test]
fn arguments_include_only_explicit_filters() {
    let request = CapabilitiesRequest {
        dcc_type: "houdini".to_string(),
        instance_id: "abc12345-0000-0000-0000-000000000000".to_string(),
        keys: vec!["render.lpe".to_string(), "render.cryptomatte".to_string()],
        force_refresh: true,
        invalidate: false,
    };

    assert_eq!(
        request.arguments(),
        json!({
            "keys": ["render.lpe", "render.cryptomatte"],
            "force_refresh": true
        })
    );
}

#[test]
fn arguments_omit_defaults() {
    let request = CapabilitiesRequest {
        dcc_type: "maya".to_string(),
        instance_id: "def67890-0000-0000-0000-000000000000".to_string(),
        keys: Vec::new(),
        force_refresh: false,
        invalidate: false,
    };

    assert_eq!(request.arguments(), json!({}));
}

#[test]
fn arguments_include_invalidate() {
    let request = CapabilitiesRequest {
        dcc_type: "maya".to_string(),
        instance_id: "abc".to_string(),
        keys: Vec::new(),
        force_refresh: false,
        invalidate: true,
    };
    assert_eq!(request.backend_tool(), DESCRIBE_CAPABILITIES_TOOL);
    assert_eq!(request.arguments(), json!({"invalidate": true}));
}
