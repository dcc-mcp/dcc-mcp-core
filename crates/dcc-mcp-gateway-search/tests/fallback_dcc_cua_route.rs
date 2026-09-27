//! Explicit `dcc-cua` fallback routing contract (PIP-3702).
//!
//! Three behaviours gate this contract:
//!
//! 1. **A solvable search must not fall back.** The route is a last resort, so
//!    firing it on a result set that already answers the request is a defect.
//! 2. **An unsolvable search must fall back, with the right reason code.**
//!    `no_candidate` / `low_confidence` / `no_executable_interface` are
//!    different failures and the caller needs to tell them apart.
//! 3. **An unusable `dcc-cua` must block, never re-route.** The
//!    non-substitution contract forbids swapping in a generic computer-use
//!    provider, so the advice says "blocked" and names the repair commands.
//!
//! Ranking is *not* touched here: `dcc-cua` carries `layer: infrastructure`,
//! which the rank policy deliberately demotes to `0.35`, and that must stay.

use dcc_mcp_gateway_search::{
    CuaRuntimeProbe, CuaRuntimeState, FALLBACK_REASON_CUA_UNAVAILABLE,
    FALLBACK_REASON_LOW_CONFIDENCE, FALLBACK_REASON_NO_CANDIDATE,
    FALLBACK_REASON_NO_EXECUTABLE_INTERFACE, FALLBACK_SKILL, FallbackPolicy, SearchQuery,
    SearchRecord, StaticCuaProbe, search_page, search_page_with_fallback,
};
use uuid::Uuid;

const READY: StaticCuaProbe = StaticCuaProbe::new(CuaRuntimeState::Ready);
const MISSING: StaticCuaProbe = StaticCuaProbe::new(CuaRuntimeState::Missing);

// ── Threshold calibration (PIP-3702) ────────────────────────────────────────

/// Pin the measured band the confidence gate sits in.
///
/// The gate is only meaningful while a real name match scores well above it
/// and description-only noise scores below it. If a scorer change compresses
/// that band, this test fails — which is the point: the failure must be loud,
/// not a silent change in routing behaviour.
#[test]
fn the_confidence_gate_sits_in_the_measured_gap() {
    let gate = dcc_mcp_gateway_search::FALLBACK_MIN_CONFIDENT_SCORE;

    // A genuine name match: query tokens hit the skill name and the tool.
    let name_match = score_top1("maya shot export", &record_set());
    // Description-only noise: every token is generic, nothing names a skill.
    let noise = score_top1("render the farm queue", &record_set());

    assert!(
        noise < gate,
        "description noise must fall below the gate: noise={noise}, gate={gate}"
    );
    assert!(
        name_match >= gate,
        "a real name match must clear the gate: top1={name_match}, gate={gate}"
    );
    assert!(
        name_match >= gate * 2,
        "the band must stay wide (top1={name_match}, gate={gate}); a narrower \
         band means the scorer moved and the gate needs re-measuring"
    );
}

/// A corpus mixing name-bearing rows with description-only rows.
fn record_set() -> Vec<Row> {
    vec![
        Row::new(
            "maya-shot-export",
            "maya_shot_export",
            "Export the current Maya shot to disk.",
        ),
        Row::new(
            "maya-create-sphere",
            "create_sphere",
            "Create a polygonal sphere primitive in Maya.",
        ),
        Row::new(
            "render-queue",
            "submit_render",
            "Submit a job to the render queue.",
        ),
    ]
}

fn score_top1(query: &str, records: &[Row]) -> u32 {
    dcc_mcp_gateway_search::search(
        records,
        &SearchQuery {
            query: query.to_string(),
            ..Default::default()
        },
    )
    .first()
    .map_or(0, |hit| hit.score)
}

// ── Guards the route must not trip over ─────────────────────────────────────

#[test]
fn the_target_does_not_route_to_itself() {
    // `dcc-cua` declares no tools of its own, so it satisfies the
    // no-executable-interface criterion. Recommending it for a request it
    // already answered would tell the caller to use the thing it just found.
    let records =
        vec![Row::new("dcc-cua", "dcc-cua", "Project UI control route.").with_tools(Some(0))];

    let page = search_page_with_fallback(
        &records,
        &SearchQuery {
            query: "dcc-cua".to_string(),
            ..Default::default()
        },
        ready_probe(),
    );

    assert!(!page.hits.is_empty(), "precondition: the target was found");
    assert!(
        page.fallback.is_none(),
        "the fallback target answering its own request is not an unanswered request"
    );
}

#[test]
fn the_target_is_matched_case_insensitively() {
    // Skill names come from hand-written SKILL.md frontmatter.
    let records =
        vec![Row::new("DCC-CUA", "DCC-CUA", "Project UI control route.").with_tools(Some(0))];

    let page = search_page_with_fallback(
        &records,
        &SearchQuery {
            query: "dcc cua".to_string(),
            ..Default::default()
        },
        ready_probe(),
    );
    assert!(page.fallback.is_none());
}

#[test]
fn an_empty_index_is_not_reported_as_no_skill_can_do_this() {
    // Nothing was ever eligible to be ranked. That is a scan-path or
    // configuration problem; routing it to CUA would mask the real fix.
    let empty: Vec<Row> = Vec::new();
    let page = search_page_with_fallback(
        &empty,
        &SearchQuery {
            query: "click through the vendor wizard".to_string(),
            ..Default::default()
        },
        ready_probe(),
    );

    assert!(page.hits.is_empty());
    assert!(
        page.fallback.is_none(),
        "an empty catalog is not evidence that no skill can serve the request"
    );
}

#[test]
fn a_query_side_filter_that_excluded_everything_is_not_no_candidate() {
    // Regression: the candidate count must survive the query's own filters
    // (`dcc_type`, `instance_id`, `loaded_only`, `tags`, `min_score`), not just
    // count the rows handed in. Otherwise "the caller filtered everything out"
    // is reported as "no skill can do this".
    let mut records = corpus();
    records[0].dcc_type = "maya".to_string();
    records[1].dcc_type = "maya".to_string();

    let page = search_page_with_fallback(
        &records,
        &SearchQuery {
            query: "click through the vendor wizard".to_string(),
            dcc_type: Some("unreal".to_string()),
            ..Default::default()
        },
        ready_probe(),
    );

    assert!(page.hits.is_empty());
    assert!(
        page.fallback.is_none(),
        "a dcc filter that excluded every row is not evidence that no skill can serve the request"
    );
}

#[test]
fn min_score_is_a_quality_bar_not_a_scope_filter() {
    // `min_score` is applied after scoring and does not shrink the eligible
    // set, so a bar nothing clears still means "nothing here was good enough" —
    // which IS worth routing on. Contrast with `dcc_type` / `instance_id` /
    // `loaded_only` / `tags`, which make rows ineligible before scoring and
    // therefore must not trigger the route.
    let page = search_page_with_fallback(
        &corpus(),
        &SearchQuery {
            query: "export".to_string(),
            min_score: Some(u32::MAX),
            ..Default::default()
        },
        ready_probe(),
    );

    assert!(page.hits.is_empty(), "precondition: nothing clears the bar");
    let fallback = page.fallback.expect("nothing met the caller's quality bar");
    assert_eq!(fallback.reason, FALLBACK_REASON_NO_CANDIDATE);
}

#[test]
fn empty_pages_never_fire_through_the_convenience_overload() {
    // Documented limitation: `resolve_fallback` derives the candidate count
    // from the page, so an empty page cannot yield `no_candidate`. Callers that
    // can see the candidate set use `resolve_fallback_among`.
    let page: Vec<dcc_mcp_gateway_search::SearchHit<Row>> = Vec::new();
    assert!(dcc_mcp_gateway_search::resolve_fallback(&page, "wizard", ready_probe()).is_none());
}

#[test]
fn an_empty_candidate_set_is_never_a_routing_decision() {
    // `resolve_fallback` only sees a ranked page, so it cannot tell "nothing
    // matched" from "nothing was eligible". Callers that filter before
    // ranking must pass the candidate count explicitly.
    let no_hits: Vec<dcc_mcp_gateway_search::SearchHit<Row>> = Vec::new();
    assert!(
        dcc_mcp_gateway_search::resolve_fallback_among(
            &no_hits,
            "click through the vendor wizard",
            0,
            ready_probe(),
        )
        .is_none(),
        "zero candidates considered is a discovery problem, not a routing one"
    );
    // With candidates in the running, the same empty page is a real no-hit.
    let hits = dcc_mcp_gateway_search::search(
        &corpus(),
        &SearchQuery {
            query: "wizard".to_string(),
            ..Default::default()
        },
    );
    assert!(hits.is_empty(), "precondition: the query matches nothing");
    assert!(
        dcc_mcp_gateway_search::resolve_fallback_among(
            &hits,
            "wizard",
            corpus().len(),
            ready_probe(),
        )
        .is_some(),
        "candidates existed and none matched — that IS a routing decision"
    );
}

#[derive(Clone)]
struct Row {
    tool_slug: String,
    backend_tool: String,
    summary: String,
    skill_name: Option<String>,
    tags: Vec<String>,
    dcc_type: String,
    instance_id: Uuid,
    loaded: bool,
    /// `None` models a row type that does not track tool counts at all.
    tools: Option<usize>,
}

impl Row {
    fn new(slug: &str, tool: &str, summary: &str) -> Self {
        Self {
            tool_slug: slug.to_string(),
            backend_tool: tool.to_string(),
            summary: summary.to_string(),
            skill_name: None,
            tags: Vec::new(),
            dcc_type: "python".to_string(),
            instance_id: Uuid::from_u128(1),
            loaded: true,
            tools: Some(1),
        }
    }

    fn with_tools(mut self, count: Option<usize>) -> Self {
        self.tools = count;
        self
    }
}

impl SearchRecord for Row {
    fn tool_slug(&self) -> &str {
        &self.tool_slug
    }
    fn backend_tool(&self) -> &str {
        &self.backend_tool
    }
    fn summary(&self) -> &str {
        &self.summary
    }
    fn skill_name(&self) -> Option<&str> {
        self.skill_name.as_deref()
    }
    fn tags(&self) -> &[String] {
        &self.tags
    }
    fn dcc_type(&self) -> &str {
        &self.dcc_type
    }
    fn instance_id(&self) -> Uuid {
        self.instance_id
    }
    fn loaded(&self) -> bool {
        self.loaded
    }
    fn executable_interface_count(&self) -> Option<usize> {
        self.tools
    }
}

fn corpus() -> Vec<Row> {
    vec![
        Row::new(
            "export-fbx",
            "export_fbx",
            "Export the current scene to FBX interchange.",
        ),
        Row::new(
            "create-sphere",
            "create_sphere",
            "Create a polygonal sphere primitive.",
        ),
    ]
}

fn ready_probe() -> &'static dyn CuaRuntimeProbe {
    &READY
}

// ── 1. A solvable search must not fall back ──────────────────────────────────

#[test]
fn answered_search_does_not_fall_back() {
    let page = search_page_with_fallback(
        &corpus(),
        &SearchQuery {
            query: "export_fbx".to_string(),
            ..Default::default()
        },
        ready_probe(),
    );

    assert!(!page.hits.is_empty(), "the query is answerable");
    assert!(
        page.fallback.is_none(),
        "a result set that answers the request must not advertise the fallback: {:?}",
        page.fallback
    );
}

#[test]
fn the_hits_are_identical_with_and_without_the_route() {
    let query = SearchQuery {
        query: "create sphere".to_string(),
        ..Default::default()
    };
    let plain = search_page(&corpus(), &query);
    let routed = search_page_with_fallback(&corpus(), &query, ready_probe());

    assert_eq!(plain.total, routed.total);
    assert_eq!(plain.offset, routed.offset);
    assert_eq!(plain.limit, routed.limit);
    assert_eq!(
        plain
            .hits
            .iter()
            .map(|hit| hit.record.tool_slug.clone())
            .collect::<Vec<_>>(),
        routed
            .hits
            .iter()
            .map(|hit| hit.record.tool_slug.clone())
            .collect::<Vec<_>>(),
        "the fallback is additive advice; it must never reorder or drop hits"
    );
    assert_eq!(plain.fallback, None);
}

#[test]
fn discovery_mode_never_falls_back() {
    let page = search_page_with_fallback(
        &corpus(),
        &SearchQuery {
            query: String::new(),
            ..Default::default()
        },
        ready_probe(),
    );
    assert!(!page.hits.is_empty());
    assert!(
        page.fallback.is_none(),
        "an empty query is a browse request, not an unanswered one"
    );
}

// ── 2. An unsolvable search must fall back with the right reason ─────────────

#[test]
fn no_candidate_falls_back_with_the_no_candidate_reason() {
    let page = search_page_with_fallback(
        &corpus(),
        &SearchQuery {
            query: "wizard".to_string(),
            ..Default::default()
        },
        ready_probe(),
    );

    assert!(page.hits.is_empty(), "precondition: nothing matches");
    let fallback = page
        .fallback
        .expect("a query no tool can serve must offer the dcc-cua route");
    assert_eq!(fallback.skill, FALLBACK_SKILL);
    assert_eq!(fallback.reason, FALLBACK_REASON_NO_CANDIDATE);
    assert!(!fallback.blocked, "the runtime is ready, so act on it");
    assert!(fallback.blocked_reason.is_none());
    assert!(
        fallback
            .preflight
            .iter()
            .any(|c| c.contains("components status"))
    );
}

#[test]
fn a_realistic_gui_only_task_falls_back_with_the_low_confidence_reason() {
    // Nothing in the corpus can drive a GUI, but fuzzy matching still returns
    // a weak lexical match — the near-miss case the threshold exists for.
    let page = search_page_with_fallback(
        &corpus(),
        &SearchQuery {
            query: "click through the vendor export wizard".to_string(),
            ..Default::default()
        },
        ready_probe(),
    );

    assert!(
        !page.hits.is_empty(),
        "precondition: fuzzy matching returns a near-miss rather than nothing"
    );
    let fallback = page.fallback.expect("a near-miss is not an answer");
    assert_eq!(fallback.reason, FALLBACK_REASON_LOW_CONFIDENCE);
    assert_eq!(fallback.skill, FALLBACK_SKILL);
    assert!(!fallback.blocked);
    // The near-miss is still returned; the advice does not replace it.
    assert_eq!(page.hits.len(), 1);
}

#[test]
fn a_documentation_only_top_hit_falls_back_with_the_no_interface_reason() {
    // Matches the query on text (`export_fbx`) but declares nothing to call,
    // so the strong score must not be mistaken for a usable interface.
    let records = vec![
        Row::new(
            "export-guide",
            "export_fbx",
            "Export the current scene to FBX interchange.",
        )
        .with_tools(Some(0)),
    ];

    let page = search_page_with_fallback(
        &records,
        &SearchQuery {
            query: "export_fbx".to_string(),
            ..Default::default()
        },
        ready_probe(),
    );

    let fallback = page
        .fallback
        .expect("a skill with nothing to call is not an answer");
    assert_eq!(fallback.reason, FALLBACK_REASON_NO_EXECUTABLE_INTERFACE);
    assert_eq!(fallback.skill, FALLBACK_SKILL);
    assert!(!fallback.blocked);
    // The textual match is still returned — the advice is additive.
    assert_eq!(page.hits.len(), 1);
}

#[test]
fn an_unmodelled_tool_count_is_not_evidence_of_a_missing_interface() {
    let records = vec![
        Row::new(
            "export-guide",
            "export_fbx",
            "Export the current scene to FBX interchange.",
        )
        .with_tools(None),
    ];

    let page = search_page_with_fallback(
        &records,
        &SearchQuery {
            query: "export_fbx".to_string(),
            ..Default::default()
        },
        ready_probe(),
    );

    assert!(
        page.fallback.is_none(),
        "a row type that does not model tools must not trip the route"
    );
}

#[test]
fn a_near_miss_falls_back_with_the_low_confidence_reason() {
    // A deliberately tight policy isolates the low-confidence branch from the
    // no-candidate branch without depending on scorer internals.
    let records = corpus();
    let query = SearchQuery {
        query: "sphere".to_string(),
        ..Default::default()
    };
    let hits = dcc_mcp_gateway_search::search(&records, &query);
    let top = hits.first().expect("the query matches something").score;
    assert!(
        top >= dcc_mcp_gateway_search::FALLBACK_MIN_CONFIDENT_SCORE,
        "precondition: the default policy must not fire on this query (top-1 = {top})"
    );

    let strict = FallbackPolicy {
        min_confident_score: top + 1,
        ..FallbackPolicy::default()
    };
    let page = dcc_mcp_gateway_search::search_page_with_fallback(&records, &query, ready_probe());
    assert!(page.fallback.is_none());

    let fallback = dcc_mcp_gateway_search::resolve_fallback_with_policy(
        &hits,
        &query.query,
        ready_probe(),
        strict,
    )
    .expect("raising the threshold must tighten the route");
    assert_eq!(fallback.reason, FALLBACK_REASON_LOW_CONFIDENCE);
    assert_eq!(fallback.skill, FALLBACK_SKILL);
}

#[test]
fn reason_codes_distinguish_low_confidence_from_no_interface() {
    assert_ne!(
        FALLBACK_REASON_LOW_CONFIDENCE, FALLBACK_REASON_NO_EXECUTABLE_INTERFACE,
        "the caller must be able to tell the two headline cases apart"
    );
}

// ── 3. An unusable dcc-cua blocks instead of re-routing ──────────────────────

#[test]
fn an_unusable_runtime_blocks_the_route_and_never_changes_provider() {
    for state in [
        CuaRuntimeState::NotResponding,
        CuaRuntimeState::Missing,
        CuaRuntimeState::Incompatible,
        CuaRuntimeState::Unknown,
    ] {
        let probe = StaticCuaProbe::new(state);
        let page = search_page_with_fallback(
            &corpus(),
            &SearchQuery {
                query: "wizard".to_string(),
                ..Default::default()
            },
            &probe,
        );

        let fallback = page
            .fallback
            .unwrap_or_else(|| panic!("{state:?} must still report why the route was needed"));
        assert!(fallback.blocked, "{state:?} must block, not re-route");
        assert_eq!(
            fallback.blocked_reason.as_deref(),
            Some(FALLBACK_REASON_CUA_UNAVAILABLE),
            "{state:?} must carry a distinguishable blocker reason code"
        );
        assert_eq!(
            fallback.skill, FALLBACK_SKILL,
            "{state:?} must never substitute another provider"
        );
        // The original trigger survives, so the caller still knows why.
        assert_eq!(fallback.reason, FALLBACK_REASON_NO_CANDIDATE);
        assert!(
            fallback
                .preflight
                .iter()
                .any(|c| c.contains("components ensure")),
            "{state:?} advice must point at the official repair command"
        );
        assert!(
            !fallback.message.is_empty(),
            "the caller needs one sentence to hand to the user"
        );
    }
}

#[test]
fn a_missing_runtime_still_reports_the_hits_it_found() {
    let page = search_page_with_fallback(
        &corpus(),
        &SearchQuery {
            query: "sphere".to_string(),
            ..Default::default()
        },
        &MISSING,
    );
    assert_eq!(
        page.fallback, None,
        "this query is answered; no advice at all"
    );
    assert!(!page.hits.is_empty());
}

#[test]
fn the_route_never_names_a_forbidden_substitute() {
    let probe = StaticCuaProbe::new(CuaRuntimeState::Missing);
    let page = search_page_with_fallback(
        &corpus(),
        &SearchQuery {
            query: "wizard".to_string(),
            ..Default::default()
        },
        &probe,
    );
    let fallback = page.fallback.expect("blocked advice is still advice");
    for forbidden in dcc_mcp_gateway_search::FORBIDDEN_SUBSTITUTES {
        assert!(
            !fallback.message.contains(forbidden) && fallback.skill != *forbidden,
            "the contract forbids suggesting {forbidden}"
        );
    }
}
