//! Tests for the CUA fallback routing contract (PIP-3702).
//!
//! A request that no skill can answer must return an explicit route to the
//! project-owned `dcc-cua` skill with a distinguishable reason code, instead
//! of an empty result array with no next step.
use super::fixtures::{make_test_catalog, make_test_skill};
use super::*;
use dcc_mcp_gateway_search::{
    FALLBACK_MIN_TOP1_SCORE, FALLBACK_TARGET_SKILL, FallbackReason, FallbackStatus,
};

/// A skill whose name matches the query *and* that declares a callable tool.
fn callable_skill(name: &str, dcc: &str, description: &str) -> SkillMetadata {
    let mut skill = make_test_skill(name, dcc, &[&format!("{name}__run")]);
    skill.description = description.to_string();
    skill
}

/// A skill whose name matches the query but that declares **no** tool:
/// guidance-only, or routed through a runtime.
fn guidance_only_skill(name: &str, dcc: &str, description: &str) -> SkillMetadata {
    let mut skill = make_test_skill(name, dcc, &[]);
    skill.description = description.to_string();
    skill
}

// ── Retrieval has an answer → no fallback ──────────────────────────────────

#[test]
fn a_confident_callable_hit_does_not_trigger_the_fallback() {
    let catalog = make_test_catalog();
    catalog.add_skill(callable_skill(
        "maya-shot-export",
        "maya",
        "Export the current maya shot to disk.",
    ));

    let page = catalog.search_skills_with_fallback(Some("maya shot export"), &[], None, None, None);

    assert!(
        !page.summaries.is_empty(),
        "the query must retrieve its skill"
    );
    assert_eq!(
        page.summaries[0].name, "maya-shot-export",
        "the retrieved skill must be the callable one"
    );
    assert!(
        page.fallback.is_none(),
        "a callable hit must not be routed to the CUA fallback, got {:?}",
        page.fallback
    );
}

#[test]
fn discovery_mode_never_triggers_the_fallback() {
    let catalog = make_test_catalog();
    catalog.add_skill(callable_skill("maya-shot-export", "maya", "Export a shot."));

    // An unfiltered listing scores every row at 0; that is a browse, not a
    // failed retrieval.
    let page = catalog.search_skills_with_fallback(None, &[], None, None, None);

    assert!(!page.summaries.is_empty());
    assert!(page.fallback.is_none());
}

#[test]
fn a_filter_miss_is_not_a_retrieval_failure() {
    let catalog = make_test_catalog();
    catalog.add_skill(callable_skill("maya-shot-export", "maya", "Export a shot."));

    // No houdini shard exists at all — the caller narrowed, retrieval did not
    // fail, so no route is suggested.
    let page =
        catalog.search_skills_with_fallback(Some("export"), &[], Some("houdini"), None, None);

    assert!(page.summaries.is_empty());
    assert!(page.fallback.is_none());
}

// ── Retrieval has no answer → explicit route ───────────────────────────────

#[test]
fn an_unmatched_query_routes_to_the_project_cua_route() {
    let catalog = make_test_catalog();
    catalog.add_skill(callable_skill("maya-shot-export", "maya", "Export a shot."));

    let page = catalog.search_skills_with_fallback(
        Some("zzqqxx unrelated application request"),
        &[],
        None,
        None,
        None,
    );

    assert!(page.summaries.is_empty(), "nothing should match");
    let fallback = page
        .fallback
        .expect("an empty retrieval must return routing advice");

    assert_eq!(fallback.target, FALLBACK_TARGET_SKILL);
    assert_eq!(fallback.target, "dcc-cua");
    assert_eq!(fallback.reason, FallbackReason::LowConfidence);
    assert_eq!(fallback.reason.as_str(), "low_confidence");
    assert!(!fallback.message.is_empty());
}

#[test]
fn a_guidance_only_hit_routes_with_the_no_interface_reason_code() {
    let catalog = make_test_catalog();
    catalog.add_skill(guidance_only_skill(
        "photoshop-gui-workflow",
        "photoshop",
        "Photoshop GUI workflow with no scriptable surface.",
    ));

    let page =
        catalog.search_skills_with_fallback(Some("photoshop gui workflow"), &[], None, None, None);

    assert!(
        !page.summaries.is_empty(),
        "the skill is still returned; the fallback is advice, not a filter"
    );
    let fallback = page
        .fallback
        .expect("a hit with no callable interface must be routed");

    assert_eq!(fallback.reason, FallbackReason::NoExecutableInterface);
    assert_eq!(fallback.reason.as_str(), "no_executable_interface");
    // Distinguishable from the low-confidence code.
    assert_ne!(
        fallback.reason.as_str(),
        FallbackReason::LowConfidence.as_str()
    );
    assert_eq!(fallback.target, FALLBACK_TARGET_SKILL);
}

#[test]
fn the_two_reason_codes_are_produced_by_different_shapes_of_failure() {
    let catalog = make_test_catalog();
    catalog.add_skill(guidance_only_skill(
        "photoshop-gui-workflow",
        "photoshop",
        "Photoshop GUI workflow with no scriptable surface.",
    ));

    let weak = catalog
        .search_skills_with_fallback(Some("zzqqxx unrelated request"), &[], None, None, None)
        .fallback
        .expect("fallback");
    let non_callable = catalog
        .search_skills_with_fallback(Some("photoshop gui workflow"), &[], None, None, None)
        .fallback
        .expect("fallback");

    assert_eq!(weak.reason, FallbackReason::LowConfidence);
    assert_eq!(non_callable.reason, FallbackReason::NoExecutableInterface);
    assert!(
        non_callable.top_score > weak.top_score,
        "a name match must outscore description noise: {} vs {}",
        non_callable.top_score,
        weak.top_score
    );
}

// ── Non-substitution contract ──────────────────────────────────────────────

#[test]
fn the_route_status_is_either_usable_or_an_explicit_blocker() {
    let catalog = make_test_catalog();
    catalog.add_skill(callable_skill("maya-shot-export", "maya", "Export a shot."));
    let page = catalog.search_skills_with_fallback(
        Some("zzqqxx unrelated application request"),
        &[],
        None,
        None,
        None,
    );
    let fallback = page.fallback.expect("fallback");

    // The route is resolved from the host, so the test can only pin the
    // contract: never "unknown", and a blocker always carries a repair step.
    match fallback.status {
        FallbackStatus::Ready => {
            assert_eq!(fallback.blocker, None);
            assert_eq!(fallback.repair, None);
        }
        FallbackStatus::Blocked => {
            let blocker = fallback.blocker.as_deref().expect("a blocker has a detail");
            assert!(!blocker.is_empty());
            assert_eq!(
                fallback.repair.as_deref(),
                Some("dcc-mcp-cli components ensure dcc-cua --yes")
            );
        }
        FallbackStatus::Unverified => panic!("the catalog always probes the route"),
    }
    assert!(!fallback.preflight.is_empty());
    assert_eq!(
        fallback.preflight[0],
        "dcc-mcp-cli components status dcc-cua"
    );
}

#[test]
fn an_empty_retrieval_never_advertises_a_generic_provider() {
    let catalog = make_test_catalog();
    catalog.add_skill(callable_skill("maya-shot-export", "maya", "Export a shot."));
    let fallback = catalog
        .search_skills_with_fallback(Some("zzqqxx unrelated request"), &[], None, None, None)
        .fallback
        .expect("fallback");

    for banned in ["computer-use", "@oai/sky", "openai computer"] {
        assert!(
            !fallback.message.to_ascii_lowercase().contains(banned),
            "the route must not advertise a generic provider: {banned}"
        );
    }
}

// ── Parity with the legacy entry point ─────────────────────────────────────

#[test]
fn search_skills_still_returns_only_the_ranked_list() {
    let catalog = make_test_catalog();
    catalog.add_skill(callable_skill("maya-shot-export", "maya", "Export a shot."));

    let legacy = catalog.search_skills(Some("maya shot export"), &[], None, None, None);
    let paged =
        catalog.search_skills_with_fallback(Some("maya shot export"), &[], None, None, None);

    assert_eq!(legacy.len(), paged.summaries.len());
    for (a, b) in legacy.iter().zip(paged.summaries.iter()) {
        assert_eq!(a.name, b.name);
    }
}

// ── Threshold calibration ───────────────────────────────────────────────────

/// Pin the measured score band that `FALLBACK_MIN_TOP1_SCORE` is calibrated
/// against, so a scorer change that collapses the band fails here instead of
/// silently mis-routing requests.
#[test]
fn the_confidence_gate_sits_in_the_measured_gap() {
    let catalog = make_test_catalog();
    catalog.add_skill(callable_skill(
        "maya-shot-export",
        "maya",
        "Export the current maya shot to disk.",
    ));

    // Probe the real top-1 score by disabling the gate for the measurement.
    let probe = |query: &str| -> u32 {
        let page = catalog.search_skills_with_fallback(Some(query), &[], None, None, None);
        match page.fallback {
            Some(fallback) => fallback.top_score,
            None => u32::MAX, // cleared the gate
        }
    };

    // A single-token name substring is the weakest genuine match we must keep.
    let genuine = probe("maya");
    // Description-only noise is the strongest thing we must route away.
    let noise = probe("render the farm queue");

    assert!(
        noise < FALLBACK_MIN_TOP1_SCORE,
        "description noise ({noise}) must fall under the gate {FALLBACK_MIN_TOP1_SCORE}"
    );
    assert!(
        genuine >= FALLBACK_MIN_TOP1_SCORE,
        "a genuine name match ({genuine}) must clear the gate {FALLBACK_MIN_TOP1_SCORE}"
    );
}

#[test]
fn an_empty_catalog_is_not_a_task_no_skill_can_do() {
    let catalog = make_test_catalog();

    // With nothing scanned, the fix is a rescan — routing to the CUA route
    // would hide the real problem.
    let page = catalog.search_skills_with_fallback(
        Some("zzqqxx unrelated application request"),
        &[],
        None,
        None,
        None,
    );

    assert!(page.summaries.is_empty());
    assert!(page.fallback.is_none());
}
