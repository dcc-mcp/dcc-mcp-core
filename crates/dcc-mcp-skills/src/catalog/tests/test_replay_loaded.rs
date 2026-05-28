//! Tests for `SkillCatalog::replay_loaded` (issue #1405).

use super::*;
use crate::catalog::persistence::{LoadReplayPolicy, LoadedSkillRecord, PersistedCatalogState};
use fixtures::{make_test_catalog, make_test_skill};

fn add_skill(catalog: &SkillCatalog, name: &str, version: &str, tools: &[&str]) {
    let mut meta = make_test_skill(name, "maya", tools);
    meta.version = version.to_string();
    catalog.add_skill(meta);
}

fn record(name: &str, version: Option<&str>) -> LoadedSkillRecord {
    LoadedSkillRecord {
        name: name.to_string(),
        version: version.map(str::to_string),
        skill_path: None,
        loaded_at_ms: 0,
    }
}

#[test]
fn replay_loads_known_skills() {
    let catalog = make_test_catalog();
    add_skill(&catalog, "alpha", "1.0.0", &["alpha_one"]);
    add_skill(&catalog, "beta", "1.0.0", &["beta_one"]);

    let state = PersistedCatalogState {
        skills: vec![
            record("alpha", Some("1.0.0")),
            record("beta", Some("1.0.0")),
        ],
        active_groups: vec![],
        saved_at_ms: 0,
        schema_version: 1,
    };
    let report = catalog.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert_eq!(report.loaded, vec!["alpha".to_string(), "beta".to_string()]);
    assert!(report.missing.is_empty());
    assert!(report.skipped_drift.is_empty());
    assert!(report.failed.is_empty());
    assert!(catalog.is_loaded("alpha"));
    assert!(catalog.is_loaded("beta"));
}

#[test]
fn replay_records_missing_skill() {
    let catalog = make_test_catalog();
    add_skill(&catalog, "alpha", "1.0.0", &["alpha_one"]);

    let state = PersistedCatalogState {
        skills: vec![
            record("alpha", Some("1.0.0")),
            record("gone", Some("1.0.0")),
        ],
        active_groups: vec![],
        saved_at_ms: 0,
        schema_version: 1,
    };
    let report = catalog.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert_eq!(report.loaded, vec!["alpha".to_string()]);
    assert_eq!(report.missing, vec!["gone".to_string()]);
}

#[test]
fn replay_skips_on_version_drift_by_default() {
    let catalog = make_test_catalog();
    add_skill(&catalog, "alpha", "2.0.0", &["alpha_one"]);

    let state = PersistedCatalogState {
        skills: vec![record("alpha", Some("1.0.0"))],
        active_groups: vec![],
        saved_at_ms: 0,
        schema_version: 1,
    };
    let report = catalog.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert!(report.loaded.is_empty());
    assert_eq!(report.skipped_drift.len(), 1);
    assert_eq!(report.skipped_drift[0].name, "alpha");
    assert_eq!(
        report.skipped_drift[0].persisted_version.as_deref(),
        Some("1.0.0")
    );
    assert_eq!(report.skipped_drift[0].current_version, "2.0.0");
    assert!(!catalog.is_loaded("alpha"));
}

#[test]
fn replay_ignore_version_loads_drift() {
    let catalog = make_test_catalog();
    add_skill(&catalog, "alpha", "2.0.0", &["alpha_one"]);

    let state = PersistedCatalogState {
        skills: vec![record("alpha", Some("1.0.0"))],
        active_groups: vec![],
        saved_at_ms: 0,
        schema_version: 1,
    };
    let report = catalog.replay_loaded(&state, LoadReplayPolicy::IgnoreVersion);

    assert_eq!(report.loaded, vec!["alpha".to_string()]);
    assert!(report.skipped_drift.is_empty());
    assert!(catalog.is_loaded("alpha"));
}

#[test]
fn replay_restores_active_groups() {
    let catalog = make_test_catalog();
    add_skill(&catalog, "alpha", "1.0.0", &["alpha_one"]);

    let state = PersistedCatalogState {
        skills: vec![record("alpha", Some("1.0.0"))],
        active_groups: vec!["rigging".to_string(), "animation".to_string()],
        saved_at_ms: 0,
        schema_version: 1,
    };
    let report = catalog.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert_eq!(report.loaded, vec!["alpha".to_string()]);
    assert_eq!(
        report.activated_groups,
        vec!["rigging".to_string(), "animation".to_string()]
    );
    let mut current = catalog.active_groups();
    current.sort();
    assert_eq!(
        current,
        vec!["animation".to_string(), "rigging".to_string()]
    );
}

#[test]
fn replay_persisted_record_without_version_loads_unconditionally() {
    // Records persisted by older code paths may not carry a version.
    // The replay must still attempt the load — drift only triggers when
    // both sides have a version to compare.
    let catalog = make_test_catalog();
    add_skill(&catalog, "alpha", "2.0.0", &["alpha_one"]);

    let state = PersistedCatalogState {
        skills: vec![record("alpha", None)],
        active_groups: vec![],
        saved_at_ms: 0,
        schema_version: 1,
    };
    let report = catalog.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert_eq!(report.loaded, vec!["alpha".to_string()]);
    assert!(catalog.is_loaded("alpha"));
}
