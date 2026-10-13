use super::*;

fn payload() -> Payload {
    serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "source_revision": "a".repeat(40),
        "issued_at": 1000,
        "expires_at": 2000,
        "entries": [{"name":"dcc-mcp-godot", "description":"Godot adapter", "dcc":["godot"]}]
    }))
    .unwrap()
}

#[test]
fn payload_rejects_expired_future_and_unbounded_lifetimes() {
    assert!(validate_payload(&payload(), 1500, true).is_ok());
    assert!(validate_payload(&payload(), 2000, true).is_err());
    assert!(validate_payload(&payload(), 2000, false).is_ok());
    assert!(validate_payload(&payload(), 699, false).is_err());
    for expires in [999, 1000, 1000 + MAX_LIFETIME + 1] {
        let mut candidate = payload();
        candidate.expires_at = expires;
        assert!(validate_payload(&candidate, 1500, true).is_err());
    }
}

#[test]
fn payload_rejects_unknown_schema_duplicate_entries_and_mutable_revision() {
    let mut candidate = payload();
    candidate.schema_version = 2;
    assert!(validate_payload(&candidate, 1500, true).is_err());
    candidate = payload();
    candidate.entries.push(candidate.entries[0].clone());
    assert!(validate_payload(&candidate, 1500, true).is_err());
    candidate = payload();
    candidate.source_revision = "main".into();
    assert!(validate_payload(&candidate, 1500, true).is_err());
    candidate = payload();
    candidate.entries.clear();
    assert!(validate_payload(&candidate, 1500, true).is_err());
}

#[test]
fn payload_rejects_unbound_artifact_metadata() {
    let mut candidate = payload();
    candidate.entries = dcc_mcp_catalog::load_from_str(super::super::BUNDLED_CATALOG).unwrap();
    assert!(validate_payload(&candidate, 1500, true).is_ok());
    let entry = candidate
        .entries
        .iter_mut()
        .find(|entry| entry.name == "dcc-mcp-godot")
        .unwrap();
    entry.version = Some("9.9.9".into());
    assert!(validate_payload(&candidate, 1500, true).is_err());
}

fn snapshot(issued_at: u64, digest: &str) -> ResolvedCatalog {
    ResolvedCatalog {
        entries: payload().entries,
        provenance: CatalogProvenance {
            issued_at: Some(issued_at),
            sha256: Some(digest.into()),
            ..CatalogProvenance::local(CatalogSource::Remote)
        },
    }
}

#[test]
fn rejects_replay_and_same_time_equivocation() {
    let accepted = snapshot(1500, "a");
    assert!(reject_rollback(&accepted, &snapshot(1499, "a")).is_err());
    assert!(reject_rollback(&accepted, &snapshot(1500, "b")).is_err());
    assert!(reject_rollback(&accepted, &snapshot(1500, "a")).is_ok());
    assert!(reject_rollback(&accepted, &snapshot(1501, "b")).is_ok());
}

#[test]
fn rejects_unsigned_tampered_and_wrong_workflow_envelopes() {
    let unsigned = serde_json::json!({"catalog":"{}", "attestation":{}});
    assert!(verify(&serde_json::to_vec(&unsigned).unwrap(), 1500, true).is_err());
    assert!(verify(b"truncated", 1500, true).is_err());
    assert!(verify(&vec![b' '; MAX_BYTES + 1], 1500, true).is_err());
    // A real, valid Core release signature must not authorize install catalogs.
    let envelope = Envelope {
        catalog: include_str!(
            "../../../../../dcc-mcp-attestation/tests/fixtures/core-release.json"
        )
        .into(),
        attestation: serde_json::from_str(include_str!(
            "../../../../../dcc-mcp-attestation/tests/fixtures/core-release.sigstore.json"
        ))
        .unwrap(),
    };
    let error = verify(&serde_json::to_vec(&envelope).unwrap(), 1500, true).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("signature, workflow identity, or digest")
    );
}

#[test]
fn cache_replacement_is_atomic_and_invalid_cache_cannot_be_promoted() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalog.json");
    assert!(read_cache(&path).unwrap().is_none());
    persist(&path, b"old").unwrap();
    persist(&path, b"new").unwrap();
    assert_eq!(read_cache(&path).unwrap().unwrap(), b"new");
    let now = now_seconds().unwrap();
    let mut replacement = snapshot(now, "x");
    replacement.provenance.expires_at = Some(now + 60);
    let expected = verify(b"new", now, false).unwrap_err().to_string();
    let error = persist_verified(&path, b"replacement", &replacement, now).unwrap_err();
    assert_eq!(error.to_string(), expected);
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
}

#[tokio::test]
async fn discovery_offline_does_not_poll_http_or_create_cache_directories() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("missing/catalog.json");
    let (catalog, refresh_error) = resolve_discovery(&path, true, async {
        panic!("offline discovery must not poll the download future")
    })
    .await
    .unwrap();
    assert_eq!(catalog.provenance.source, CatalogSource::Bundled);
    assert!(!catalog.provenance.latest_checked);
    assert!(!catalog.entries.is_empty());
    assert_eq!(refresh_error, None);
    assert!(!path.parent().unwrap().exists());
}

#[tokio::test]
async fn discovery_unavailable_channel_reports_partial_bundled_fallback_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("missing/catalog.json");
    let (catalog, refresh_error) =
        resolve_discovery(&path, false, async { Err(RefreshError::Unavailable) })
            .await
            .unwrap();
    assert_eq!(catalog.provenance.source, CatalogSource::Bundled);
    assert!(!catalog.provenance.latest_checked);
    assert_eq!(refresh_error, Some("official_install_catalog_unavailable"));
    assert!(!path.parent().unwrap().exists());
}

#[tokio::test]
async fn discovery_rejects_tampered_cache_without_network_or_cache_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalog.json");
    for bytes in [
        b"truncated".as_slice(),
        br#"{"catalog":"{}","attestation":{}}"#.as_slice(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        for offline in [true, false] {
            let error = resolve_discovery(&path, offline, async {
                panic!("a rejected cache must fail before network access")
            })
            .await
            .unwrap_err();
            assert_eq!(error, "catalog_rejected");
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        }
    }
}

#[tokio::test]
async fn discovery_rejects_unsigned_remote_without_creating_cache() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("missing/catalog.json");
    let error = resolve_discovery(&path, false, async {
        Ok(br#"{"catalog":"{}","attestation":{}}"#.to_vec())
    })
    .await
    .unwrap_err();
    assert_eq!(error, "catalog_rejected");
    assert!(!path.parent().unwrap().exists());
}

fn current_snapshot(issued_at: u64, expires_at: u64, digest: &str) -> ResolvedCatalog {
    let mut catalog = snapshot(issued_at, digest);
    catalog.provenance.expires_at = Some(expires_at);
    catalog.provenance.latest_checked = true;
    catalog
}

#[test]
fn discovery_cached_fallback_preserves_provenance_and_reports_failed_refresh() {
    let cached = current_snapshot(1000, 2000, "a");
    for unavailable in [false, true] {
        let remote = unavailable.then_some(Err(RefreshError::Unavailable));
        let (catalog, refresh_error) =
            discovery_snapshot(Some(cached.clone()), remote, 1500).unwrap();
        assert_eq!(catalog.provenance.source, CatalogSource::Cache);
        assert!(!catalog.provenance.latest_checked);
        assert_eq!(catalog.provenance.sha256.as_deref(), Some("a"));
        assert_eq!(catalog.provenance.issued_at, Some(1000));
        assert_eq!(catalog.provenance.expires_at, Some(2000));
        assert_eq!(
            refresh_error,
            unavailable.then_some("official_install_catalog_unavailable")
        );
    }
}

#[test]
fn discovery_expired_cache_is_an_error_instead_of_bundled_fallback() {
    let cached = current_snapshot(1000, 2000, "a");
    for now in [2000, 2001] {
        for unavailable in [false, true] {
            let remote = unavailable.then_some(Err(RefreshError::Unavailable));
            let error = discovery_snapshot(Some(cached.clone()), remote, now).unwrap_err();
            assert_eq!(error, "catalog_expired");
        }
    }
}

#[test]
fn discovery_remote_must_respect_expired_cache_rollback_history() {
    let cached = current_snapshot(1000, 2000, "a");
    for remote in [
        current_snapshot(999, 2500, "a"),
        current_snapshot(1000, 2500, "b"),
    ] {
        let error = discovery_snapshot(Some(cached.clone()), Some(Ok(remote)), 2001).unwrap_err();
        assert_eq!(error, "catalog_rejected");
    }
    let (catalog, refresh_error) = discovery_snapshot(
        Some(cached),
        Some(Ok(current_snapshot(1500, 2500, "b"))),
        2001,
    )
    .unwrap();
    assert_eq!(catalog.provenance.source, CatalogSource::Remote);
    assert!(catalog.provenance.latest_checked);
    assert_eq!(catalog.provenance.sha256.as_deref(), Some("b"));
    assert_eq!(refresh_error, None);
}

#[test]
fn discovery_rejected_remote_does_not_fall_back_to_valid_cache() {
    let error = discovery_snapshot(
        Some(current_snapshot(1000, 2000, "a")),
        Some(Err(invalid("signature verification failed"))),
        1500,
    )
    .unwrap_err();
    assert_eq!(error, "catalog_rejected");
}

#[test]
fn discovery_remote_expiring_during_verification_is_not_accepted() {
    let error =
        discovery_snapshot(None, Some(Ok(current_snapshot(1000, 2000, "a"))), 2000).unwrap_err();
    assert_eq!(error, "catalog_expired");
}
