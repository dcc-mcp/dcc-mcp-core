//! Tests for unified `search_skills` (issue #340) — scope ordering,
//! filtering, and combined predicates.
use super::fixtures::{add_skill_with_scope, make_test_catalog, make_test_skill};
use super::*;

#[test]
fn test_search_skills_empty_query_returns_by_scope_precedence() {
    // Admin > System > Team > User > Repo, then alphabetical name.
    let catalog = make_test_catalog();
    add_skill_with_scope(
        &catalog,
        make_test_skill("zeta-user", "maya", &[]),
        SkillScope::User,
    );
    add_skill_with_scope(
        &catalog,
        make_test_skill("alpha-repo", "maya", &[]),
        SkillScope::Repo,
    );
    add_skill_with_scope(
        &catalog,
        make_test_skill("gamma-admin", "maya", &[]),
        SkillScope::Admin,
    );
    add_skill_with_scope(
        &catalog,
        make_test_skill("beta-system", "maya", &[]),
        SkillScope::System,
    );
    add_skill_with_scope(
        &catalog,
        make_test_skill("delta-team", "maya", &[]),
        SkillScope::Team,
    );

    let results = catalog.search_skills(None, &[], None, None, None);
    let names: Vec<&str> = results.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "gamma-admin",
            "beta-system",
            "delta-team",
            "zeta-user",
            "alpha-repo"
        ]
    );
}

#[test]
fn test_search_skills_limit_caps_output() {
    let catalog = make_test_catalog();
    for i in 0..5 {
        catalog.add_skill(make_test_skill(&format!("skill-{i}"), "maya", &[]));
    }

    let results = catalog.search_skills(None, &[], None, None, Some(2));
    assert_eq!(results.len(), 2);
}

#[test]
fn test_search_skills_scope_filter() {
    let catalog = make_test_catalog();
    add_skill_with_scope(
        &catalog,
        make_test_skill("sys-skill", "maya", &[]),
        SkillScope::System,
    );
    add_skill_with_scope(
        &catalog,
        make_test_skill("repo-skill", "maya", &[]),
        SkillScope::Repo,
    );

    let results = catalog.search_skills(None, &[], None, Some(SkillScope::System), None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "sys-skill");
}

#[test]
fn test_search_skills_combined_filters() {
    // query + dcc + scope + limit all AND-ed.
    let catalog = make_test_catalog();
    let mut modeling = make_test_skill("maya-modeling", "maya", &["bevel"]);
    modeling.tags = vec!["modeling".to_string()];
    add_skill_with_scope(&catalog, modeling, SkillScope::System);

    let mut rendering = make_test_skill("maya-rendering", "maya", &["render"]);
    rendering.tags = vec!["rendering".to_string()];
    add_skill_with_scope(&catalog, rendering, SkillScope::System);

    let results = catalog.search_skills(
        Some("bevel"),
        &["modeling"],
        Some("maya"),
        Some(SkillScope::System),
        Some(5),
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "maya-modeling");
}

#[test]
fn test_search_skills_parse_scope_str_valid_and_invalid() {
    use super::super::parse_scope_str;
    assert_eq!(parse_scope_str("repo").unwrap(), SkillScope::Repo);
    assert_eq!(parse_scope_str("USER").unwrap(), SkillScope::User);
    assert_eq!(parse_scope_str("Team").unwrap(), SkillScope::Team);
    assert_eq!(parse_scope_str("System").unwrap(), SkillScope::System);
    assert_eq!(parse_scope_str("admin").unwrap(), SkillScope::Admin);
    assert!(parse_scope_str("bogus").is_err());
}

#[test]
fn test_search_skills_returns_matching_skills() {
    let catalog = make_test_catalog();
    let mut a = make_test_skill("a", "maya", &["bevel"]);
    a.tags = vec!["modeling".to_string()];
    catalog.add_skill(a);
    catalog.add_skill(make_test_skill("b", "blender", &[]));

    let results = catalog.search_skills(Some("bevel"), &["modeling"], Some("maya"), None, None);

    let names: Vec<&str> = results.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"a"), "search_skills must include 'a'");
}

// ── PIP-2469: inverted index integration tests ──────────────────────────

#[test]
fn test_search_skills_with_inverted_index_same_results() {
    // search_skills with inverted index must return identical results as
    // search_skills without (the linear path is a correctness baseline).
    let catalog = make_test_catalog();

    // Add enough skills to trigger index usage.
    let words = ["polygon", "bevel", "render", "bake", "simulate"];
    for i in 0..50 {
        let name = format!("maya-skill-{i:03}");
        let mut skill = make_test_skill(&name, "maya", &[]);
        skill.description = format!("{} tools for maya", words[i % words.len()]);
        skill.tags = vec![words[(i + 2) % words.len()].to_string()];
        catalog.add_skill(skill);
    }

    // First query builds the index, second uses it.
    let results1 = catalog.search_skills(Some("polygon bevel"), &[], None, None, None);
    let results2 = catalog.search_skills(Some("polygon bevel"), &[], None, None, None);

    assert!(
        !results1.is_empty(),
        "must find skills matching polygon bevel"
    );
    assert_eq!(
        results1.len(),
        results2.len(),
        "repeat query must be stable"
    );
    for (a, b) in results1.iter().zip(results2.iter()) {
        assert_eq!(a.name, b.name, "order must be stable");
    }
}

#[test]
fn test_search_skills_index_invalidation_on_add() {
    // Adding a new skill must invalidate the index; subsequent search
    // must include the new skill.
    let catalog = make_test_catalog();

    // Populate and query once to build index.
    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    let _ = catalog.search_skills(Some("modeling"), &[], None, None, None);

    // Add a new skill that matches the same query.
    let mut new_skill = make_test_skill("maya-bevel", "maya", &[]);
    new_skill.description = "bevel polygon edges".to_string();
    catalog.add_skill(new_skill);

    // Search again — the index was invalidated and rebuilt; new skill
    // must appear.
    let results = catalog.search_skills(Some("bevel"), &[], None, None, None);
    assert!(
        results.iter().any(|s| s.name == "maya-bevel"),
        "new skill must appear after index invalidation"
    );
}

#[test]
fn test_search_skills_index_invalidation_on_remove() {
    // Removing a skill must invalidate the index; subsequent search
    // must not include the removed skill.
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    catalog.add_skill(make_test_skill("maya-bevel", "maya", &[]));

    // Build index.
    let _ = catalog.search_skills(Some("maya"), &[], None, None, None);

    // Remove a skill.
    assert!(catalog.remove_skill("maya-bevel"));

    // Search again — removed skill must not appear.
    let results = catalog.search_skills(Some("maya"), &[], None, None, None);
    assert!(
        !results.iter().any(|s| s.name == "maya-bevel"),
        "removed skill must not appear after index invalidation"
    );
    assert!(
        results.iter().any(|s| s.name == "maya-modeling"),
        "remaining skill must still appear"
    );
}

#[test]
fn test_search_skills_index_stale_flag_cleared_after_rebuild() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));

    // Initially stale.
    assert!(catalog.inverted_index.read().is_stale());

    // Query builds the index.
    let _ = catalog.search_skills(Some("modeling"), &[], None, None, None);

    // Now not stale.
    assert!(!catalog.inverted_index.read().is_stale());
}

#[test]
fn test_search_skills_empty_query_skips_index() {
    // Empty query must not build or use the inverted index.
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));

    let results = catalog.search_skills(None, &[], None, None, None);
    assert_eq!(results.len(), 1);

    // Index should still be stale (empty query path skips index).
    assert!(catalog.inverted_index.read().is_stale());
}
