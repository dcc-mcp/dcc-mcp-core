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

fn add_grouped_skill(catalog: &SkillCatalog, name: &str, group: &str) {
    let mut skill = make_test_skill(name, "maya", &[]);
    skill.groups = vec![SkillGroup {
        name: group.to_string(),
        ..Default::default()
    }];
    skill.tools = vec![dcc_mcp_models::ToolDeclaration {
        name: "inspect".to_string(),
        group: group.to_string(),
        ..Default::default()
    }];
    catalog.add_skill(skill);
}

#[test]
fn replay_restores_scoped_group_without_enabling_sibling_skill() {
    let catalog = make_test_catalog();
    for name in ["alpha", "beta"] {
        let mut skill = make_test_skill(name, "maya", &[]);
        skill.groups = vec![SkillGroup {
            name: "inspection".to_string(),
            ..Default::default()
        }];
        skill.tools = vec![dcc_mcp_models::ToolDeclaration {
            name: "inspect".to_string(),
            group: "inspection".to_string(),
            ..Default::default()
        }];
        catalog.add_skill(skill);
    }

    let state = PersistedCatalogState {
        skills: vec![
            record("alpha", Some("1.0.0")),
            record("beta", Some("1.0.0")),
        ],
        active_groups: vec!["alpha:inspection".to_string()],
        saved_at_ms: 0,
        schema_version: 1,
    };
    catalog.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert!(
        catalog
            .registry()
            .get_action("alpha__inspect", None)
            .is_some_and(|tool| tool.enabled)
    );
    assert!(
        catalog
            .registry()
            .get_action("beta__inspect", None)
            .is_some_and(|tool| !tool.enabled)
    );
}

#[test]
fn replay_skips_scoped_groups_for_skills_that_are_not_loaded() {
    let catalog = make_test_catalog();
    add_grouped_skill(&catalog, "alpha", "inspection");
    let state = PersistedCatalogState {
        skills: vec![],
        active_groups: vec!["alpha:inspection".to_string()],
        saved_at_ms: 0,
        schema_version: 1,
    };

    let report = catalog.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert!(report.activated_groups.is_empty());
    assert!(catalog.active_group_keys().is_empty());
    catalog.load_skill_with_options("alpha", false).unwrap();
    assert!(
        catalog
            .registry()
            .get_action("alpha__inspect", None)
            .is_some_and(|tool| !tool.enabled)
    );
    assert_eq!(
        catalog
            .list_groups()
            .into_iter()
            .find(|(skill, group, _)| skill == "alpha" && group == "inspection")
            .map(|(_, _, active)| active),
        Some(false)
    );
}

#[test]
fn scoped_deactivate_migrates_legacy_bare_group_before_snapshot_replay() {
    let persisted = PersistedCatalogState {
        skills: vec![
            record("alpha", Some("1.0.0")),
            record("beta", Some("1.0.0")),
        ],
        active_groups: vec!["inspection".to_string()],
        saved_at_ms: 0,
        schema_version: 1,
    };
    let catalog = make_test_catalog();
    for name in ["alpha", "beta"] {
        add_grouped_skill(&catalog, name, "inspection");
    }
    catalog.replay_loaded(&persisted, LoadReplayPolicy::SkipOnDrift);
    let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = changes.clone();
    catalog.set_after_scoped_group_change_hook(move |key, active| {
        observed.lock().unwrap().push((key.to_string(), active));
        Ok(())
    });

    catalog.deactivate_skill_group("alpha", "inspection");

    assert!(
        catalog
            .registry()
            .get_action("alpha__inspect", None)
            .is_some_and(|tool| !tool.enabled)
    );
    assert!(
        catalog
            .registry()
            .get_action("beta__inspect", None)
            .is_some_and(|tool| tool.enabled)
    );
    assert_eq!(
        catalog.active_group_keys(),
        vec!["beta:inspection".to_string()]
    );
    let changes = changes.lock().unwrap();
    assert!(changes.contains(&("beta:inspection".to_string(), true)));
    assert!(changes.contains(&("inspection".to_string(), false)));
    drop(changes);

    let snapshot = PersistedCatalogState {
        skills: persisted.skills.clone(),
        active_groups: catalog.active_group_keys(),
        saved_at_ms: 0,
        schema_version: 1,
    };
    let replayed = make_test_catalog();
    for name in ["alpha", "beta"] {
        add_grouped_skill(&replayed, name, "inspection");
    }
    replayed.replay_loaded(&snapshot, LoadReplayPolicy::SkipOnDrift);
    assert!(
        replayed
            .registry()
            .get_action("alpha__inspect", None)
            .is_some_and(|tool| !tool.enabled)
    );
    assert!(
        replayed
            .registry()
            .get_action("beta__inspect", None)
            .is_some_and(|tool| tool.enabled)
    );
}

#[test]
fn default_active_group_persists_without_enabling_same_named_sibling() {
    fn grouped_skill(name: &str, default_active: bool) -> SkillMetadata {
        let mut skill = make_test_skill(name, "maya", &[]);
        skill.groups = vec![SkillGroup {
            name: "core".to_string(),
            default_active,
            ..Default::default()
        }];
        skill.tools = vec![dcc_mcp_models::ToolDeclaration {
            name: "inspect".to_string(),
            group: "core".to_string(),
            ..Default::default()
        }];
        skill
    }

    let source = make_test_catalog();
    source.add_skill(grouped_skill("alpha", true));
    source.add_skill(grouped_skill("beta", false));
    source.load_skill_with_options("alpha", false).unwrap();
    source.load_skill_with_options("beta", false).unwrap();
    assert!(source.active_groups().contains(&"core".to_string()));
    assert!(
        source
            .active_group_keys()
            .contains(&"alpha:core".to_string())
    );

    let state = PersistedCatalogState {
        skills: vec![
            record("alpha", Some("1.0.0")),
            record("beta", Some("1.0.0")),
        ],
        active_groups: source.active_group_keys(),
        saved_at_ms: 0,
        schema_version: 1,
    };
    let replayed = make_test_catalog();
    replayed.add_skill(grouped_skill("alpha", true));
    replayed.add_skill(grouped_skill("beta", false));
    replayed.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert!(
        replayed
            .registry()
            .get_action("alpha__inspect", None)
            .is_some_and(|tool| tool.enabled)
    );
    assert!(
        replayed
            .registry()
            .get_action("beta__inspect", None)
            .is_some_and(|tool| !tool.enabled)
    );
}

#[test]
fn replay_empty_active_groups_override_skill_defaults() {
    let catalog = make_test_catalog();
    let mut skill = make_test_skill("alpha", "maya", &[]);
    skill.groups = vec![SkillGroup {
        name: "core".to_string(),
        default_active: true,
        ..Default::default()
    }];
    skill.tools = vec![dcc_mcp_models::ToolDeclaration {
        name: "inspect".to_string(),
        group: "core".to_string(),
        ..Default::default()
    }];
    catalog.add_skill(skill);
    let state = PersistedCatalogState {
        skills: vec![record("alpha", Some("1.0.0"))],
        active_groups: vec![],
        saved_at_ms: 0,
        schema_version: 1,
    };

    catalog.replay_loaded(&state, LoadReplayPolicy::SkipOnDrift);

    assert!(catalog.active_groups().is_empty());
    assert!(
        catalog
            .registry()
            .get_action("alpha__inspect", None)
            .is_some_and(|tool| !tool.enabled)
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
