//! Tests for search_hint field on SkillSummary and search_skills filtering.
use super::fixtures::{make_test_catalog, make_test_skill, make_test_skill_with_hint};

#[test]
fn test_skill_summary_search_hint_from_metadata() {
    let catalog = make_test_catalog();
    let skill = make_test_skill_with_hint(
        "maya-bevel",
        "maya",
        "polygon modeling, bevel, chamfer",
        &["bevel"],
    );
    catalog.add_skill(skill);

    let summaries = catalog.list_skills(None);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].search_hint, "polygon modeling, bevel, chamfer");
}

#[test]
fn test_skill_summary_search_hint_fallback_to_description() {
    let catalog = make_test_catalog();
    // No search_hint set — should fall back to description
    catalog.add_skill(make_test_skill("no-hint", "maya", &[]));

    let summaries = catalog.list_skills(None);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].search_hint, summaries[0].description);
}

#[test]
fn test_search_skills_matches_search_hint() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill_with_hint(
        "maya-bevel",
        "maya",
        "polygon modeling, bevel, chamfer, extrude",
        &["bevel"],
    ));
    catalog.add_skill(make_test_skill_with_hint(
        "git-tools",
        "python",
        "git, commit, branch, vcs",
        &["log"],
    ));

    // "chamfer" only appears in search_hint of maya-bevel
    let results = catalog.search_skills(Some("chamfer"), &[], None, None, None);
    assert_eq!(results.len(), 1, "Expected 1 match for 'chamfer'");
    assert_eq!(results[0].name, "maya-bevel");
}

#[test]
fn test_search_skills_matches_tool_name() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill_with_hint(
        "maya-bevel",
        "maya",
        "polygon modeling",
        &["bevel", "chamfer"],
    ));
    catalog.add_skill(make_test_skill_with_hint(
        "git-tools",
        "python",
        "version control",
        &["log", "diff"],
    ));

    // "diff" is a tool name in git-tools
    let results = catalog.search_skills(Some("diff"), &[], None, None, None);
    assert_eq!(results.len(), 1, "Expected 1 match for tool 'diff'");
    assert_eq!(results[0].name, "git-tools");
}

#[test]
fn test_search_skills_no_match_returns_empty() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill_with_hint(
        "skill-a",
        "maya",
        "modeling tools",
        &["tool_a"],
    ));

    let results = catalog.search_skills(Some("xyzzy_no_match"), &[], None, None, None);
    assert!(results.is_empty(), "Expected empty results for no match");
}

#[test]
fn test_search_skills_matches_name_and_hint_combined() {
    let catalog = make_test_catalog();
    // "maya" appears in name of first, but also search_hint of second
    catalog.add_skill(make_test_skill_with_hint(
        "maya-geometry",
        "maya",
        "polygon sphere cylinder",
        &["create"],
    ));
    catalog.add_skill(make_test_skill_with_hint(
        "blender-shader",
        "blender",
        "maya-compatible shaders, pbr",
        &["shader"],
    ));

    let results = catalog.search_skills(Some("maya"), &[], None, None, None);
    // Both should match: first by name, second by search_hint
    assert_eq!(results.len(), 2, "Both skills should match 'maya'");
}
