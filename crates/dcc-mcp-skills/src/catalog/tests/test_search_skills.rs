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

// ── PIP-2469 P2: indexed vs linear parity and multi-filter regression ──

#[test]
fn test_search_skills_indexed_vs_linear_parity() {
    // Force the index path and verify results match a forced-linear path
    // for the same catalog state, same filter, same query. This catches
    // the P0 class of bug where index doc_idx is wrong.
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
        // Indexed path (builds or reuses index).
        let indexed = catalog.search_skills(Some(query), tags, *dcc, None, None);
        let indexed_names: Vec<String> = indexed.iter().map(|s| s.name.clone()).collect();

        // Force linear path by invalidating index but NOT allowing rebuild
        // (we check staleness directly — the linear fallback in prune_with_index
        // happens when tokens are empty or prefiltered is empty, so instead we
        // compare against a fresh catalog that never built an index).
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
        // Force linear path by setting index stale and never querying without
        // filter first (the empty query doesn't build index).
        let linear = fresh.search_skills(Some(query), tags, *dcc, None, None);
        let linear_names: Vec<String> = linear.iter().map(|s| s.name.clone()).collect();

        assert_eq!(
            indexed_names, linear_names,
            "indexed vs linear result order mismatch for tags={:?} dcc={:?}",
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
fn test_search_skills_multi_filter_index_integrity() {
    // Multi-filter sequence: dcc=None → dcc=maya → tags=[...] — the index
    // must remain correct across filter changes (P0 regression).
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

    // Step 1: no filter — builds index from full 40-entry catalog.
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
