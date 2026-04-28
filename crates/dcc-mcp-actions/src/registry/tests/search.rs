//! Tests for search_actions, get_categories, get_tags, count_actions.

use super::fixtures::populate_search_registry;
use super::*;

// ── search_actions ──────────────────────────────────────────────────────────

#[test]
fn search_by_category_returns_matching() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some("geometry"), &[], None);
    assert_eq!(
        results.len(),
        3,
        "should find 3 geometry actions across all DCCs"
    );
    assert!(results.iter().all(|m| m.category == "geometry"));
}

#[test]
fn search_by_tag_returns_matching() {
    let reg = populate_search_registry();
    let results = reg.search_actions(None, &["mesh"], None);
    assert_eq!(results.len(), 3, "create_sphere, delete_mesh, create_cube");
    assert!(results.iter().all(|m| m.tags.contains(&"mesh".to_string())));
}

#[test]
fn search_by_multiple_tags_all_required() {
    let reg = populate_search_registry();
    let results = reg.search_actions(None, &["create", "mesh"], None);
    assert_eq!(results.len(), 2, "create_sphere + create_cube");
}

#[test]
fn search_by_category_and_dcc() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some("geometry"), &[], Some("maya"));
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|m| m.dcc == "maya"));
}

#[test]
fn search_by_category_tag_and_dcc() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some("geometry"), &["create"], Some("blender"));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "create_cube");
}

#[test]
fn search_with_no_filters_returns_all() {
    let reg = populate_search_registry();
    let results = reg.search_actions(None, &[], None);
    assert_eq!(results.len(), 5);
}

#[test]
fn search_with_empty_category_matches_all_categories() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some(""), &[], None);
    // Empty string means no category filter
    assert_eq!(results.len(), 5);
}

#[test]
fn search_no_match_returns_empty() {
    let reg = populate_search_registry();
    let results = reg.search_actions(Some("nonexistent_category"), &[], None);
    assert!(results.is_empty());
}

#[test]
fn search_tag_not_found_returns_empty() {
    let reg = populate_search_registry();
    let results = reg.search_actions(None, &["nonexistent_tag"], None);
    assert!(results.is_empty());
}

#[test]
fn search_on_empty_registry_returns_empty() {
    let reg = ActionRegistry::new();
    let results = reg.search_actions(Some("geometry"), &["mesh"], Some("maya"));
    assert!(results.is_empty());
}

// ── get_categories ──────────────────────────────────────────────────────────

#[test]
fn get_categories_returns_sorted_deduped() {
    let reg = populate_search_registry();
    let cats = reg.get_categories(None);
    assert_eq!(cats, vec!["geometry", "rendering"]);
}

#[test]
fn get_categories_scoped_to_dcc() {
    let reg = populate_search_registry();
    let cats = reg.get_categories(Some("maya"));
    assert_eq!(cats, vec!["geometry", "rendering"]);
    let blender_cats = reg.get_categories(Some("blender"));
    assert_eq!(blender_cats, vec!["geometry", "rendering"]);
}

#[test]
fn get_categories_empty_registry_returns_empty() {
    let reg = ActionRegistry::new();
    assert!(reg.get_categories(None).is_empty());
}

#[test]
fn get_categories_skips_blank_category() {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "no_cat".into(),
        dcc: "maya".into(),
        category: String::new(), // blank category
        ..Default::default()
    });
    assert!(reg.get_categories(None).is_empty());
}

// ── get_tags ────────────────────────────────────────────────────────────────

#[test]
fn get_tags_returns_sorted_deduped() {
    let reg = populate_search_registry();
    let mut tags = reg.get_tags(None);
    tags.sort(); // already sorted, but just to be explicit
    // Expected: bake, create, delete, mesh, output, render, texture
    assert!(tags.contains(&"mesh".to_string()));
    assert!(tags.contains(&"create".to_string()));
    assert!(tags.contains(&"render".to_string()));
    // No duplicates
    let before_dedup = tags.len();
    let mut deduped = tags.clone();
    deduped.dedup();
    assert_eq!(before_dedup, deduped.len());
}

#[test]
fn get_tags_scoped_to_dcc() {
    let reg = populate_search_registry();
    let maya_tags = reg.get_tags(Some("maya"));
    // Maya has: create, mesh, delete, render, output
    assert!(maya_tags.contains(&"create".to_string()));
    assert!(maya_tags.contains(&"output".to_string()));
    // "texture" is only in blender
    assert!(!maya_tags.contains(&"texture".to_string()));
}

#[test]
fn get_tags_empty_registry_returns_empty() {
    let reg = ActionRegistry::new();
    assert!(reg.get_tags(None).is_empty());
}

// ── count_actions ───────────────────────────────────────────────────────────

#[test]
fn count_actions_matches_search_results() {
    let reg = populate_search_registry();
    assert_eq!(
        reg.count_actions(Some("geometry"), &["create"], None),
        reg.search_actions(Some("geometry"), &["create"], None)
            .len()
    );
    assert_eq!(reg.count_actions(None, &[], None), reg.len());
}
