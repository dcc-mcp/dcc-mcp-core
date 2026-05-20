//! Unit tests for the artefact module.

use super::*;

use tempfile::tempdir;

#[test]
fn fileref_round_trips_through_json() {
    let fr = FileRef {
        uri: "artefact://sha256/abc".to_string(),
        mime: Some("image/png".to_string()),
        size_bytes: Some(1024),
        display_name: Some("preview.png".to_string()),
        digest: Some("sha256:abc".to_string()),
        producer_job_id: Some(Uuid::nil()),
        tool_call_id: Some("req-1".to_string()),
        session_id: Some("session-1".to_string()),
        correlation_id: Some("corr-1".to_string()),
        created_at: Utc::now(),
        expires_at: Some(Utc::now() + Duration::seconds(60)),
        metadata: serde_json::json!({"width": 256}),
    };
    let text = serde_json::to_string(&fr).unwrap();
    let round: FileRef = serde_json::from_str(&text).unwrap();
    assert_eq!(round, fr);
}

#[test]
fn sha256_hash_is_stable_for_same_bytes() {
    let a = hash_bytes_sha256(b"hello world");
    let b = hash_bytes_sha256(b"hello world");
    assert_eq!(a, b);
    // Known SHA-256 of "hello world".
    assert_eq!(
        a,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn in_memory_store_put_get_head() {
    let store = InMemoryArtefactStore::new();
    let fr = store.put(ArtefactBody::Inline(b"hello".to_vec())).unwrap();
    assert!(fr.uri.starts_with("artefact://sha256/"));
    assert_eq!(fr.size_bytes, Some(5));
    assert!(fr.digest.as_deref().unwrap().starts_with("sha256:"));

    let body = store.get(&fr.uri).unwrap().unwrap();
    assert_eq!(body.into_bytes().unwrap(), b"hello");

    let head = store.head(&fr.uri).unwrap().unwrap();
    assert_eq!(head.uri, fr.uri);
}

#[test]
fn in_memory_store_dedups_same_bytes() {
    let store = InMemoryArtefactStore::new();
    let a = store.put(ArtefactBody::Inline(b"dup".to_vec())).unwrap();
    let b = store.put(ArtefactBody::Inline(b"dup".to_vec())).unwrap();
    assert_eq!(a.uri, b.uri);
    assert_eq!(a.digest, b.digest);
}

#[test]
fn in_memory_store_unknown_uri_returns_none() {
    let store = InMemoryArtefactStore::new();
    assert!(store.get("artefact://sha256/ffff").unwrap().is_none());
    assert!(store.head("artefact://sha256/ffff").unwrap().is_none());
}

#[test]
fn fs_store_put_get_and_sidecar_exists() {
    let tmp = tempdir().unwrap();
    let store = FilesystemArtefactStore::new_in(tmp.path()).unwrap();
    let fr = store
        .put(ArtefactBody::Inline(b"payload".to_vec()))
        .unwrap();
    let hex = fr.sha256_hex().unwrap().to_string();

    assert!(tmp.path().join(format!("{hex}.bin")).exists());
    assert!(tmp.path().join(format!("{hex}.json")).exists());

    let body = store.get(&fr.uri).unwrap().unwrap();
    assert_eq!(body.into_bytes().unwrap(), b"payload");

    // Sidecar round-trip.
    let head = store.head(&fr.uri).unwrap().unwrap();
    assert_eq!(head.uri, fr.uri);
    assert_eq!(head.size_bytes, Some(7));
}

#[test]
fn fs_store_dedups_same_bytes() {
    let tmp = tempdir().unwrap();
    let store = FilesystemArtefactStore::new_in(tmp.path()).unwrap();
    let a = store.put(ArtefactBody::Inline(b"same".to_vec())).unwrap();
    let b = store.put(ArtefactBody::Inline(b"same".to_vec())).unwrap();
    assert_eq!(a.uri, b.uri);
}

#[test]
fn fs_store_list_and_filter() {
    let tmp = tempdir().unwrap();
    let store = FilesystemArtefactStore::new_in(tmp.path()).unwrap();
    store.put(ArtefactBody::Inline(b"one".to_vec())).unwrap();
    store.put(ArtefactBody::Inline(b"two".to_vec())).unwrap();
    let all = store.list(ArtefactFilter::default()).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn fs_store_delete_removes_body_and_sidecar() {
    let tmp = tempdir().unwrap();
    let store = FilesystemArtefactStore::new_in(tmp.path()).unwrap();
    let fr = store.put(ArtefactBody::Inline(b"gone".to_vec())).unwrap();
    store.delete(&fr.uri).unwrap();
    assert!(store.get(&fr.uri).unwrap().is_none());
    assert!(store.head(&fr.uri).unwrap().is_none());
}

#[test]
fn invalid_uri_rejected() {
    let store = FilesystemArtefactStore::new_in(tempdir().unwrap().path()).unwrap();
    let err = store.get("not-a-uri").unwrap_err();
    assert!(matches!(err, ArtefactError::InvalidUri(_)));
}

#[test]
fn hash_file_matches_bytes() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("f.bin");
    fs::write(&p, b"abc123").unwrap();
    let a = hash_file_sha256(&p).unwrap();
    let b = hash_bytes_sha256(b"abc123");
    assert_eq!(a, b);
}

#[test]
fn put_with_options_persists_metadata_and_correlation() {
    let store = InMemoryArtefactStore::new();
    let fr = store
        .put_with_options(
            ArtefactBody::Inline(b"metadata".to_vec()),
            ArtefactPutOptions {
                mime: Some("text/plain".to_string()),
                display_name: Some("log.txt".to_string()),
                tool_call_id: Some("req-42".to_string()),
                session_id: Some("session-a".to_string()),
                correlation_id: Some("trace-a".to_string()),
                metadata: serde_json::json!({"lines": 3}),
                ..ArtefactPutOptions::default()
            },
        )
        .unwrap();

    assert_eq!(fr.mime.as_deref(), Some("text/plain"));
    assert_eq!(fr.display_name.as_deref(), Some("log.txt"));
    assert_eq!(fr.tool_call_id.as_deref(), Some("req-42"));
    assert_eq!(fr.metadata["lines"], 3);

    let filtered = store
        .list(ArtefactFilter {
            session_id: Some("session-a".to_string()),
            ..ArtefactFilter::default()
        })
        .unwrap();
    assert_eq!(filtered.len(), 1);
}

#[test]
fn in_memory_duplicate_bytes_keep_distinct_logical_metadata() {
    let store = InMemoryArtefactStore::new();
    let first = store
        .put_with_options(
            ArtefactBody::Inline(b"same bytes".to_vec()),
            ArtefactPutOptions {
                tool_call_id: Some("req-1".to_string()),
                session_id: Some("session-a".to_string()),
                correlation_id: Some("corr-a".to_string()),
                ..ArtefactPutOptions::default()
            },
        )
        .unwrap();
    let second = store
        .put_with_options(
            ArtefactBody::Inline(b"same bytes".to_vec()),
            ArtefactPutOptions {
                tool_call_id: Some("req-2".to_string()),
                session_id: Some("session-b".to_string()),
                correlation_id: Some("corr-b".to_string()),
                ..ArtefactPutOptions::default()
            },
        )
        .unwrap();

    assert_eq!(first.uri, second.uri);
    assert_eq!(second.tool_call_id.as_deref(), Some("req-2"));

    let first_filtered = store
        .list(ArtefactFilter {
            session_id: Some("session-a".to_string()),
            ..ArtefactFilter::default()
        })
        .unwrap();
    let second_filtered = store
        .list(ArtefactFilter {
            tool_call_id: Some("req-2".to_string()),
            ..ArtefactFilter::default()
        })
        .unwrap();
    assert_eq!(first_filtered.len(), 1);
    assert_eq!(second_filtered.len(), 1);
    assert_eq!(second_filtered[0].correlation_id.as_deref(), Some("corr-b"));
}

#[test]
fn fs_duplicate_bytes_keep_distinct_logical_metadata() {
    let tmp = tempdir().unwrap();
    let store = FilesystemArtefactStore::new_in(tmp.path()).unwrap();
    let first = store
        .put_with_options(
            ArtefactBody::Inline(b"same bytes".to_vec()),
            ArtefactPutOptions {
                tool_call_id: Some("req-1".to_string()),
                session_id: Some("session-a".to_string()),
                correlation_id: Some("corr-a".to_string()),
                ..ArtefactPutOptions::default()
            },
        )
        .unwrap();
    let second = store
        .put_with_options(
            ArtefactBody::Inline(b"same bytes".to_vec()),
            ArtefactPutOptions {
                tool_call_id: Some("req-2".to_string()),
                session_id: Some("session-b".to_string()),
                correlation_id: Some("corr-b".to_string()),
                ..ArtefactPutOptions::default()
            },
        )
        .unwrap();

    assert_eq!(first.uri, second.uri);
    assert_eq!(second.session_id.as_deref(), Some("session-b"));

    let first_filtered = store
        .list(ArtefactFilter {
            session_id: Some("session-a".to_string()),
            ..ArtefactFilter::default()
        })
        .unwrap();
    let second_filtered = store
        .list(ArtefactFilter {
            tool_call_id: Some("req-2".to_string()),
            ..ArtefactFilter::default()
        })
        .unwrap();
    assert_eq!(first_filtered.len(), 1);
    assert_eq!(second_filtered.len(), 1);
    assert_eq!(second_filtered[0].correlation_id.as_deref(), Some("corr-b"));
}

#[test]
fn in_memory_store_rejects_oversized_payloads() {
    let store = InMemoryArtefactStore::with_limits(ArtefactStoreLimits {
        max_body_bytes: Some(4),
        ..ArtefactStoreLimits::default()
    });
    let err = store
        .put(ArtefactBody::Inline(b"too large".to_vec()))
        .unwrap_err();
    assert!(matches!(err, ArtefactError::LimitExceeded(_)));
}

#[test]
fn in_memory_store_expires_ttl_payloads() {
    let store = InMemoryArtefactStore::new();
    let fr = store
        .put_with_options(
            ArtefactBody::Inline(b"short lived".to_vec()),
            ArtefactPutOptions {
                ttl_secs: Some(0),
                ..ArtefactPutOptions::default()
            },
        )
        .unwrap();
    assert!(store.head(&fr.uri).unwrap().is_none());
    assert!(store.get(&fr.uri).unwrap().is_none());
}

#[test]
fn fs_store_enforces_max_entries_by_removing_oldest() {
    let tmp = tempdir().unwrap();
    let store = FilesystemArtefactStore::new_bounded_in(
        tmp.path(),
        ArtefactStoreLimits {
            max_entries: Some(2),
            ..ArtefactStoreLimits::default()
        },
    )
    .unwrap();
    let first = store.put(ArtefactBody::Inline(b"one".to_vec())).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = store.put(ArtefactBody::Inline(b"two".to_vec())).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let third = store.put(ArtefactBody::Inline(b"three".to_vec())).unwrap();

    assert!(store.head(&first.uri).unwrap().is_none());
    assert!(store.head(&second.uri).unwrap().is_some());
    assert!(store.head(&third.uri).unwrap().is_some());
}
