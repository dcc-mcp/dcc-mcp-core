//! Tests for per-dcc_type catalog shard partitioning (PIP-2470).
use super::fixtures::{add_skill_with_scope, make_test_catalog, make_test_skill};
use super::*;

#[test]
fn test_shard_single_type_filter_only_scans_matching_shard() {
    let catalog = make_test_catalog();

    // Add maya and blender skills.
    for i in 0..20 {
        catalog.add_skill(make_test_skill(
            &format!("maya-skill-{:03}", i),
            "maya",
            &[],
        ));
    }
    for i in 0..20 {
        catalog.add_skill(make_test_skill(
            &format!("blender-skill-{:03}", i),
            "blender",
            &[],
        ));
    }

    // dcc=maya: must only return maya skills.
    let results = catalog.search_skills(Some("skill"), &[], Some("maya"), None, None);
    assert!(!results.is_empty());
    for s in &results {
        assert_eq!(
            s.dcc.to_lowercase(),
            "maya",
            "dcc=maya filter must only return maya skills, got {} ({})",
            s.name,
            s.dcc
        );
    }

    // dcc=blender: must only return blender skills.
    let results = catalog.search_skills(Some("skill"), &[], Some("blender"), None, None);
    assert!(!results.is_empty());
    for s in &results {
        assert_eq!(
            s.dcc.to_lowercase(),
            "blender",
            "dcc=blender filter must only return blender skills, got {} ({})",
            s.name,
            s.dcc
        );
    }
}

#[test]
fn test_shard_unfiltered_search_merges_all_shards() {
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    catalog.add_skill(make_test_skill("blender-modeling", "blender", &[]));
    catalog.add_skill(make_test_skill("houdini-modeling", "houdini", &[]));

    // No dcc filter: must return all skills.
    let results = catalog.search_skills(Some("modeling"), &[], None, None, None);
    assert_eq!(results.len(), 3, "unfiltered search must merge all shards");
}

#[test]
fn test_shard_missing_type_returns_empty() {
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));

    // Query for a dcc that has no skills in the catalog.
    let results = catalog.search_skills(Some("modeling"), &[], Some("photoshop"), None, None);
    assert!(
        results.is_empty(),
        "missing dcc_type must return empty results"
    );
}

#[test]
fn test_shard_consistency_after_add() {
    let catalog = make_test_catalog();

    // Start empty — shard should not exist.
    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert!(results.is_empty());

    // Add a maya skill — shard is populated.
    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert_eq!(results.len(), 1);

    // Add a second maya skill.
    catalog.add_skill(make_test_skill("maya-rendering", "maya", &[]));
    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert_eq!(results.len(), 2);
}

#[test]
fn test_shard_consistency_after_remove() {
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    catalog.add_skill(make_test_skill("maya-rendering", "maya", &[]));
    catalog.add_skill(make_test_skill("blender-modeling", "blender", &[]));

    // Remove one maya skill.
    assert!(catalog.remove_skill("maya-modeling"));

    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "maya-rendering");

    // Remove the last maya skill — shard should be cleaned up.
    assert!(catalog.remove_skill("maya-rendering"));

    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert!(results.is_empty());

    // Blender shard still intact.
    let results = catalog.search_skills(None, &[], Some("blender"), None, None);
    assert_eq!(results.len(), 1);
}

#[test]
fn test_shard_consistency_after_clear() {
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    catalog.add_skill(make_test_skill("blender-modeling", "blender", &[]));

    catalog.clear();

    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert!(results.is_empty());

    let results = catalog.search_skills(None, &[], Some("blender"), None, None);
    assert!(results.is_empty());
}

#[test]
fn test_shard_combined_with_tags_filter() {
    let catalog = make_test_catalog();

    let mut modeling = make_test_skill("maya-modeling", "maya", &["bevel"]);
    modeling.tags = vec!["modeling".to_string()];
    add_skill_with_scope(&catalog, modeling, SkillScope::System);

    let mut rendering = make_test_skill("maya-rendering", "maya", &["render"]);
    rendering.tags = vec!["rendering".to_string()];
    add_skill_with_scope(&catalog, rendering, SkillScope::System);

    let mut blender_modeling = make_test_skill("blender-modeling", "blender", &["bevel"]);
    blender_modeling.tags = vec!["modeling".to_string()];
    add_skill_with_scope(&catalog, blender_modeling, SkillScope::System);

    // dcc=maya + tags=modeling: only maya-modeling.
    let results = catalog.search_skills(Some("bevel"), &["modeling"], Some("maya"), None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "maya-modeling");

    // dcc=blender + tags=modeling: only blender-modeling.
    let results = catalog.search_skills(Some("bevel"), &["modeling"], Some("blender"), None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "blender-modeling");
}

#[test]
fn test_shard_empty_dcc_filter_scans_all() {
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    catalog.add_skill(make_test_skill("blender-modeling", "blender", &[]));

    // Empty string dcc filter: treated as no filter → scans all.
    let results = catalog.search_skills(None, &[], Some(""), None, None);
    assert_eq!(results.len(), 2);
}

#[test]
fn test_shard_case_insensitive_dcc_lookup() {
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));

    // Mixed-case dcc filter.
    let results = catalog.search_skills(None, &[], Some("MAYA"), None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "maya-modeling");

    let results = catalog.search_skills(None, &[], Some("Maya"), None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "maya-modeling");
}

#[test]
fn test_shard_multi_dcc_query() {
    // search_skills only supports a single dcc filter, but the shard
    // mechanism is per-type. Verify that successive single-dcc queries
    // each hit the correct shard.
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    catalog.add_skill(make_test_skill("blender-modeling", "blender", &[]));
    catalog.add_skill(make_test_skill("houdini-modeling", "houdini", &[]));

    for dcc in &["maya", "blender", "houdini"] {
        let results = catalog.search_skills(Some("modeling"), &[], Some(dcc), None, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].dcc.to_lowercase(), *dcc);
    }
}

#[test]
fn test_shard_registry_register_updates_shard() {
    let catalog = make_test_catalog();

    let entry = SkillEntry::new(
        make_test_skill("reg-skill", "maya", &[]),
        SkillState::Discovered,
        Vec::new(),
        SkillScope::Repo,
        Default::default(),
    );

    // Register via the Registry trait.
    catalog.register(entry);

    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "reg-skill");
}

#[test]
fn test_shard_registry_remove_cleans_shard() {
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("reg-skill", "maya", &[]));

    // Remove via the Registry trait.
    assert!(catalog.remove("reg-skill"));

    let results = catalog.search_skills(None, &[], Some("maya"), None, None);
    assert!(results.is_empty());
}
