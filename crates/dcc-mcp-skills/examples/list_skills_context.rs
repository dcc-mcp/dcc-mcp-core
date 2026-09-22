//! Reproducible `list_skills` context-budget measurement (issue PIP-3407).
//!
//! Prints the `hosts -> rows -> tokens` table for the seven-DCC studio
//! scenario, then walks every page to show that pagination still reaches the
//! whole catalogue.
//!
//! ```text
//! cargo run --example list_skills_context -p dcc-mcp-skills
//! ```

use dcc_mcp_skills::catalog::list_context::{
    MAX_DEFAULT_PAGE_TOKENS, SEVEN_DCC_COUNTS, TOKEN_ESTIMATOR_ID, measure, paged_walk,
    seven_dcc_scenario,
};
use dcc_mcp_skills::catalog::list_projection::{DEFAULT_LIST_SKILLS_LIMIT, MAX_LIST_SKILLS_LIMIT};
use serde_json::json;

/// The compact field set `list_skills` returned before PIP-3407. Kept here so
/// the trimming decision stays measurable instead of folklore.
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

fn main() {
    println!("estimator: {TOKEN_ESTIMATOR_ID} (bytes / 4, ±10%)");
    println!("scenario:  {SEVEN_DCC_COUNTS:?}");
    println!();

    println!("default page (no `limit` argument)");
    println!(
        "{:>5}  {:>6}  {:>6}  {:>8}  {:>8}  {:>9}",
        "hosts", "total", "rows", "bytes", "tokens", "truncated"
    );
    for hosts in 1..=7 {
        let scenario = seven_dcc_scenario(hosts);
        let m = measure(&scenario, &serde_json::json!({}));
        println!(
            "{:>5}  {:>6}  {:>6}  {:>8}  {:>8}  {:>9}",
            m.hosts, m.total, m.returned, m.bytes, m.tokens, m.truncated
        );
    }
    println!();
    println!("budget: max default page = {MAX_DEFAULT_PAGE_TOKENS} tokens");
    println!();

    println!("field-set cost on a 25-row page (seven-host scenario)");
    let scenario = seven_dcc_scenario(7);
    for (label, fields) in [
        ("name only", vec!["name"]),
        ("name + dcc + summary", vec!["name", "dcc", "summary"]),
        ("slim default (omitted `fields`)", vec![]),
        ("pre-PIP-3407 compact set", LEGACY_COMPACT_FIELDS.to_vec()),
        (
            "description instead of summary",
            vec!["name", "dcc", "description"],
        ),
    ] {
        let args = if fields.is_empty() {
            json!({"limit": DEFAULT_LIST_SKILLS_LIMIT})
        } else {
            json!({"limit": DEFAULT_LIST_SKILLS_LIMIT, "fields": fields})
        };
        let m = measure(&scenario, &args);
        println!(
            "  {:<34} rows {:>3}  bytes {:>6}  tokens {:>5}  ({:>3} tokens/row)",
            label,
            m.returned,
            m.bytes,
            m.tokens,
            m.tokens / m.returned.max(1)
        );
    }
    println!();
    println!("worst-case bounded page (limit = {MAX_LIST_SKILLS_LIMIT}, max allowed)");
    let worst = measure(&scenario, &json!({"limit": MAX_LIST_SKILLS_LIMIT}));
    println!(
        "  rows {}  bytes {}  tokens {}",
        worst.returned, worst.bytes, worst.tokens
    );
    println!();

    println!("paged walk (limit = {DEFAULT_LIST_SKILLS_LIMIT}) over the seven-host scenario");
    let pages = paged_walk(&scenario, DEFAULT_LIST_SKILLS_LIMIT);
    let reached: usize = pages.iter().map(|p| p.returned).sum();
    let worst = pages.iter().map(|p| p.tokens).max().unwrap_or(0);
    for (i, page) in pages.iter().enumerate() {
        println!(
            "  page {:<2} rows {:>3}  tokens {:>5}  next_offset {:?}",
            i, page.returned, page.tokens, page.next_offset
        );
    }
    println!();
    println!(
        "  pages: {}  rows reached: {} / {}  worst page: {} tokens",
        pages.len(),
        reached,
        pages.first().map_or(0, |p| p.total),
        worst
    );
    if reached == pages.first().map_or(0, |p| p.total) {
        println!("  full catalogue reachable by paging: yes");
    } else {
        println!("  full catalogue reachable by paging: NO");
        std::process::exit(1);
    }
}
