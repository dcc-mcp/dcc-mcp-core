use super::*;
use serde_json::json;

fn projector() -> DiagnosticProjection {
    DiagnosticProjection::new(
        ["godot", "blender", "dcc-mcp-godot", "dcc-mcp-cli"].map(str::to_owned),
    )
}

#[test]
fn incompatible_core_status_survives_diagnostic_projection() {
    let output = projector().project(&json!({"adapter_imports": {"probes": [
        {"status": "incompatible_core"}
    ]}}));
    assert_eq!(
        output["adapter_imports"]["probes"][0]["status"],
        "incompatible_core"
    );
}

#[test]
fn unexpected_and_nested_sensitive_fields_never_echo_keys_or_values() {
    let mut projection = projector();
    let output = projection.project(&json!({
        "dcc_type": "godot", "adapter_version": "0.3.0", "phase": "startup",
        "observed": "SENTINEL_FREE_TEXT", "evidence": { "error_kind": "ModuleNotFoundError",
            "secret_SENTINEL_KEY": "SENTINEL_CREDENTIAL", "extra": { "password": "SENTINEL_NESTED" } },
        "unknown_SENTINEL_FIELD": {"token": "SENTINEL_UNKNOWN", "username": "SENTINEL_USER"},
        "cli": { "version": "0.20.4", "chat": "SENTINEL_CHAT", "private_path": "C:\\Users\\SENTINEL_HOME" },
        "status": "SENTINEL_STATUS", "environment": { "KEY": "SENTINEL_ENV" }
    }));
    let review = projection.finish();
    let encoded = serde_json::to_string(&json!({"data": output, "review": review})).unwrap();
    assert!(!encoded.contains("SENTINEL"));
    assert_eq!(output["dcc_type"], "godot");
    assert_eq!(output["evidence"]["error_kind"], "import_error");
    assert!(review.omitted_fields["credentials"] >= 3);
    assert!(review.review_required);
    assert!(!review.secret_free_guarantee);
}

#[test]
fn versions_drop_private_build_metadata_and_unrecognized_prereleases() {
    let mut projection = projector();
    let output = projection.project(&json!({
        "adapter_version": "0.3.0+SENTINEL_WORKSTATION", "core_version": "v0.20.4+SENTINEL_USER",
        "host_version": "4.4", "python_version": "3.11.8", "version": "0.3.0-SENTINEL_PRIVATE"
    }));
    assert_eq!(output["adapter_version"], "0.3.0");
    assert_eq!(output["core_version"], "0.20.4");
    assert_eq!(output["host_version"], "4.4");
    assert!(output.get("version").is_none());
    assert_eq!(
        projection.finish().omitted_fields["version_build_metadata"],
        2
    );
    assert!(!output.to_string().contains("SENTINEL"));
}

#[test]
fn loading_origin_categories_survive_but_paths_and_custom_errors_do_not() {
    let mut projection = projector();
    let output = projection.project(&json!({ "adapter_imports": { "total": 1, "probes": [{
        "dcc_type": "blender", "status": "unimportable", "dist_version": "0.9.2",
        "python_source": "cli_override", "dist_source": "dist_info_dir",
        "error_type": "ModuleNotFoundError",
        "module_path": "SENTINEL_MODULE", "exception_type": "SENTINEL_CUSTOM_EXCEPTION",
        "message": "SENTINEL_MESSAGE", "username": "SENTINEL_USER"
    }] } }));
    let row = &output["adapter_imports"]["probes"][0];
    assert_eq!(row["python_source"], "cli_override");
    assert_eq!(row["dist_source"], "dist_info_dir");
    assert_eq!(row["error_type"], "import_error");
    assert_eq!(row["exception_type"], "unclassified");
    assert!(!output.to_string().contains("SENTINEL"));
}

#[test]
fn known_field_names_cannot_smuggle_objects_or_untrusted_identifiers() {
    let mut projection = projector();
    let output = projection.project(&json!({ "dcc_type": "SENTINEL_DCC", "adapter": "SENTINEL_ADAPTER",
        "core_version": {"private": "SENTINEL_VERSION"}, "healthy": "SENTINEL_HEALTHY",
        "status": {"secret": "SENTINEL_STATUS"}, "steps": ["SENTINEL_COMMAND", {"status": "failed"}] }));
    assert_eq!(output["steps"], json!([{"status": "failed"}]));
    assert!(!output.to_string().contains("SENTINEL"));
}

#[test]
fn collections_and_recursion_are_bounded() {
    let mut projection = projector();
    let output =
        projection.project(&json!({"records": vec![json!({"status": "ok"}); MAX_ITEMS + 4]}));
    assert_eq!(output["records"].as_array().unwrap().len(), MAX_ITEMS);
    assert_eq!(projection.finish().omitted_fields["bounded_collections"], 1);
    let mut projection = projector();
    let mut nested = json!({"status": "ok"});
    for _ in 0..MAX_DEPTH + 2 {
        nested = json!({"evidence": nested});
    }
    let output = projection.project(&nested);
    assert!(!output.to_string().contains("ok"));
    assert_eq!(projection.finish().omitted_fields["bounded_collections"], 1);
}

#[test]
fn version_normalization_keeps_supported_prerelease_identity() {
    let mut projection = projector();
    let output = projection.project(&json!({"version": "v0.20.4", "core_version": "0.21.0-rc.1"}));
    assert_eq!(output["version"], "0.20.4");
    assert_eq!(output["core_version"], "0.21.0-rc.1");
    let review = projection.finish();
    assert_eq!(review.omitted_fields["version_normalized"], 1);
    assert!(!review.omitted_fields.contains_key("version_build_metadata"));
}
