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
