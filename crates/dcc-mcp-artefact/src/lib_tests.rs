//! Unit tests for the artefact module.

use super::*;

use tempfile::tempdir;

#[test]
fn fileref_round_trips_through_json() {
    let fr = FileRef {
        uri: "artefact://sha256/abc".to_string(),
        mime: Some("image/png".to_string()),
        size_bytes: Some(1024),
        digest: Some("sha256:abc".to_string()),
        producer_job_id: Some(Uuid::nil()),
        created_at: Utc::now(),
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
