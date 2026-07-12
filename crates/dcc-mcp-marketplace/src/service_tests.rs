use super::*;

#[tokio::test]
async fn outdated_fetches_each_catalog_once() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let body = serde_json::json!({
        "version": "1",
        "entries": [
            {
                "name": "forest-assets",
                "description": "Forest assets",
                "dcc": ["blender"],
                "version": "2.0.0"
            },
            {
                "name": "rock-assets",
                "description": "Rock assets",
                "dcc": ["blender"],
                "version": "2.0.0"
            }
        ]
    })
    .to_string();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let request_count = Arc::new(AtomicUsize::new(0));
    let stopped = Arc::new(AtomicBool::new(false));
    let server_count = request_count.clone();
    let server_stopped = stopped.clone();
    let server = std::thread::spawn(move || {
        while !server_stopped.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if server_stopped.load(Ordering::Acquire) {
                        break;
                    }
                    let mut request = [0_u8; 2048];
                    let _ = stream.read(&mut request);
                    server_count.fetch_add(1, Ordering::AcqRel);
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("catalog test server failed: {error}"),
            }
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let service = MarketplaceService::new(temp.path().to_path_buf());
    let source_url = format!("http://{address}/marketplace.json");
    for name in ["forest-assets", "rock-assets"] {
        service
            .upsert_installed(InstalledMarketplacePackage {
                name: name.into(),
                dcc: "blender".into(),
                version: Some("1.0.0".into()),
                path: temp.path().join(name).display().to_string(),
                source_name: "local-test".into(),
                source_url: source_url.clone(),
                install_type: "git".into(),
                install_url: None,
                install_ref: None,
                resolved_commit: None,
                installed_at_ms: 1,
            })
            .unwrap();
    }

    let result = service.outdated(Some("blender"), Vec::new()).await.unwrap();
    stopped.store(true, Ordering::Release);
    let _ = TcpStream::connect(address);
    server.join().unwrap();

    assert_eq!(result.count, 2);
    assert_eq!(request_count.load(Ordering::Acquire), 1);
}

#[test]
fn pinned_git_revision_marks_unchanged_version_as_outdated() {
    let entry = CatalogEntry {
        name: "test-skill".into(),
        description: "desc".into(),
        dcc: vec!["maya".into()],
        url: None,
        tags: vec![],
        version: Some("1.0.0".into()),
        min_core_version: None,
        install: Some(CatalogInstall {
            install_type: "git".into(),
            url: Some("https://example.invalid/skill".into()),
            ref_: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into()),
            sha256: None,
            skill_roots: None,
            pip_package: None,
            pip_extras: None,
            python_path: None,
            entry_point: None,
            instructions_url: None,
        }),
        maintainer: None,
        category: None,
        policy: None,
        requires: None,
        icon: None,
        showcase: None,
    };
    let installed = InstalledMarketplacePackage {
        name: "test-skill".into(),
        dcc: "maya".into(),
        version: Some("1.0.0".into()),
        path: "/tmp/test".into(),
        source_name: "official".into(),
        source_url: "https://example.invalid/catalog.json".into(),
        install_type: "git".into(),
        install_url: entry
            .install
            .as_ref()
            .and_then(|install| install.url.clone()),
        install_ref: entry
            .install
            .as_ref()
            .and_then(|install| install.ref_.clone()),
        resolved_commit: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
        installed_at_ms: 1,
    };

    let (outdated, latest_commit) = is_entry_outdated(Some(&entry), &installed);
    assert!(outdated);
    assert_eq!(
        latest_commit.as_deref(),
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
}

#[test]
fn immutable_git_commit_only_accepts_full_object_ids() {
    let mut install = CatalogInstall {
        install_type: "git".into(),
        url: Some("https://example.invalid/skill".into()),
        ref_: Some("main".into()),
        sha256: None,
        skill_roots: None,
        pip_package: None,
        pip_extras: None,
        python_path: None,
        entry_point: None,
        instructions_url: None,
    };
    assert_eq!(immutable_git_commit(&install), None);
    install.ref_ = Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into());
    assert_eq!(
        immutable_git_commit(&install).as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
}

#[test]
fn core_version_gate_rejects_too_new_or_invalid_requirements() {
    let mut entry = CatalogEntry {
        name: "test-skill".into(),
        description: "desc".into(),
        dcc: vec!["maya".into()],
        url: None,
        tags: vec![],
        version: None,
        min_core_version: Some("999.0.0".into()),
        install: None,
        maintainer: None,
        category: None,
        policy: None,
        requires: None,
        icon: None,
        showcase: None,
    };

    assert!(matches!(
        ensure_entry_installable(&entry),
        Err(MarketplaceError::IncompatibleCoreVersion { .. })
    ));
    entry.min_core_version = Some("not-semver".into());
    assert!(matches!(
        ensure_entry_installable(&entry),
        Err(MarketplaceError::InvalidMinCoreVersion { .. })
    ));
    entry.min_core_version = Some("0.19.0".into());
    assert!(ensure_entry_installable(&entry).is_ok());
    entry.min_core_version = None;
    assert!(ensure_entry_installable(&entry).is_ok());
}

#[test]
fn install_policy_rejects_unavailable_entries() {
    let entry = CatalogEntry {
        name: "retired-skill".into(),
        description: "desc".into(),
        dcc: vec!["maya".into()],
        url: None,
        tags: vec![],
        version: None,
        min_core_version: None,
        install: None,
        maintainer: None,
        category: None,
        policy: Some(dcc_mcp_catalog::CatalogPolicy {
            installation: "not_available".into(),
        }),
        requires: None,
        icon: None,
        showcase: None,
    };
    assert!(matches!(
        ensure_entry_installable(&entry),
        Err(MarketplaceError::NotAvailable(name)) if name == "retired-skill"
    ));
}
