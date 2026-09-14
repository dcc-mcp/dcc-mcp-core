use dcc_mcp_catalog::{load_from_str, validate_catalog_entries};
use serde_json::json;

#[test]
fn optional_policy_reason_preserves_legacy_catalogs_and_curation_explanations() {
    for reason in [
        None,
        Some("Held until the newer release passes validation."),
    ] {
        let mut policy = json!({"installation": "available"});
        if let Some(reason) = reason {
            policy["reason"] = json!(reason);
        }
        let catalog = json!({
            "entries": [{
                "name": "dcc-mcp-studio-editor",
                "description": "Studio adapter",
                "policy": policy,
            }],
        });
        let entries = load_from_str(&catalog.to_string()).unwrap();
        validate_catalog_entries(&entries).unwrap();
        assert_eq!(
            entries[0].policy.as_ref().unwrap().reason.as_deref(),
            reason
        );
        let serialized = serde_json::to_value(&entries[0]).unwrap();
        assert_eq!(serialized["policy"], policy);
    }
}

#[test]
fn policy_reason_must_not_be_empty_when_supplied() {
    let entries = load_from_str(
        &json!({
            "entries": [{
                "name": "dcc-mcp-studio-editor",
                "description": "Studio adapter",
                "policy": {"installation": "not_available", "reason": ""},
            }],
        })
        .to_string(),
    )
    .unwrap();
    assert!(validate_catalog_entries(&entries).is_err());
}
