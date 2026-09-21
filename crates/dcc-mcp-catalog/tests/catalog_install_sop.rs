//! Integration tests for the Install SOP metadata carried by `CatalogInstall`.
//!
//! `sop_version` / `sop_schema_digest` let a consumer discover which Install SOP
//! schema version an adapter's runbook targets and pin the exact schema bytes.
//! These tests live outside `src/lib.rs` to keep that file within the
//! repository's 1500-line production Rust limit.

use std::fs;
use std::path::Path;

use dcc_mcp_catalog::{
    CatalogEntry, CatalogInstall, load_from_file, load_from_str, validate_entry,
};
use sha2::{Digest, Sha256};

/// The digest of the Install SOP schema document (`adapter-install-sop-v1`),
/// matching `INSTALL_SOP_SCHEMA_VERSION = 1` in
/// `dcc_mcp_core.deployment.install_sop`.
///
/// Mirrored by the same constant in `src/lib.rs`, which the bundled-catalog test
/// there compares against `dcc-mcp-catalog.yml`. Since that YAML is also
/// asserted here to equal the SHA-256 of the real schema file, both copies stay
/// anchored to the same bytes.
const SOP_V1_DIGEST: &str = "2b3a8a101384a5163c7569c4a2b0de6586c672c5ee291735f94334a33b7d37a0";

fn sop_entry(version: Option<u32>, digest: Option<&str>) -> CatalogEntry {
    CatalogEntry {
        name: "sop-adapter".into(),
        description: "An adapter reporting Install SOP metadata".into(),
        dcc: vec!["maya".into()],
        targets: vec![],
        url: None,
        issues_url: None,
        tags: vec![],
        version: Some("1.0.0".into()),
        min_core_version: None,
        package: None,
        maintainer: Some("dcc-mcp".into()),
        category: None,
        policy: None,
        requires: None,
        install: Some(CatalogInstall {
            install_type: "pip".into(),
            url: Some(
                "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-1.0.0-py3-none-any.whl"
                    .into(),
            ),
            ref_: None,
            sha256: Some("a".repeat(64)),
            skill_roots: None,
            pip_package: Some("dcc-mcp-maya".into()),
            pip_extras: None,
            python_path: None,
            entry_point: None,
            instructions_url: None,
            sop_version: version,
            sop_schema_digest: digest.map(str::to_string),
            adobe: None,
        }),
        icon: None,
        showcase: None,
    }
}

#[test]
fn test_validate_entry_accepts_install_sop_metadata() {
    let entry = sop_entry(Some(2), Some(SOP_V1_DIGEST));
    assert!(validate_entry(&entry).is_ok());

    let install = entry.install.as_ref().unwrap();
    assert_eq!(install.sop_version, Some(2));
    assert_eq!(install.sop_schema_digest.as_deref(), Some(SOP_V1_DIGEST));
}

#[test]
fn test_validate_entry_accepts_sop_metadata_without_digest() {
    // `sop_version` alone stays valid so adapters can adopt it incrementally.
    assert!(validate_entry(&sop_entry(Some(1), None)).is_ok());
}

#[test]
fn test_validate_entry_rejects_malformed_sop_schema_digest() {
    for digest in [
        String::new(),
        "abc123".to_string(),
        SOP_V1_DIGEST.to_uppercase(),
        SOP_V1_DIGEST.replace('2', "g"),
        format!("{SOP_V1_DIGEST}00"),
    ] {
        let entry = sop_entry(Some(1), Some(&digest));
        let err = validate_entry(&entry).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("sop_schema_digest"),
            "digest {digest:?} should be rejected, got: {message}"
        );
    }
}

#[test]
fn test_validate_entry_rejects_non_positive_sop_version() {
    // Version 0 is not a published schema version.
    assert!(validate_entry(&sop_entry(Some(0), Some(SOP_V1_DIGEST))).is_err());
}

#[test]
fn test_load_marketplace_json_with_install_sop_metadata() {
    let json = r#"
{
  "version": "1",
  "entries": [{
"name": "dcc-mcp-maya-sop",
"description": "Maya adapter reporting Install SOP metadata",
"dcc": ["maya"],
"version": "1.0.0",
"install": {
  "type": "pip",
  "pip_package": "dcc-mcp-maya",
  "url": "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-1.0.0-py3-none-any.whl",
  "sha256": "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90",
  "sop_version": 2,
  "sop_schema_digest": "2b3a8a101384a5163c7569c4a2b0de6586c672c5ee291735f94334a33b7d37a0"
}
  }]
}
"#;
    let entries = load_from_str(json).unwrap();
    let install = entries[0].install.as_ref().unwrap();
    assert_eq!(install.sop_version, Some(2));
    assert_eq!(install.sop_schema_digest.as_deref(), Some(SOP_V1_DIGEST));
    assert!(validate_entry(&entries[0]).is_ok());
}

#[test]
fn test_non_integer_sop_version_is_rejected() {
    // A typed entry cannot hold a string, so the raw document is rejected at
    // parse time instead of during `validate_entry`.
    let json = r#"
{
  "version": "1",
  "entries": [{
"name": "dcc-mcp-maya-bad-sop",
"description": "Maya adapter with a mistyped SOP version",
"dcc": ["maya"],
"install": {
  "type": "pip",
  "pip_package": "dcc-mcp-maya",
  "sop_version": "x"
}
  }]
}
"#;
    assert!(
        load_from_str(json).is_err(),
        "a string sop_version must not deserialize into Option<u32>"
    );
}

/// `sop_schema_digest` exists to pin schema bytes, so it must be verified against
/// the real schema document — comparing a YAML value against a Rust constant
/// only proves the two hand-written copies agree.
#[test]
fn bundled_sop_schema_digest_matches_the_published_schema_file() {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../python/dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json");
    let schema_bytes = fs::read(&schema_path)
        .unwrap_or_else(|err| panic!("reading {}: {err}", schema_path.display()));
    let actual_digest = Sha256::digest(&schema_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    assert_eq!(
        actual_digest, SOP_V1_DIGEST,
        "SOP_V1_DIGEST is stale; the Install SOP schema document changed"
    );

    let catalog_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dcc-mcp-catalog.yml");
    let entries = load_from_file(&catalog_path).expect("the bundled catalog must parse");

    let published: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|entry| {
            entry
                .install
                .as_ref()
                .is_some_and(|install| install.sop_schema_digest.is_some())
        })
        .collect();
    assert!(
        !published.is_empty(),
        "the bundled catalog must publish at least one sop_schema_digest for this test to guard"
    );

    for entry in published {
        let digest = entry.install.as_ref().unwrap().sop_schema_digest.as_deref();
        assert_eq!(
            digest,
            Some(actual_digest.as_str()),
            "{} pins a sop_schema_digest that no longer matches the schema document",
            entry.name
        );
    }
}
