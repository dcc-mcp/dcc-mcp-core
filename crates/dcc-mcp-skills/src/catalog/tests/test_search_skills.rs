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

#[test]
fn test_search_skills_uses_shared_fuzzy_recall_without_exact_index_pruning() {
    let catalog = make_test_catalog();
    let mut maya = make_test_skill("maya-primitives", "maya", &[]);
    maya.description = "Create polygon spheres and cubes".to_string();
    catalog.add_skill(maya);
    let mut photoshop = make_test_skill("photoshop-export", "photoshop", &[]);
    photoshop.description = "Export the active image document".to_string();
    catalog.add_skill(photoshop);

    let results = catalog.search_skills(Some("polgyon sphere"), &[], None, None, None);

    assert_eq!(
        results.first().map(|hit| hit.name.as_str()),
        Some("maya-primitives")
    );
}

// ── Shared scorer determinism and mutation tests ────────────────────────

#[test]
fn test_search_skills_repeated_query_is_stable() {
    let catalog = make_test_catalog();

    // Add enough skills to exercise deterministic ranking at catalog scale.
    let words = ["polygon", "bevel", "render", "bake", "simulate"];
    for i in 0..50 {
        let name = format!("maya-skill-{i:03}");
        let mut skill = make_test_skill(&name, "maya", &[]);
        skill.description = format!("{} tools for maya", words[i % words.len()]);
        skill.tags = vec![words[(i + 2) % words.len()].to_string()];
        catalog.add_skill(skill);
    }

    // Repeated queries must remain stable.
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
fn test_search_skills_reflects_add() {
    let catalog = make_test_catalog();

    // Populate and query once.
    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    let _ = catalog.search_skills(Some("modeling"), &[], None, None, None);

    // Add a new skill that matches the same query.
    let mut new_skill = make_test_skill("maya-bevel", "maya", &[]);
    new_skill.description = "bevel polygon edges".to_string();
    catalog.add_skill(new_skill);
    // Search again; the new skill must appear.
    let results = catalog.search_skills(Some("bevel"), &[], None, None, None);
    assert!(
        results.iter().any(|s| s.name == "maya-bevel"),
        "new skill must appear after index invalidation"
    );
}

#[test]
fn test_search_skills_reflects_remove() {
    let catalog = make_test_catalog();

    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));
    catalog.add_skill(make_test_skill("maya-bevel", "maya", &[]));

    // Prime search state.
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
fn test_search_skills_query_is_deterministic_after_add() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));

    let first = catalog.search_skills(Some("modeling"), &[], None, None, None);
    let second = catalog.search_skills(Some("modeling"), &[], None, None, None);
    assert_eq!(first[0].name, second[0].name);
}

#[test]
fn test_search_skills_empty_query_returns_catalog() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-modeling", "maya", &[]));

    let results = catalog.search_skills(None, &[], None, None, None);
    assert_eq!(results.len(), 1);
}

// ── Cross-catalog parity and multi-filter regression ────────────────────

#[test]
fn test_search_skills_cross_catalog_parity() {
    let catalog = make_test_catalog();

    let words = [
        "polygon", "bevel", "render", "bake", "simulate", "animate", "rig", "uv",
    ];
    for i in 0..30 {
        let name = format!("maya-skill-{:03}", i);
        let mut skill = make_test_skill(&name, "maya", &[]);
        skill.description = format!("{} tools for dcc", words[i % words.len()]);
        skill.tags = vec![words[(i + 1) % words.len()].to_string()];
        catalog.add_skill(skill);
    }
    for i in 0..30 {
        let name = format!("blender-skill-{:03}", i);
        let mut skill = make_test_skill(&name, "blender", &[]);
        skill.description = format!("{} tools for dcc", words[i % words.len()]);
        skill.tags = vec![words[(i + 3) % words.len()].to_string()];
        catalog.add_skill(skill);
    }

    let query = "render animate";
    let filters: Vec<(&[&str], Option<&str>)> = vec![
        (&[][..], None),
        (&[], Some("maya")),
        (&[], Some("blender")),
        (&["render"], None),
        (&["render"], Some("maya")),
        (&["rig"], Some("blender")),
    ];

    for (tags, dcc) in &filters {
        // First catalog.
        let indexed = catalog.search_skills(Some(query), tags, *dcc, None, None);
        let indexed_names: Vec<String> = indexed.iter().map(|s| s.name.clone()).collect();

        // Equivalent fresh catalog.
        let fresh = make_test_catalog();
        for i in 0..30 {
            let name = format!("maya-skill-{:03}", i);
            let mut skill = make_test_skill(&name, "maya", &[]);
            skill.description = format!("{} tools for dcc", words[i % words.len()]);
            skill.tags = vec![words[(i + 1) % words.len()].to_string()];
            fresh.add_skill(skill);
        }
        for i in 0..30 {
            let name = format!("blender-skill-{:03}", i);
            let mut skill = make_test_skill(&name, "blender", &[]);
            skill.description = format!("{} tools for dcc", words[i % words.len()]);
            skill.tags = vec![words[(i + 3) % words.len()].to_string()];
            fresh.add_skill(skill);
        }
        let linear = fresh.search_skills(Some(query), tags, *dcc, None, None);
        let linear_names: Vec<String> = linear.iter().map(|s| s.name.clone()).collect();

        assert_eq!(
            indexed_names, linear_names,
            "cross-catalog result order mismatch for tags={:?} dcc={:?}",
            tags, dcc
        );
        assert_eq!(
            indexed.len(),
            linear.len(),
            "result count mismatch for tags={:?} dcc={:?}",
            tags,
            dcc
        );
    }
}

#[test]
fn test_search_skills_multi_filter_integrity() {
    // Multi-filter sequence must remain correct across filter changes.
    let catalog = make_test_catalog();

    let words = ["polygon", "bevel", "render", "bake", "simulate"];
    for i in 0..20 {
        let name = format!("maya-skill-{:03}", i);
        let mut skill = make_test_skill(&name, "maya", &[]);
        skill.description = format!("{} maya tool", words[i % words.len()]);
        catalog.add_skill(skill);
    }
    for i in 0..20 {
        let name = format!("blender-skill-{:03}", i);
        let mut skill = make_test_skill(&name, "blender", &[]);
        skill.description = format!("{} blender tool", words[i % words.len()]);
        catalog.add_skill(skill);
    }

    let query = "polygon";

    // Step 1: no filter.
    let r1 = catalog.search_skills(Some(query), &[], None, None, None);
    assert!(!r1.is_empty(), "step 1 must return results");

    // Step 2: dcc=maya — must only return maya skills.
    let r2 = catalog.search_skills(Some(query), &[], Some("maya"), None, None);
    assert!(!r2.is_empty(), "step 2 must return maya results");
    for s in &r2 {
        assert_eq!(
            s.dcc.to_lowercase(),
            "maya",
            "step 2 must only return maya skills, got {}",
            s.name
        );
    }

    // Step 3: dcc=blender — must only return blender skills.
    let r3 = catalog.search_skills(Some(query), &[], Some("blender"), None, None);
    assert!(!r3.is_empty(), "step 3 must return blender results");
    for s in &r3 {
        assert_eq!(
            s.dcc.to_lowercase(),
            "blender",
            "step 3 must only return blender skills, got {}",
            s.name
        );
    }

    // Step 4: back to no filter — must return both.
    let r4 = catalog.search_skills(Some(query), &[], None, None, None);
    assert!(!r4.is_empty(), "step 4 must return results");
    let has_maya = r4.iter().any(|s| s.dcc.eq_ignore_ascii_case("maya"));
    let has_blender = r4.iter().any(|s| s.dcc.eq_ignore_ascii_case("blender"));
    assert!(has_maya, "step 4 must include maya skills");
    assert!(has_blender, "step 4 must include blender skills");

    // Step 5: dcc=maya again after a different filter — must still be correct.
    let r5 = catalog.search_skills(Some(query), &[], Some("maya"), None, None);
    assert!(!r5.is_empty(), "step 5 must return maya results");
    for s in &r5 {
        assert_eq!(
            s.dcc.to_lowercase(),
            "maya",
            "step 5 must only return maya skills, got {}",
            s.name
        );
    }
}
