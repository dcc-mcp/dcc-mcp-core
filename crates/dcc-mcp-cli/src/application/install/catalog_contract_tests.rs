//! Service contracts after the separate cryptographic verification boundary.
use super::*;
use crate::domain::install_catalog::CatalogProvenance;

fn request() -> InstallRequest {
    InstallRequest {
        dcc_type: "godot".into(),
        version: None,
        catalog_path: None,
        python: None,
        dcc_path: None,
        plugin_source: None,
        adobe_debug_root: None,
    }
}

fn service(source: CatalogSource, expires_at: u64) -> InstallService {
    let mut entries = dcc_mcp_catalog::load_from_str(BUNDLED_CATALOG).unwrap();
    let entry = entries
        .iter_mut()
        .find(|entry| entry.name == "dcc-mcp-godot")
        .unwrap();
    let old_version = entry.version.replace("7.7.7".into()).unwrap();
    let install = entry.install.as_mut().unwrap();
    install.url = install
        .url
        .as_ref()
        .map(|url| url.replace(&old_version, "7.7.7"));
    let latest_checked = source == CatalogSource::Remote;
    let mut service = InstallService::bundled();
    service.require_fresh_catalog = true;
    service.auto_install_policy.enabled = true;
    service.catalog_snapshot = Some(catalog::ResolvedCatalog {
        entries,
        provenance: CatalogProvenance {
            latest_checked,
            issued_at: Some(1),
            expires_at: Some(expires_at),
            sha256: Some("a".repeat(64)),
            source_revision: Some("b".repeat(40)),
            source,
        },
    });
    service
}

#[test]
fn discovery_and_plan_share_the_refreshed_version_and_locked_artifact() {
    let service = service(CatalogSource::Remote, u64::MAX);
    let listing = service.dcc_types(None).unwrap();
    let godot = listing
        .dcc_types
        .iter()
        .find(|item| item.dcc_type == "godot")
        .unwrap();
    assert_eq!(godot.adapters[0].version.as_deref(), Some("7.7.7"));
    let plan = service.plan(request()).unwrap();
    assert_eq!(plan.version.as_deref(), Some("7.7.7"));
    assert!(
        plan.adapter
            .install
            .unwrap()
            .url
            .unwrap()
            .contains("7.7.7-py3-none-any.whl")
    );
    assert!(plan.catalog.unwrap().latest_checked);
}

#[test]
fn default_catalog_fallback_reports_bundled_and_cannot_bypass_freshness() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("catalog.json");
    for contents in [None, Some(r#"{"entries":[]}"#)] {
        if let Some(contents) = contents {
            std::fs::write(&path, contents).unwrap();
        }
        let mut service = InstallService::new(path.clone());
        service.require_fresh_catalog = true;
        service.auto_install_policy.enabled = true;
        let plan = service.plan(request()).unwrap();
        let provenance = plan.catalog.as_ref().unwrap();
        assert_eq!(provenance.source, CatalogSource::Bundled);
        assert!(!provenance.latest_checked);
        assert!(provenance.sha256.is_some());
        assert_eq!(service.catalog_provenance(None), *provenance);
        let report = service.execute_plan_with(
            &plan,
            true,
            |_, _| panic!("fallback must not execute"),
            |_| panic!("no rollback"),
        );
        assert_eq!(report.error.unwrap().code, "INSTALL_CATALOG_UNAVAILABLE");
    }
}

#[test]
fn existing_default_and_requested_catalogs_report_explicit_source() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("catalog.yml");
    std::fs::write(&path, BUNDLED_CATALOG).unwrap();
    let default_plan = InstallService::new(path.clone()).plan(request()).unwrap();
    assert_eq!(
        default_plan.catalog.unwrap().source,
        CatalogSource::Explicit
    );
    let mut explicit = request();
    explicit.catalog_path = Some(path);
    let refreshed = service(CatalogSource::Remote, u64::MAX);
    let explicit_plan = refreshed.plan(explicit).unwrap();
    assert_eq!(
        explicit_plan.catalog.unwrap().source,
        CatalogSource::Explicit
    );
    assert_eq!(explicit_plan.version, default_plan.version);
    let mut missing = request();
    missing.catalog_path = Some(root.path().join("missing.yml"));
    assert!(refreshed.plan(missing).is_err());
}

#[test]
fn invalid_refresh_never_silently_uses_bundled_metadata() {
    let mut service = InstallService::bundled();
    service.catalog_error = Some("signature verification failed".into());
    assert!(service.plan(request()).is_err());
    assert!(service.dcc_types(None).is_err());
    let report = service.execute(request(), true);
    assert_eq!(report.stage, "preflight");
    assert!(report.steps.is_empty());
    assert_eq!(report.error.unwrap().code, "INSTALL_CATALOG_UNAVAILABLE");
    assert_eq!(
        report.catalog.unwrap(),
        CatalogProvenance::local(CatalogSource::Unavailable)
    );
    assert_eq!(
        service.catalog_provenance(None).source,
        CatalogSource::Unavailable
    );
}

#[test]
fn implicit_offline_fallback_cannot_execute_without_explicit_offline_selection() {
    let mut service = service(CatalogSource::Cache, u64::MAX);
    let plan = service.plan(request()).unwrap();
    let report = service.execute_plan_with(
        &plan,
        true,
        |_, _| panic!("no mutation"),
        |_| panic!("no rollback"),
    );
    assert_eq!(report.error.unwrap().code, "INSTALL_CATALOG_UNAVAILABLE");
    service.require_fresh_catalog = false;
    let report = service.execute_plan_with(
        &plan,
        true,
        |_, _| Ok(StepExecution::Deferred),
        |_| panic!("no rollback"),
    );
    assert_eq!(report.stage, "complete");
    assert_eq!(report.catalog.unwrap().source, CatalogSource::Cache);
}

#[test]
fn expiry_is_rechecked_before_the_first_mutation() {
    let service = service(CatalogSource::Remote, 1);
    let plan = service.plan(request()).unwrap();
    let report = service.execute_plan_with(
        &plan,
        true,
        |_, _| panic!("no mutation"),
        |_| panic!("no rollback"),
    );
    assert_eq!(report.error.unwrap().code, "INSTALL_CATALOG_EXPIRED");
    assert_eq!(report.exit_code, 10);
}

fn local_pip_entry(name: &str, dcc_type: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": format!("{dcc_type} adapter fixture"),
        "dcc": [dcc_type],
        "tags": ["adapter"],
        "version": "1.0.0",
        "install": {
            "type": "pip",
            "pip_package": name,
            "url": format!(
                "https://example.invalid/{}-1.0.0-py3-none-any.whl",
                name.replace('-', "_")
            ),
            "sha256": "a".repeat(64)
        }
    })
}

fn invalid_local_catalogs() -> Vec<(&'static str, serde_json::Value)> {
    let selected = local_pip_entry("dcc-mcp-godot", "godot");
    let other = local_pip_entry("dcc-mcp-blender", "blender");
    let mut bad_digest = other.clone();
    bad_digest["install"]["sha256"] = serde_json::json!("not-a-digest");
    let mut wrong_wheel = other.clone();
    wrong_wheel["version"] = serde_json::json!("2.0.0");
    let mut invalid_requirement = other.clone();
    invalid_requirement["core_requirement"] = serde_json::json!(">=0.20.0,<1.0.0");
    let mut empty_description = other;
    empty_description["description"] = serde_json::json!("");
    let mut duplicate = selected.clone();
    duplicate["name"] = serde_json::json!("DCC-MCP-GODOT");
    [
        ("bad-digest", bad_digest),
        ("wrong-wheel", wrong_wheel),
        ("invalid-requirement", invalid_requirement),
        ("empty-description", empty_description),
        ("duplicate-name", duplicate),
    ]
    .into_iter()
    .map(|(name, invalid)| {
        (
            name,
            serde_json::json!({"entries": [selected.clone(), invalid]}),
        )
    })
    .collect()
}

fn write_local_catalog(path: &Path, document: &serde_json::Value, yaml: bool) {
    let contents = if yaml {
        serde_yaml_ng::to_string(document).unwrap()
    } else {
        serde_json::to_string(document).unwrap()
    };
    std::fs::write(path, contents).unwrap();
}

#[test]
fn invalid_local_entries_fail_before_planning_even_when_not_selected() {
    let root = tempfile::tempdir().unwrap();
    for yaml in [false, true] {
        for (case, document) in invalid_local_catalogs() {
            let path = root.path().join(format!("{case}-{yaml}.catalog"));
            write_local_catalog(&path, &document, yaml);
            for explicit in [false, true] {
                let local = if explicit {
                    service(CatalogSource::Remote, u64::MAX)
                } else {
                    InstallService::new(path.clone())
                };
                let requested_path = explicit.then_some(path.as_path());
                assert!(
                    matches!(
                        local.load_entries(requested_path),
                        Err(InstallError::Catalog(_))
                    ),
                    "{case}, yaml={yaml}, explicit={explicit}"
                );
                assert!(local.dcc_types(requested_path).is_err());
                let mut selected = request();
                selected.catalog_path = requested_path.map(Path::to_path_buf);
                assert!(matches!(
                    local.plan(selected),
                    Err(InstallError::Catalog(_))
                ));
                assert_eq!(
                    local.catalog_provenance(requested_path).source,
                    CatalogSource::Unavailable
                );
            }
        }
    }
}

#[test]
fn invalid_local_entries_return_preflight_reports_without_actions_or_rollback() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("catalog.json");
    for (case, document) in invalid_local_catalogs() {
        write_local_catalog(&path, &document, false);
        for explicit in [false, true] {
            let mut local = InstallService::new(path.clone());
            local.auto_install_policy.enabled = true;
            let mut selected = request();
            selected.catalog_path = explicit.then_some(path.clone());
            selected.python = Some(root.path().join("must-not-launch").display().to_string());
            let report = local.execute(selected, true);
            assert_eq!(report.exit_code, 10, "{case}, explicit={explicit}");
            assert_eq!(report.stage, "preflight");
            assert_eq!(report.status, "failed");
            assert!(report.steps.is_empty());
            assert!(!report.rollback.attempted);
            assert!(report.receipt_path.is_none());
            assert!(!report.verify.directly_usable);
            assert_eq!(report.error.unwrap().code, "INSTALL_PLAN_FAILED");
        }
    }
}

#[test]
fn valid_local_catalog_aliases_and_floor_only_entries_keep_their_contract() {
    let root = tempfile::tempdir().unwrap();
    for yaml in [false, true] {
        for collection in ["entries", "items", "skills"] {
            for bounded in [false, true] {
                let mut entry = local_pip_entry("dcc-mcp-godot", "godot");
                let object = entry.as_object_mut().unwrap();
                let install = object.remove("install").unwrap();
                object.insert("source".into(), install);
                object.insert("minCoreVersion".into(), serde_json::json!("0.20.0"));
                object.insert(
                    "issuesUrl".into(),
                    serde_json::json!("https://github.com/dcc-mcp/dcc-mcp-godot/issues"),
                );
                if bounded {
                    object.insert(
                        "coreRequirement".into(),
                        serde_json::json!(">=0.20.0,<0.21.0"),
                    );
                }
                let mut document = serde_json::json!({"version": "1"});
                document[collection] = serde_json::json!([
                    entry,
                    {"name": "catalog-only", "description": "Discovery-only package"}
                ]);
                let path = root.path().join("compatible.catalog");
                write_local_catalog(&path, &document, yaml);
                let local = InstallService::new(path.clone());
                for explicit in [false, true] {
                    let requested_path = explicit.then_some(path.as_path());
                    let entries = local.load_entries(requested_path).unwrap();
                    assert_eq!(entries.len(), 2);
                    assert!(entries[1].install.is_none());
                    assert_eq!(entries[0].core_requirement.is_some(), bounded);
                    let mut selected = request();
                    selected.catalog_path = requested_path.map(Path::to_path_buf);
                    let plan = local.plan(selected).unwrap();
                    assert_eq!(plan.version.as_deref(), Some("1.0.0"));
                    assert_eq!(plan.adapter.min_core_version.as_deref(), Some("0.20.0"));
                    assert_eq!(plan.catalog.unwrap().source, CatalogSource::Explicit);
                }
            }
        }
    }
}

#[test]
fn bundled_local_catalog_is_valid_and_preserves_its_provenance() {
    let local = InstallService::bundled();
    let entries = local.load_entries(None).unwrap();
    dcc_mcp_catalog::validate_catalog_entries(&entries).unwrap();
    assert!(!entries.is_empty());
    let provenance = local.catalog_provenance(None);
    assert_eq!(provenance.source, CatalogSource::Bundled);
    assert!(provenance.sha256.is_some());
    assert!(!provenance.latest_checked);
}

#[test]
fn generic_parser_stays_permissive_but_a_local_collection_typo_cannot_fall_back() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("typo.catalog");
    let document = serde_json::json!({
        "version": "1",
        "entires": [local_pip_entry("dcc-mcp-godot", "godot")]
    });
    for yaml in [false, true] {
        write_local_catalog(&path, &document, yaml);
        // The generic decoder intentionally retains its existing behavior.
        // Before the local shape gate, this empty result selected bundled data.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(dcc_mcp_catalog::load_from_str(&text).unwrap().is_empty());
        let local = InstallService::new(path.clone());
        assert!(matches!(
            local.plan(request()),
            Err(InstallError::Catalog(_))
        ));
        assert_eq!(
            local.catalog_provenance(None).source,
            CatalogSource::Unavailable
        );
    }
}

#[test]
fn malformed_local_document_shapes_fail_before_planning_or_execution() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("malformed.catalog");
    let entry = local_pip_entry("dcc-mcp-godot", "godot");
    for document in [
        serde_json::json!({}),
        serde_json::json!({"version": "1"}),
        serde_json::json!({"entries": [entry.clone()], "items": []}),
        serde_json::json!({"entries": null}),
        serde_json::json!({"entries": entry.clone()}),
        serde_json::json!(["1", [entry]]),
    ] {
        for yaml in [false, true] {
            write_local_catalog(&path, &document, yaml);
            if document.get("items").is_some() {
                // Multiple aliases already fail in the generic decoder. Keep
                // that error path distinct from the missing-collection bug.
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(dcc_mcp_catalog::load_from_str(&text).is_err());
            }
            for explicit in [false, true] {
                let mut local = InstallService::new(path.clone());
                local.auto_install_policy.enabled = true;
                let requested_path = explicit.then_some(path.as_path());
                assert!(matches!(
                    local.load_entries(requested_path),
                    Err(InstallError::Catalog(_))
                ));
                assert!(local.dcc_types(requested_path).is_err());
                let mut selected = request();
                selected.catalog_path = requested_path.map(Path::to_path_buf);
                selected.python = Some(root.path().join("must-not-launch").display().to_string());
                let report = local.execute(selected, true);
                assert_eq!(report.exit_code, 10);
                assert_eq!(report.stage, "preflight");
                assert!(report.steps.is_empty());
                assert!(!report.rollback.attempted);
                assert!(report.receipt_path.is_none());
                assert!(!report.verify.directly_usable);
                assert_eq!(report.error.unwrap().code, "INSTALL_PLAN_FAILED");
            }
        }
    }
}

#[test]
fn local_document_metadata_and_discovery_only_entries_keep_their_source() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("discovery.catalog");
    for yaml in [false, true] {
        for collection in ["entries", "items", "skills"] {
            let mut document = serde_json::json!({
                "name": "dcc-mcp-official",
                "version": "1.0.0",
                "$schema": "https://example.invalid/catalog.schema.json",
                "$comment": "Informational catalog annotation",
                "metadata": {"label": "legacy metadata"}
            });
            document[collection] = serde_json::json!([{
                "name": "catalog-only",
                "description": "Discovery-only adapter",
                "dcc": ["godot"],
                "tags": ["adapter"]
            }]);
            write_local_catalog(&path, &document, yaml);
            if yaml {
                let text = std::fs::read_to_string(&path).unwrap();
                let text = format!("{text}extra_yaml_metadata:\n  42: retained\n");
                assert!(dcc_mcp_catalog::load_from_str(&text).is_ok());
                std::fs::write(&path, text).unwrap();
            }
            let local = InstallService::new(path.clone());
            for explicit in [false, true] {
                let requested_path = explicit.then_some(path.as_path());
                let entries = local.load_entries(requested_path).unwrap();
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "catalog-only");
                assert!(entries[0].install.is_none());
                assert_eq!(
                    local.catalog_provenance(requested_path).source,
                    CatalogSource::Explicit
                );
                assert!(local.dcc_types(requested_path).is_ok());
            }
            document[collection] = serde_json::json!([]);
            write_local_catalog(&path, &document, yaml);
            assert!(local.load_entries(Some(&path)).unwrap().is_empty());
            assert_eq!(
                local.catalog_provenance(None).source,
                CatalogSource::Bundled
            );
        }
    }
}

#[test]
fn yaml_tags_keep_the_generic_parsers_acceptance() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tagged.yml");
    let mut documents = Vec::new();
    for (document_tag, collection_tag) in [
        ("", ""),
        ("!catalog\n", ""),
        ("", "!collection "),
        ("!catalog\n", "!collection "),
    ] {
        documents.push(format!(
            "{document_tag}entries: {collection_tag}\n  - name: catalog-only\n    description: Discovery-only entry\n"
        ));
    }
    documents.push("entries: []\nmetadata: !note\n  42: retained\n".into());
    for text in documents {
        std::fs::write(&path, &text).unwrap();
        let local = InstallService::new(path.clone());
        match dcc_mcp_catalog::load_from_str(&text) {
            Ok(entries) => assert_eq!(local.load_entries(Some(&path)).unwrap(), entries),
            Err(_) => assert!(local.load_entries(Some(&path)).is_err()),
        }
    }
}

#[test]
fn local_file_read_errors_are_not_mistaken_for_an_empty_catalog() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("catalog-directory");
    std::fs::create_dir(&directory).unwrap();
    let non_utf8 = root.path().join("non-utf8.catalog");
    std::fs::write(&non_utf8, [0xff, 0xfe]).unwrap();
    for path in [directory, non_utf8] {
        let local = InstallService::new(path.clone());
        for explicit in [false, true] {
            let requested_path = explicit.then_some(path.as_path());
            assert!(matches!(
                local.load_entries(requested_path),
                Err(InstallError::Catalog(dcc_mcp_catalog::CatalogError::Io(_, _)))
            ));
            assert_eq!(
                local.catalog_provenance(requested_path).source,
                CatalogSource::Unavailable
            );
        }
    }
}
