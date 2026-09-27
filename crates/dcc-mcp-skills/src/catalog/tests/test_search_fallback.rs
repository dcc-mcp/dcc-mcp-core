//! Catalog-level tests for the explicit `dcc-cua` fallback route (PIP-3702).
//!
//! The judgement itself is unit-tested in `dcc-mcp-gateway-search`. These tests
//! cover the part the catalog owns: that a skill with an empty `tools.yaml` /
//! no tool declaration reports `Some(0)`, that the advice rides on the same
//! ranking `search_skills` returns, and that a caller that ignores the advice
//! still gets byte-identical hits.

use super::fixtures::{make_test_catalog, make_test_skill};
use dcc_mcp_gateway_search::{
    CuaRuntimeState, FALLBACK_REASON_NO_CANDIDATE, FALLBACK_REASON_NO_EXECUTABLE_INTERFACE,
    FALLBACK_SKILL, StaticCuaProbe,
};

const READY: StaticCuaProbe = StaticCuaProbe::new(CuaRuntimeState::Ready);
const MISSING: StaticCuaProbe = StaticCuaProbe::new(CuaRuntimeState::Missing);

#[test]
fn an_answered_search_carries_no_fallback() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &["export_fbx"]));

    let result =
        catalog.search_skills_with_fallback(Some("maya-export-fbx"), &[], None, None, None, &READY);

    assert_eq!(result.hits.len(), 1);
    assert!(
        result.fallback.is_none(),
        "a skill with a callable tool is an answer: {:?}",
        result.fallback
    );
}

#[test]
fn a_skill_with_no_tool_declaration_falls_back() {
    let catalog = make_test_catalog();
    // Matches on name/description but declares nothing to call.
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &[]));

    let result =
        catalog.search_skills_with_fallback(Some("maya-export-fbx"), &[], None, None, None, &READY);

    assert_eq!(result.hits.len(), 1, "the textual match is still returned");
    let fallback = result
        .fallback
        .expect("a documentation-only skill is not an answer");
    assert_eq!(fallback.skill, FALLBACK_SKILL);
    assert_eq!(fallback.reason, FALLBACK_REASON_NO_EXECUTABLE_INTERFACE);
    assert!(!fallback.blocked);
}

#[test]
fn an_empty_result_falls_back_with_no_candidate() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &["export_fbx"]));

    let result =
        catalog.search_skills_with_fallback(Some("vendor-wizard"), &[], None, None, None, &READY);

    assert!(result.hits.is_empty());
    let fallback = result.fallback.expect("no candidates must offer the route");
    assert_eq!(fallback.reason, FALLBACK_REASON_NO_CANDIDATE);
    assert_eq!(fallback.skill, FALLBACK_SKILL);
}

#[test]
fn an_unusable_runtime_blocks_instead_of_rerouting() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &["export_fbx"]));

    let result =
        catalog.search_skills_with_fallback(Some("vendor-wizard"), &[], None, None, None, &MISSING);

    let fallback = result.fallback.expect("the blocker must still be reported");
    assert!(fallback.blocked);
    assert_eq!(
        fallback.blocked_reason.as_deref(),
        Some(dcc_mcp_gateway_search::FALLBACK_REASON_CUA_UNAVAILABLE)
    );
    assert_eq!(fallback.skill, FALLBACK_SKILL, "never another provider");
    assert!(
        fallback
            .preflight
            .iter()
            .any(|command| command.contains("components status dcc-cua"))
    );
}

#[test]
fn the_fallback_route_does_not_change_what_search_skills_returns() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &["export_fbx"]));
    catalog.add_skill(make_test_skill(
        "maya-create-sphere",
        "maya",
        &["create_sphere"],
    ));

    let plain = catalog.search_skills(Some("maya"), &[], None, None, None);
    let routed = catalog.search_skills_with_fallback(Some("maya"), &[], None, None, None, &MISSING);

    assert_eq!(
        plain.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        routed
            .hits
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        "the fallback is additive advice; ranking is untouched"
    );
    assert!(
        routed.fallback.is_none(),
        "this query is answered, so no advice is attached"
    );
}

#[test]
fn discovery_mode_never_attaches_advice() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &[]));

    let result = catalog.search_skills_with_fallback(None, &[], None, None, None, &MISSING);

    assert_eq!(result.hits.len(), 1);
    assert!(
        result.fallback.is_none(),
        "an empty query is a browse request, not an unanswered one"
    );
}

#[test]
fn an_empty_catalog_is_not_reported_as_no_skill_can_do_this() {
    // A misconfigured scan path is a discovery problem. Routing it to CUA
    // would hide the real fix behind a plausible-sounding suggestion.
    let catalog = make_test_catalog();

    let result = catalog.search_skills_with_fallback(
        Some("click through the vendor wizard"),
        &[],
        None,
        None,
        None,
        &READY,
    );

    assert!(result.hits.is_empty());
    assert!(
        result.fallback.is_none(),
        "an empty catalog is not evidence that no skill can serve the request"
    );
}

#[test]
fn a_filter_that_excluded_everything_is_not_reported_as_no_candidate() {
    // The catalog knows how many rows were eligible before ranking, so a
    // `dcc=` filter that matched nothing must not read as "no skill can do it".
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &["export_fbx"]));

    let result = catalog.search_skills_with_fallback(
        Some("click through the vendor wizard"),
        &[],
        Some("unreal"),
        None,
        None,
        &READY,
    );

    assert!(result.hits.is_empty());
    assert!(
        result.fallback.is_none(),
        "a shard filter that excluded every row is not a routing decision"
    );
}

#[test]
fn the_target_does_not_route_to_itself() {
    // `dcc-cua` declares no tools, so it satisfies the no-interface criterion.
    // Recommending it for a request it answered would be circular.
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("dcc-cua", "python", &[]));

    let result =
        catalog.search_skills_with_fallback(Some("dcc-cua"), &[], None, None, None, &READY);

    assert!(
        !result.hits.is_empty(),
        "precondition: the target was found"
    );
    assert!(
        result.fallback.is_none(),
        "the fallback target answering its own request is not an unanswered request"
    );
}

#[test]
fn the_advice_serializes_onto_the_wire() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &[]));

    let result = catalog.search_skills_with_fallback(
        Some("maya-export-fbx"),
        &[],
        None,
        None,
        None,
        &MISSING,
    );
    let json = serde_json::to_string(&result).unwrap();

    assert!(json.contains(FALLBACK_SKILL));
    assert!(json.contains(FALLBACK_REASON_NO_EXECUTABLE_INTERFACE));
    assert!(json.contains(dcc_mcp_gateway_search::FALLBACK_REASON_CUA_UNAVAILABLE));
    assert!(json.contains("\"blocked\":true"));
}

#[test]
fn a_result_without_advice_omits_the_field_entirely() {
    let catalog = make_test_catalog();
    catalog.add_skill(make_test_skill("maya-export-fbx", "maya", &["export_fbx"]));

    let result =
        catalog.search_skills_with_fallback(Some("maya-export-fbx"), &[], None, None, None, &READY);
    let json = serde_json::to_string(&result).unwrap();

    assert!(
        !json.contains("fallback"),
        "a caller that never triggers the route must see the old payload shape"
    );
}
