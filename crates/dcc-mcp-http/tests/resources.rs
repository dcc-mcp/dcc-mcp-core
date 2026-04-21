//! Integration tests for the MCP Resources primitive (issue #350).
//!
//! Covers the dispatcher contract: `resources/list`, `resources/read`,
//! `resources/subscribe` and the `artefact://` disabled-stub behaviour.

use std::sync::Arc;

use dcc_mcp_actions::ActionRegistry;
use dcc_mcp_http::{McpHttpConfig, McpHttpServer, ResourceRegistry};
use dcc_mcp_sandbox::{AuditEntry, AuditLog, AuditOutcome};
use serde_json::json;

fn make_server(enable: bool, artefact: bool) -> McpHttpServer {
    let registry = Arc::new(ActionRegistry::new());
    let mut cfg = McpHttpConfig::new(0);
    cfg.enable_resources = enable;
    cfg.enable_artefact_resources = artefact;
    McpHttpServer::new(registry, cfg)
}

#[test]
fn registry_lists_default_producers() {
    let reg = ResourceRegistry::new(true, false);
    let uris: Vec<String> = reg.list().into_iter().map(|r| r.uri).collect();
    assert!(uris.contains(&"scene://current".to_string()));
    assert!(uris.contains(&"audit://recent".to_string()));
    assert!(!uris.iter().any(|u| u.starts_with("artefact://")));
}

#[test]
fn registry_hides_all_when_disabled() {
    let reg = ResourceRegistry::new(false, false);
    assert!(reg.list().is_empty());
    assert!(!reg.is_enabled());
}

#[test]
fn scene_snapshot_is_round_tripped() {
    let reg = ResourceRegistry::new(true, false);
    reg.set_scene(json!({"scene_name": "my_shot", "node_count": 7}));
    let result = reg.read("scene://current").expect("read");
    let first = &result.contents[0];
    assert_eq!(first.mime_type.as_deref(), Some("application/json"));
    let text = first.text.as_deref().unwrap();
    assert!(text.contains("my_shot"));
    assert!(text.contains("7"));
}

#[test]
fn artefact_returns_not_enabled_error() {
    use dcc_mcp_http::ResourceError;
    let reg = ResourceRegistry::new(true, false);
    let err = reg.read("artefact://sha256/abc").unwrap_err();
    assert!(matches!(err, ResourceError::NotEnabled(_)));
}

#[test]
fn subscribe_tracks_per_session_and_is_reversible() {
    let reg = ResourceRegistry::new(true, false);
    assert!(reg.subscribe("session-a", "scene://current"));
    assert!(reg.subscribe("session-b", "scene://current"));
    assert!(reg.subscribe("session-a", "audit://recent"));

    let mut scene_subs = reg.sessions_subscribed_to("scene://current");
    scene_subs.sort();
    assert_eq!(scene_subs, vec!["session-a", "session-b"]);

    assert!(reg.unsubscribe("session-a", "scene://current"));
    assert_eq!(
        reg.sessions_subscribed_to("scene://current"),
        vec!["session-b".to_string()]
    );
}

#[test]
fn server_registers_resource_registry() {
    let server = make_server(true, false);
    // Registry is reachable and carries the built-in producers.
    let uris: Vec<String> = server
        .resources()
        .list()
        .into_iter()
        .map(|r| r.uri)
        .collect();
    assert!(uris.contains(&"scene://current".to_string()));
}

#[test]
fn disabled_server_still_has_registry_but_empty_list() {
    let server = make_server(false, false);
    assert!(!server.resources().is_enabled());
    assert!(server.resources().list().is_empty());
}

#[tokio::test]
async fn audit_wire_notifies_on_append() {
    use std::time::Duration;

    let reg = ResourceRegistry::new(true, false);
    let log = Arc::new(AuditLog::new());
    reg.wire_audit_log(log.clone());

    let mut rx = reg.watch_updates();
    log.record(AuditEntry::new(
        Some("agent".into()),
        "op",
        "{}",
        Duration::from_millis(1),
        AuditOutcome::Success,
    ));

    let uri = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("update notification fired within 2s")
        .expect("broadcast delivered");
    assert_eq!(uri, "audit://recent");

    let result = reg.read("audit://recent?limit=10").unwrap();
    let text = result.contents[0].text.as_deref().unwrap();
    assert!(text.contains("\"count\":1"));
    assert!(text.contains("\"op\""));
}
