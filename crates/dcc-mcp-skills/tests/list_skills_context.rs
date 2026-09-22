//! Context-budget regression tests for `list_skills` (issue PIP-3407).
//!
//! `list_skills` fans out to every live DCC host and merges the results, so a
//! default call used to return the whole studio catalogue in one tool result:
//! seven hosts shipped 167 skills in a single unbounded page. These tests pin
//! the two properties that must hold for any host count `N`:
//!
//! 1. a default page is **bounded** — row count and token cost stay flat as
//!    hosts are added, instead of growing linearly with `N`;
//! 2. the bound does **not lose capability** — walking `next_offset` still
//!    reaches every skill exactly once.
//!
//! The fixture and the token estimator live in
//! [`dcc_mcp_skills::catalog::list_context`]; run
//! `cargo run --example list_skills_context -p dcc-mcp-skills` to print the
//! table these thresholds come from.

use dcc_mcp_skills::catalog::list_context::{
    MAX_DEFAULT_PAGE_TOKENS, measure, paged_walk, seven_dcc_scenario,
};
use dcc_mcp_skills::catalog::list_projection::{DEFAULT_LIST_SKILLS_LIMIT, MAX_LIST_SKILLS_LIMIT};
use serde_json::json;

/// The compact field set `list_skills` returned before PIP-3407, used to keep
/// the trimming decision honest (the slim projection must stay measurably
/// cheaper).
const LEGACY_COMPACT_FIELDS: &[&str] = &[
    "name",
    "stage",
    "tool_count",
    "loaded",
    "status",
    "missing_dependencies",
    "scope",
    "summary",
    "dcc",
    "version",
    "layer",
    "runtime_state",
    "implicit_invocation",
];

/// One to seven live DCC hosts, the studio scenario the issue measured.
fn scenarios() -> Vec<Vec<(&'static str, usize)>> {
    (1..=7).map(seven_dcc_scenario).collect()
}

#[test]
fn default_page_is_bounded_for_every_host_count() {
    for hosts in scenarios() {
        let m = measure(&hosts, &json!({}));
        assert!(
            m.returned <= DEFAULT_LIST_SKILLS_LIMIT,
            "N={} returned {} rows, cap is {}",
            m.hosts,
            m.returned,
            DEFAULT_LIST_SKILLS_LIMIT
        );
        assert!(
            m.tokens <= MAX_DEFAULT_PAGE_TOKENS,
            "N={} default page costs {} tokens, budget is {}",
            m.hosts,
            m.tokens,
            MAX_DEFAULT_PAGE_TOKENS
        );
    }
}

#[test]
fn context_cost_does_not_grow_with_host_count() {
    // The regression: cost used to scale with the number of live hosts. With a
    // bounded page, adding a seventh host may only change which skills land on
    // the page, not how much it costs.
    let one_host = measure(&seven_dcc_scenario(1), &json!({}));
    let seven_hosts = measure(&seven_dcc_scenario(7), &json!({}));
    assert_eq!(seven_hosts.total, 167, "seven-host catalogue size");
    let growth = seven_hosts.tokens as f64 / one_host.tokens as f64;
    assert!(
        growth < 1.10,
        "cost grew {growth:.2}x from N=1 ({}) to N=7 ({})",
        one_host.tokens,
        seven_hosts.tokens
    );
}

#[test]
fn seven_host_catalogue_no_longer_fits_on_one_page() {
    let m = measure(&seven_dcc_scenario(7), &json!({}));
    assert!(m.total > MAX_LIST_SKILLS_LIMIT);
    assert!(m.truncated, "a 167-skill catalogue must report truncation");
    assert_eq!(m.next_offset, Some(DEFAULT_LIST_SKILLS_LIMIT));
    // The pre-fix behaviour returned all 167 rows; 50 (the hard cap) alone
    // already blows the default-page budget, which is why the bound is the fix.
    let capped = measure(
        &seven_dcc_scenario(7),
        &json!({"limit": MAX_LIST_SKILLS_LIMIT}),
    );
    assert!(capped.tokens > MAX_DEFAULT_PAGE_TOKENS);
}

#[test]
fn paging_reaches_every_skill_exactly_once() {
    for hosts in scenarios() {
        let pages = paged_walk(&hosts, DEFAULT_LIST_SKILLS_LIMIT);
        let expected_total = hosts.iter().map(|(_, n)| n).sum::<usize>();
        let reached: usize = pages.iter().map(|p| p.returned).sum();
        assert_eq!(
            reached,
            expected_total,
            "N={} paging reached {reached} of {expected_total} skills",
            hosts.len()
        );
        assert!(
            pages.len() >= expected_total.div_ceil(DEFAULT_LIST_SKILLS_LIMIT),
            "N={} took only {} pages",
            hosts.len(),
            pages.len()
        );
        let last = pages.last().expect("at least one page");
        assert!(
            last.next_offset.is_none(),
            "N={} walk did not terminate",
            hosts.len()
        );
        assert!(
            !last.truncated,
            "N={} last page reports truncation",
            hosts.len()
        );
    }
}

#[test]
fn pages_are_disjoint() {
    // A merged fan-out pages over the union, so no skill may appear twice and
    // none may be skipped between pages.
    let hosts = seven_dcc_scenario(7);
    let mut offsets: Vec<usize> = vec![0];
    loop {
        let page = measure(&hosts, &json!({"offset": offsets.last().copied().unwrap()}));
        match page.next_offset {
            Some(next) => offsets.push(next),
            None => break,
        }
    }
    let mut unique = offsets.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), offsets.len(), "next_offset repeated a page");
    assert_eq!(
        offsets.last().copied().unwrap(),
        167 - (167 % DEFAULT_LIST_SKILLS_LIMIT),
        "last page offset"
    );
}

#[test]
fn explicit_fields_still_reach_the_heavy_columns() {
    // Bounding the page must not cost capability: a caller that asks for the
    // columns the default page omits still gets them.
    let hosts = seven_dcc_scenario(7);
    let page = measure(
        &hosts,
        &json!({"limit": 5, "fields": ["name", "description", "tags", "tool_names", "version"]}),
    );
    assert_eq!(page.returned, 5);
    // The full description is ~4x the truncated summary, so this page must be
    // measurably heavier — proof the projection is not dropping the data.
    let slim = measure(&hosts, &json!({"limit": 5}));
    assert!(
        page.tokens > slim.tokens,
        "full-fields page ({}) should exceed the slim page ({})",
        page.tokens,
        slim.tokens
    );
}

#[test]
fn slim_projection_is_cheaper_than_the_legacy_field_set() {
    let hosts = seven_dcc_scenario(7);
    let slim = measure(&hosts, &json!({"limit": DEFAULT_LIST_SKILLS_LIMIT}));
    let legacy = measure(
        &hosts,
        &json!({"limit": DEFAULT_LIST_SKILLS_LIMIT, "fields": LEGACY_COMPACT_FIELDS}),
    );
    let saved = 1.0 - (slim.tokens as f64 / legacy.tokens as f64);
    assert!(
        saved >= 0.20,
        "slim page ({}) saves only {:.0}% over the legacy field set ({})",
        slim.tokens,
        saved * 100.0,
        legacy.tokens
    );
    assert!(slim.tokens < legacy.tokens);
}
