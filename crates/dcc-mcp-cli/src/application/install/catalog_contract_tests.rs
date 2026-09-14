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
