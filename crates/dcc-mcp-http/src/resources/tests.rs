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

#[test]
fn skill_resources_list_and_read_text_and_binary_files() {
    let tmp = tempfile::tempdir().unwrap();
    let resources_dir = tmp.path().join("resources");
    let data_dir = resources_dir.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::write(data_dir.join("help.txt"), "polySphere help").unwrap();
    std::fs::write(data_dir.join("preset.bin"), [0_u8, 1, 2, 3]).unwrap();
    std::fs::write(
        resources_dir.join("static.resource.yaml"),
        r#"
resources:
  - uri: maya-cmds://help/polySphere
    name: cmds.polySphere help
    mimeType: text/plain
    source:
      type: file
      path: data/help.txt
  - uri: preset://binary/cube
    name: cube preset
    mimeType: application/octet-stream
    source:
      type: file
      path: data/preset.bin
"#,
    )
    .unwrap();

    let reg = ResourceRegistry::new(true, false);
    let metadata = dcc_mcp_models::SkillMetadata {
        name: "maya-docs".to_string(),
        skill_path: tmp.path().to_string_lossy().into_owned(),
        metadata: json!({"dcc-mcp": {"resources": "resources/"}}),
        ..Default::default()
    };
    reg.sync_skill_resources(|visit| visit(&metadata));

    let uris: Vec<_> = reg
        .list()
        .into_iter()
        .map(|resource| resource.uri)
        .collect();
    assert!(uris.iter().any(|uri| uri == "maya-cmds://help/polySphere"));
    assert!(uris.iter().any(|uri| uri == "preset://binary/cube"));

    let text = reg.read("maya-cmds://help/polySphere").unwrap();
    assert_eq!(text.contents[0].text.as_deref(), Some("polySphere help"));

    let blob = reg.read("preset://binary/cube").unwrap();
    let decoded = BASE64_STANDARD
        .decode(blob.contents[0].blob.as_deref().unwrap())
        .unwrap();
    assert_eq!(decoded, [0_u8, 1, 2, 3]);
}

#[test]
fn malformed_skill_resource_yaml_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let resources_dir = tmp.path().join("resources");
    std::fs::create_dir_all(&resources_dir).unwrap();
    std::fs::write(resources_dir.join("bad.resource.yaml"), "resources: [").unwrap();

    let reg = ResourceRegistry::new(true, false);
    let metadata = dcc_mcp_models::SkillMetadata {
        name: "bad-docs".to_string(),
        skill_path: tmp.path().to_string_lossy().into_owned(),
        metadata: json!({"dcc-mcp": {"resources": "resources/"}}),
        ..Default::default()
    };
    reg.sync_skill_resources(|visit| visit(&metadata));

    assert!(
        !reg.list()
            .iter()
            .any(|resource| resource.uri.starts_with("bad://"))
    );
}
