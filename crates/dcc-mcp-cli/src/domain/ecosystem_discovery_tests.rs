use super::*;
use serde_json::json;

fn request() -> DiscoveryRequest {
    DiscoveryRequest {
        offline: false,
        query: String::new(),
        target: None,
        provider: None,
        limit: 50,
    }
}

fn observation(provider: &str, entries: serde_json::Value) -> CatalogObservation {
    let mut source =
        DiscoverySource::unavailable(provider, "https://example.invalid/catalog", "unused");
    source.state = "available";
    source.source = "remote";
    source.freshness = "live_observation";
    source.latest_checked = true;
    source.sha256 = Some("a".repeat(64));
    source.reason = None;
    CatalogObservation {
        source,
        entries: serde_json::from_value(entries).unwrap(),
    }
}

#[test]
fn arbitrary_new_names_targets_and_requirements_flow_from_catalog() {
    let catalog = observation(
        INSTALL_PROVIDER,
        json!([{
            "name": "future-compositor-integration",
            "description": "Future compositor bridge",
            "targets": [{"kind": "application", "id": "future-compositor"}],
            "version": "2.7.0",
            "core_requirement": ">=0.20.0,<0.21.0",
            "requires": {"bins": ["future-compositor"], "env": ["COMPOSITOR_HOME"]},
            "policy": {"installation": "disabled", "reason": "publisher hold"}
        }]),
    );
    let report = assemble(request(), vec![catalog]);
    assert_eq!(report.status, "complete");
    assert_eq!(report.count, 1);
    let component = &report.components[0];
    assert_eq!(component.identity.name, "future-compositor-integration");
    assert_eq!(component.targets[0].id, "future-compositor");
    assert_eq!(component.component_type, "unknown");
    assert_eq!(
        component.core_requirement.as_deref(),
        Some(">=0.20.0,<0.21.0")
    );
    assert_eq!(
        component.requires.as_ref().unwrap().bins,
        ["future-compositor"]
    );
    assert_eq!(component.policy.as_ref().unwrap().installation, "disabled");
    assert_eq!(component.availability, "catalog_available");
    assert_eq!(component.installed, "not_checked");
    assert_eq!(component.runtime, "not_checked");
    assert_eq!(component.compatibility, "not_checked");
    let wire = serde_json::to_value(report).unwrap();
    assert!(wire["components"][0]["published_at"].is_null());
    assert_eq!(wire["schema_version"], "dcc-mcp.ecosystem-discovery.v1");
    assert!(wire["read_only"].as_bool().unwrap());
}

#[test]
fn duplicate_names_are_provider_qualified_and_tie_order_is_stable() {
    let entries = json!([{"name": "shared-name", "description": "Shared catalog name"}]);
    let report = assemble(
        request(),
        vec![
            observation(MARKETPLACE_PROVIDER, entries.clone()),
            observation(INSTALL_PROVIDER, entries),
        ],
    );
    assert_eq!(report.count, 2);
    assert_eq!(
        report.components[0].identity.name,
        report.components[1].identity.name
    );
    assert_ne!(
        report.components[0].identity.provider_id,
        report.components[1].identity.provider_id
    );
    assert_eq!(report.components[0].identity.provider_id, INSTALL_PROVIDER);
    assert_eq!(report.components[0].evidence.provider_id, INSTALL_PROVIDER);
}

#[test]
fn filters_and_limit_apply_before_total_and_truncation_are_reported() {
    let entries = json!([
        {"name":"future-alpha", "description":"Graph editing", "targets":[{"kind":"application","id":"future-editor"}], "package":{"format":"skill","skills":["graph-edit"]}},
        {"name":"future-beta", "description":"Graph editing", "targets":[{"kind":"application","id":"future-editor"}]},
        {"name":"other", "description":"Graph editing", "targets":[{"kind":"game","id":"future-editor"}]}
    ]);
    let mut options = request();
    options.query = "Graph editing".into();
    options.target = Some(dcc_mcp_marketplace::parse_target("application:FUTURE-EDITOR").unwrap());
    options.limit = 1;
    let report = assemble(options, vec![observation(INSTALL_PROVIDER, entries)]);
    assert_eq!(report.total_matches, 2);
    assert_eq!(report.count, 1);
    assert!(report.truncated);
    assert_eq!(report.components[0].identity.name, "future-alpha");
    assert_eq!(report.components[0].component_type, "skill");
}

#[test]
fn provider_failure_is_distinct_from_empty_success_and_preserves_results() {
    let empty = observation(INSTALL_PROVIDER, json!([]));
    let complete = assemble(request(), vec![empty.clone()]);
    assert_eq!(complete.status, "complete");
    assert_eq!(complete.count, 0);
    let failed = CatalogObservation {
        source: DiscoverySource::unavailable(
            MARKETPLACE_PROVIDER,
            "https://example.invalid",
            "network_error",
        ),
        entries: Vec::new(),
    };
    let unavailable = assemble(request(), vec![failed.clone()]);
    assert_eq!(unavailable.status, "unavailable");
    let partial = assemble(
        request(),
        vec![
            observation(
                INSTALL_PROVIDER,
                json!([{"name":"future", "description":"Available"}]),
            ),
            failed,
        ],
    );
    assert_eq!(partial.status, "partial");
    assert_eq!(partial.count, 1);
    assert_eq!(partial.providers[1].reason, Some("network_error"));
    let mut fallback = empty;
    fallback.source.refresh_error = Some("official_install_catalog_unavailable");
    assert_eq!(assemble(request(), vec![fallback]).status, "partial");
}

#[test]
fn failed_provider_entries_are_never_emitted() {
    let mut rejected = observation(
        INSTALL_PROVIDER,
        json!([{"name":"tampered", "description":"Reject"}]),
    );
    rejected.source.state = "unavailable";
    rejected.source.reason = Some("catalog_rejected");
    let report = assemble(request(), vec![rejected]);
    assert_eq!(report.count, 0);
    assert_eq!(report.status, "unavailable");
}

#[test]
fn legacy_dcc_targets_use_shared_catalog_normalization() {
    let report = assemble(
        request(),
        vec![observation(
            INSTALL_PROVIDER,
            json!([{
                "name":"unrecognized-host", "description":"A newly declared host", "dcc":["new-host"]
            }]),
        )],
    );
    assert_eq!(report.components[0].targets[0].to_string(), "dcc:new-host");
}
