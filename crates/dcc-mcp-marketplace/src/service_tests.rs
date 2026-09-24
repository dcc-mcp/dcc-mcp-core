use super::*;
use std::net::TcpStream;
use std::sync::atomic::AtomicUsize;

fn installed_package(name: &str, dcc: &str) -> InstalledMarketplacePackage {
    InstalledMarketplacePackage {
        name: name.into(),
        dcc: dcc.into(),
        target: CatalogTarget {
            kind: CatalogTargetKind::Dcc,
            id: dcc.into(),
        },
        components: Vec::new(),
        package_format: None,
        version: Some("1.0.0".into()),
        path: format!("/tmp/{name}-{dcc}"),
        source_name: "local-test".into(),
        source_url: "https://example.invalid/catalog.json".into(),
        install_type: "git".into(),
        install_url: None,
        install_ref: None,
        resolved_commit: None,
        installed_at_ms: 1,
    }
}

#[test]
fn legacy_installed_state_without_target_infers_dcc_target() {
    let temp = tempfile::tempdir().unwrap();
    let service = MarketplaceService::new(temp.path().to_path_buf());
    let state_path = temp.path().join("installed.json");
    std::fs::write(
        &state_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "packages": [{
                "name": "legacy-tools",
                "dcc": "Maya",
                "version": "0.20.4",
                "path": temp.path().join("maya").join("legacy-tools"),
                "source_name": "official",
                "source_url": "https://example.invalid/marketplace.json",
                "install_type": "git",
                "install_url": null,
                "install_ref": null,
                "installed_at_ms": 1
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let installed = service.list_installed(None).unwrap();
    assert_eq!(installed.count, 1);
    assert_eq!(
        installed.packages[0].target,
        CatalogTarget {
            kind: CatalogTargetKind::Dcc,
            id: "maya".into(),
        }
    );

    service
        .upsert_installed(installed_package("blender-tools", "blender"))
        .unwrap();
    let persisted: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(state_path).unwrap()).unwrap();
    assert!(
        persisted["packages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|package| package.get("target").is_some())
    );
}

#[test]
fn installed_package_without_target_or_legacy_dcc_is_rejected() {
    let mut package = serde_json::to_value(installed_package("broken-tools", "maya")).unwrap();
    let package = package.as_object_mut().unwrap();
    package.remove("target");
    package.insert("dcc".into(), serde_json::Value::String("  ".into()));

    let error = serde_json::from_value::<InstalledMarketplacePackage>(serde_json::Value::Object(
        package.clone(),
    ))
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing field `target` and legacy `dcc` is empty")
    );
}

#[test]
fn resolve_installed_dcc_infers_the_only_matching_host() {
    let temp = tempfile::tempdir().unwrap();
    let service = MarketplaceService::new(temp.path().to_path_buf());
    service
        .upsert_installed(installed_package("maya-tools", "maya"))
        .unwrap();

    assert_eq!(
        service.resolve_installed_dcc("maya-tools", None).unwrap(),
        "maya"
    );
}

#[test]
fn resolve_installed_dcc_requires_a_host_for_ambiguous_packages() {
    let temp = tempfile::tempdir().unwrap();
    let service = MarketplaceService::new(temp.path().to_path_buf());
    for dcc in ["maya", "blender"] {
        service
            .upsert_installed(installed_package("shared-tools", dcc))
            .unwrap();
    }

    assert!(matches!(
        service.resolve_installed_dcc("shared-tools", None),
        Err(MarketplaceError::AmbiguousInstalledDcc { name }) if name == "shared-tools"
    ));
}

#[test]
fn installed_state_keys_packages_by_name_and_generic_target() {
    let temp = tempfile::tempdir().unwrap();
    let service = MarketplaceService::new(temp.path().to_path_buf());
    for (kind, id) in [
        (CatalogTargetKind::Game, "the-bazaar"),
        (CatalogTargetKind::Application, "microsoft-excel"),
    ] {
        let mut package = installed_package("shared-profile", "");
        package.target = CatalogTarget {
            kind,
            id: id.to_string(),
        };
        service.upsert_installed(package).unwrap();
    }

    assert_eq!(service.list_installed_for_target(None).unwrap().count, 2);
    assert!(
        service
            .resolve_installed_target("shared-profile", None)
            .is_err()
    );
}

/// Answer exactly one request on `stream` with `body`, then close cleanly.
///
/// The accept loop needs a non-blocking listener so it can observe the stop
/// flag, and on Windows an accepted socket inherits that mode: `read` can
/// return `WouldBlock` before the request bytes arrive. Answering an unread
/// request and then dropping the socket leaves those bytes unread, and closing
/// a socket with unread data makes Windows reset the connection — the client
/// then fails with `WSAECONNABORTED` (10053) or
/// `hyper::Error(Canceled, UnexpectedMessage)` instead of reading the catalog.
///
/// So this reads on a blocking socket until the request header block is
/// complete and only counts the request then, and it drains the socket before
/// closing so the response the client is still reading is never discarded by a
/// reset.
fn serve_catalog_once(mut stream: TcpStream, body: &str, request_count: &AtomicUsize) {
    use std::io::{BufRead, Read, Write};
    use std::net::Shutdown;
    use std::sync::atomic::Ordering;

    let _ = stream.set_nonblocking(false);
    // Bounds both the header read below and the teardown drain.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

    let mut line = Vec::new();
    let mut headers_complete = false;
    {
        let mut reader = std::io::BufReader::new(&mut stream);
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                // The client closed before finishing its request — nothing to serve.
                Ok(0) => break,
                // A blank line terminates the request header block.
                Ok(_) if matches!(line.as_slice(), b"\r\n" | b"\n") => {
                    headers_complete = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
    if !headers_complete {
        return;
    }
    request_count.fetch_add(1, Ordering::AcqRel);

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    if stream.write_all(response.as_bytes()).is_err() {
        return;
    }
    let _ = stream.flush();

    // `Connection: close` only avoids a reset when the teardown is finished:
    // drop the write side and read until the client closes, so no unread bytes
    // are left to turn our FIN into an RST.
    let _ = stream.shutdown(Shutdown::Write);
    let mut drain = [0_u8; 256];
    while matches!(stream.read(&mut drain), Ok(count) if count > 0) {}
}

#[tokio::test]
async fn outdated_fetches_each_catalog_once() {
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
                Ok((stream, _)) => {
                    if server_stopped.load(Ordering::Acquire) {
                        break;
                    }
                    serve_catalog_once(stream, &body, &server_count);
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
                target: CatalogTarget {
                    kind: CatalogTargetKind::Dcc,
                    id: "blender".into(),
                },
                components: Vec::new(),
                package_format: None,
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
        targets: vec![],
        url: None,
        issues_url: None,
        tags: vec![],
        version: Some("1.0.0".into()),
        min_core_version: None,
        package: None,
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
            adobe: None,
            sop_version: None,
            sop_schema_digest: None,
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
        target: CatalogTarget {
            kind: CatalogTargetKind::Dcc,
            id: "maya".into(),
        },
        components: Vec::new(),
        package_format: None,
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
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
    };
    assert_eq!(immutable_git_commit(&install), None);
    install.ref_ = Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into());
    assert_eq!(
        immutable_git_commit(&install).as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
}

#[test]
fn git_install_rejects_mutable_ref_before_starting_git() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("checkout");
    let install = CatalogInstall {
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
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
    };

    let error = install_from_git_command(&install, &destination).unwrap_err();
    assert!(error.to_string().contains("40-character commit"));
    assert!(!destination.exists());
}

#[test]
fn git_install_checks_out_the_declared_commit_detached() {
    fn run_git(repo: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    let source = tempfile::tempdir().unwrap();
    run_git(source.path(), &["init", "--quiet"]);
    run_git(source.path(), &["config", "user.name", "Marketplace Test"]);
    run_git(
        source.path(),
        &["config", "user.email", "marketplace@example.invalid"],
    );
    std::fs::write(source.path().join("SKILL.md"), "first\n").unwrap();
    run_git(source.path(), &["add", "SKILL.md"]);
    run_git(source.path(), &["commit", "--quiet", "-m", "first"]);
    let pinned = run_git(source.path(), &["rev-parse", "HEAD"]);
    std::fs::write(source.path().join("SKILL.md"), "second\n").unwrap();
    run_git(source.path(), &["commit", "--quiet", "-am", "second"]);

    let target_root = tempfile::tempdir().unwrap();
    let destination = target_root.path().join("checkout");
    let install = CatalogInstall {
        install_type: "git".into(),
        url: Some(source.path().display().to_string()),
        ref_: Some(pinned.clone()),
        sha256: None,
        skill_roots: None,
        pip_package: None,
        pip_extras: None,
        python_path: None,
        entry_point: None,
        instructions_url: None,
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
    };

    install_from_git_command(&install, &destination).unwrap();
    assert_eq!(
        git_head_commit(&destination).as_deref(),
        Some(pinned.as_str())
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("SKILL.md"))
            .unwrap()
            .trim(),
        "first"
    );
    assert_eq!(run_git(&destination, &["branch", "--show-current"]), "");
}

#[tokio::test]
async fn zip_install_rejects_missing_or_invalid_sha_before_reading_archive() {
    let temp = tempfile::tempdir().unwrap();
    let service = MarketplaceService::new(temp.path().to_path_buf());
    let mut install = CatalogInstall {
        install_type: "zip".into(),
        url: Some(temp.path().join("missing.zip").display().to_string()),
        ref_: None,
        sha256: None,
        skill_roots: None,
        pip_package: None,
        pip_extras: None,
        python_path: None,
        entry_point: None,
        instructions_url: None,
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
    };

    let error = service
        .install_from_zip(&install, &temp.path().join("missing-hash"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("requires SHA-256"));

    install.sha256 = Some("abc123".into());
    let error = service
        .install_from_zip(&install, &temp.path().join("invalid-hash"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("invalid SHA-256"));
}

#[test]
fn archive_verification_accepts_only_the_declared_bytes() {
    let bytes = b"verified marketplace package";
    let expected = service_internals::sha256_hex(bytes);
    verify_archive_sha256(bytes, &expected, "package.zip").unwrap();

    let error = verify_archive_sha256(b"tampered", &expected, "package.zip").unwrap_err();
    assert!(matches!(error, MarketplaceError::HashMismatch { .. }));
}

#[test]
fn core_version_gate_rejects_too_new_or_invalid_requirements() {
    let mut entry = CatalogEntry {
        name: "test-skill".into(),
        description: "desc".into(),
        dcc: vec!["maya".into()],
        targets: vec![],
        url: None,
        issues_url: None,
        tags: vec![],
        version: None,
        min_core_version: Some("999.0.0".into()),
        install: None,
        package: None,
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
        targets: vec![],
        url: None,
        issues_url: None,
        tags: vec![],
        version: None,
        min_core_version: None,
        install: None,
        package: None,
        maintainer: None,
        category: None,
        policy: Some(dcc_mcp_catalog::CatalogPolicy {
            installation: "not_available".into(),
            reason: None,
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

fn catalog_entry(name: &str, dcc: &[&str]) -> CatalogEntry {
    CatalogEntry {
        name: name.into(),
        description: "desc".into(),
        dcc: dcc.iter().map(|dcc| (*dcc).to_string()).collect(),
        targets: vec![],
        url: None,
        issues_url: None,
        tags: vec![],
        version: None,
        min_core_version: None,
        install: None,
        package: None,
        maintainer: None,
        category: None,
        policy: None,
        requires: None,
        icon: None,
        showcase: None,
    }
}

#[test]
fn host_neutral_entry_installs_under_any_when_any_is_requested() {
    let entry = catalog_entry("host-neutral", &["any"]);
    assert_eq!(resolve_install_dcc(&entry, Some("any")).unwrap(), "any");
}

#[test]
fn host_neutral_entry_installs_under_any_without_an_explicit_dcc() {
    let entry = catalog_entry("host-neutral", &["any"]);
    assert_eq!(resolve_install_dcc(&entry, None).unwrap(), "any");
}

#[test]
fn host_neutral_entry_still_honours_a_concrete_dcc() {
    let entry = catalog_entry("host-neutral", &["any"]);
    assert_eq!(resolve_install_dcc(&entry, Some("Maya")).unwrap(), "maya");
}

#[test]
fn multi_host_entry_reports_supported_dccs_when_a_host_is_requested() {
    let entry = catalog_entry("multi-host", &["maya", "blender"]);
    let err = resolve_install_dcc(&entry, Some("any")).unwrap_err();

    assert!(matches!(err, MarketplaceError::DccMismatch { .. }), "{err}");
    let message = err.to_string();
    assert!(message.contains("maya, blender"), "{message}");
    // The caller already passed --dcc, so asking for it again is a dead end.
    assert!(!message.contains("pass --dcc"), "{message}");
}

#[test]
fn multi_host_entry_without_a_dcc_flag_lists_supported_hosts() {
    let entry = catalog_entry("multi-host", &["maya", "blender"]);
    let err = resolve_install_dcc(&entry, None).unwrap_err();

    assert!(
        matches!(err, MarketplaceError::AmbiguousDcc { .. }),
        "{err}"
    );
    assert!(err.to_string().contains("maya, blender"), "{err}");
    assert_eq!(resolve_install_dcc(&entry, Some("maya")).unwrap(), "maya");
}

#[tokio::test]
async fn resolve_install_hit_reports_dcc_mismatch_instead_of_not_found() {
    let temp = tempfile::tempdir().unwrap();
    let catalog_path = temp.path().join("marketplace.json");
    let catalog = serde_json::json!({
        "version": "1",
        "entries": [catalog_entry("dcc-asset-kenney", &["maya", "blender"])],
    });
    fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();
    let service = MarketplaceService::new(temp.path().join("root"));

    let err = service
        .resolve_install_hit(
            "dcc-asset-kenney",
            Some("openusd"),
            vec![catalog_path.display().to_string()],
            true,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, MarketplaceError::DccMismatch { .. }), "{err}");
    let message = err.to_string();
    assert!(message.contains("dcc-asset-kenney"), "{message}");
    assert!(message.contains("maya, blender"), "{message}");
    assert!(!message.contains("was not found"), "{message}");
}

#[tokio::test]
async fn install_places_a_host_neutral_entry_in_the_any_directory() {
    let temp = tempfile::tempdir().unwrap();
    let skill_src = temp.path().join("skill-src");
    fs::create_dir_all(&skill_src).unwrap();
    fs::write(skill_src.join("SKILL.md"), "# host neutral\n").unwrap();
    let catalog_path = temp.path().join("marketplace.json");
    let entry = serde_json::json!({
        "name": "dcc-mcp-cache-inspector",
        "description": "host neutral entry",
        "dcc": ["any"],
        "install": {
            "type": "path",
            "url": skill_src.display().to_string(),
        },
    });
    let catalog = serde_json::json!({
        "version": "1",
        "entries": [entry],
    });
    fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();
    let root = temp.path().join("root");
    let service = MarketplaceService::new(root.clone());

    let result = service
        .install(
            "dcc-mcp-cache-inspector".into(),
            None,
            vec![catalog_path.display().to_string()],
            false,
            true,
        )
        .await
        .unwrap();

    assert_eq!(result.dcc, "any");
    assert_eq!(
        result.skill_search_path,
        root.join("any").display().to_string()
    );
    assert!(root.join("any").join("dcc-mcp-cache-inspector").is_dir());
}

#[tokio::test]
async fn resolve_install_hit_still_reports_not_found_for_unknown_names() {
    let temp = tempfile::tempdir().unwrap();
    let catalog_path = temp.path().join("marketplace.json");
    let catalog = serde_json::json!({
        "version": "1",
        "entries": [catalog_entry("dcc-asset-kenney", &["maya"])],
    });
    fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();
    let service = MarketplaceService::new(temp.path().join("root"));

    let err = service
        .resolve_install_hit(
            "does-not-exist",
            None,
            vec![catalog_path.display().to_string()],
            true,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, MarketplaceError::NotFound(_)), "{err}");
}
