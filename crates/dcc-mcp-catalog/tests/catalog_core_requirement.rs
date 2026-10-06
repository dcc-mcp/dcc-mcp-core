//! Catalog validation for the core version-bound contract.
//!
//! `dcc-mcp-core` is `0.x`: a minor bump may break an adapter, so a catalog
//! entry's `core_requirement` must name both bounds and admit a single minor
//! line. See `compatibility/core-bounds.json` and
//! `docs/guide/adapter-core-version-contract.md`.

use dcc_mcp_catalog::{load_from_str, validate_catalog_entries, validate_entry};

fn catalog(core_requirement: &str) -> String {
    format!(
        r#"
version: "1"
entries:
  - name: "dcc-mcp-example"
    description: "Example adapter"
    dcc: ["example"]
    version: "1.0.0"
    min_core_version: "0.19.45"
    core_requirement: "{core_requirement}"
"#
    )
}

#[test]
fn bounded_core_requirement_passes() {
    let entries = load_from_str(&catalog(">=0.19.45,<0.20.0")).expect("catalog must load");
    assert_eq!(
        entries[0].core_requirement.as_deref(),
        Some(">=0.19.45,<0.20.0")
    );
    assert!(validate_catalog_entries(&entries).is_ok());
}

#[test]
fn floor_without_core_requirement_passes() {
    // `min_core_version` is a floor, not a range: the adapter's real upper
    // bound is not core's to invent.
    let yaml = r#"
version: "1"
entries:
  - name: "dcc-mcp-example"
    description: "Example adapter"
    dcc: ["example"]
    version: "1.0.0"
    min_core_version: "0.19.45"
"#;
    let entries = load_from_str(yaml).expect("catalog must load");
    assert!(entries[0].core_requirement.is_none());
    assert!(validate_catalog_entries(&entries).is_ok());
}

#[test]
fn wide_core_requirement_is_rejected_with_a_suggestion() {
    // `>=0.19.45,<1.0.0` is the legacy shape: it admits every breaking 0.x
    // minor line, which is exactly what the bound contract forbids.
    let entries = load_from_str(&catalog(">=0.19.45,<1.0.0")).expect("catalog must load");
    // `validate_entry` carries the per-entry detail; the aggregate error only
    // counts failures.
    assert!(validate_catalog_entries(&entries).is_err());
    let error = validate_entry(&entries[0]).unwrap_err().to_string();
    assert!(
        error.contains("upper bound admits more than one core minor line"),
        "{error}"
    );
    assert!(
        error.contains(">=0.19.45,<0.20.0"),
        "{error} should suggest a bounded range"
    );
}

#[test]
fn unbounded_environment_style_request_is_rejected() {
    let entries = load_from_str(&catalog("dcc_mcp_core-0")).expect("catalog must load");
    let error = validate_entry(&entries[0]).unwrap_err().to_string();
    assert!(
        error.contains("upper bound admits more than one core minor line"),
        "{error}"
    );
}

#[test]
fn alias_form_deserializes_into_core_requirement() {
    let yaml = r#"
version: "1"
entries:
  - name: "dcc-mcp-example"
    description: "Example adapter"
    dcc: ["example"]
    version: "1.0.0"
    coreRequirement: ">=0.20.14,<0.21.0"
"#;
    let entries = load_from_str(yaml).expect("catalog must load");
    assert_eq!(
        entries[0].core_requirement.as_deref(),
        Some(">=0.20.14,<0.21.0")
    );
    assert!(validate_catalog_entries(&entries).is_ok());
}
