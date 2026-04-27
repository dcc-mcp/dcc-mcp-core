//! Unit tests for [`crate::resources`].

use super::*;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::json;
use std::sync::Arc;

#[test]
fn scene_read_returns_placeholder_when_no_snapshot() {
    let reg = ResourceRegistry::new(true, false);
    let result = reg.read("scene://current").expect("scene read");
    let first = &result.contents[0];
    assert_eq!(first.uri, "scene://current");
    assert_eq!(first.mime_type.as_deref(), Some("application/json"));
    let text = first.text.as_deref().unwrap();
    assert!(text.contains("no_scene_published"));
}

#[test]
fn scene_read_uses_published_snapshot() {
    let reg = ResourceRegistry::new(true, false);
    reg.set_scene(json!({"name": "my-scene", "nodes": 42}));
    let result = reg.read("scene://current").unwrap();
    let text = result.contents[0].text.as_deref().unwrap();
    assert!(text.contains("my-scene"));
    assert!(text.contains("42"));
}

#[test]
fn list_includes_scene_and_audit_by_default() {
    let reg = ResourceRegistry::new(true, false);
    let uris: Vec<String> = reg.list().into_iter().map(|r| r.uri).collect();
    assert!(uris.iter().any(|u| u == "scene://current"));
    assert!(uris.iter().any(|u| u == "audit://recent"));
    // artefact hidden when disabled.
    assert!(!uris.iter().any(|u| u.starts_with("artefact://")));
}

#[test]
fn artefact_read_returns_not_enabled_when_disabled() {
    let reg = ResourceRegistry::new(true, false);
    let err = reg.read("artefact://abc123").unwrap_err();
    assert!(matches!(err, ResourceError::NotEnabled(_)));
}

#[test]
fn unknown_scheme_returns_not_found() {
    let reg = ResourceRegistry::new(true, false);
    let err = reg.read("bogus://x").unwrap_err();
    assert!(matches!(err, ResourceError::NotFound(_)));
}

#[test]
fn subscribe_and_unsubscribe_tracks_session() {
    let reg = ResourceRegistry::new(true, false);
    assert!(reg.subscribe("sess1", "scene://current"));
    // Duplicate returns false.
    assert!(!reg.subscribe("sess1", "scene://current"));
    let subs = reg.sessions_subscribed_to("scene://current");
    assert_eq!(subs, vec!["sess1".to_string()]);
    assert!(reg.unsubscribe("sess1", "scene://current"));
    assert!(reg.sessions_subscribed_to("scene://current").is_empty());
}

#[test]
fn audit_read_returns_empty_tail_by_default() {
    let reg = ResourceRegistry::new(true, false);
    let result = reg.read("audit://recent?limit=5").unwrap();
    let text = result.contents[0].text.as_deref().unwrap();
    assert!(text.contains("\"count\":0"));
    assert!(text.contains("\"limit\":5"));
}

#[tokio::test]
async fn wire_audit_log_fires_update_on_record() {
    use dcc_mcp_sandbox::{AuditEntry, AuditLog, AuditOutcome};
    use std::time::Duration;

    let reg = ResourceRegistry::new(true, false);
    let log = Arc::new(AuditLog::new());
    reg.wire_audit_log(log.clone());

    let mut rx = reg.watch_updates();
    log.record(AuditEntry::new(
        None,
        "test",
        "{}",
        Duration::from_millis(1),
        AuditOutcome::Success,
    ));
    // Allow the forwarding task to run.
    let uri = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("update fired")
        .expect("recv ok");
    assert_eq!(uri, "audit://recent");

    // And the tail now includes the entry.
    let result = reg.read("audit://recent?limit=10").unwrap();
    let text = result.contents[0].text.as_deref().unwrap();
    assert!(text.contains("\"count\":1"));
}

#[test]
fn disabled_artefact_hidden_from_list() {
    let reg = ResourceRegistry::new(true, false);
    assert!(!reg.list().iter().any(|r| r.uri.starts_with("artefact://")));
}

#[test]
fn enabled_artefact_store_surfaces_put_in_list_and_read() {
    use dcc_mcp_artefact::ArtefactBody;

    let reg = ResourceRegistry::new(true, true);
    let store = reg.artefact_store().expect("store wired");
    let fr = store
        .put(ArtefactBody::Inline(b"hello-artefact".to_vec()))
        .unwrap();

    // list should include the new URI.
    let uris: Vec<String> = reg.list().into_iter().map(|r| r.uri).collect();
    assert!(uris.contains(&fr.uri), "list missing {}: {uris:?}", fr.uri);

    // read should return the bytes as a blob (base64).
    let result = reg.read(&fr.uri).expect("read ok");
    let item = &result.contents[0];
    assert_eq!(item.uri, fr.uri);
    assert!(item.blob.is_some(), "expected base64 blob");
    let decoded = BASE64_STANDARD
        .decode(item.blob.as_deref().unwrap())
        .unwrap();
    assert_eq!(decoded, b"hello-artefact");
}

#[test]
fn enabled_artefact_read_unknown_uri_returns_not_found() {
    let reg = ResourceRegistry::new(true, true);
    let err = reg.read("artefact://sha256/deadbeef").unwrap_err();
    assert!(matches!(err, ResourceError::NotFound(_)));
}
