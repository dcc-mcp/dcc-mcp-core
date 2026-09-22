//! Multi-DCC context growth gate (PIP-3408).
//!
//! Discovery cost is the one search metric a user feels directly: it is paid
//! out of their context window on every turn. Unlike latency it does not
//! depend on the CI machine, so unlike latency it carries a hard cap.
//!
//! Both halves of the cap matter and they fail for different reasons:
//!
//! * [`default_page_stays_within_the_token_cap`] — the page must fit the
//!   budget no matter how many hosts are live.
//! * [`adding_a_host_does_not_grow_the_page`] — the page must not *track* the
//!   catalogue. A flat page that is merely under the cap today still grows
//!   into the cap tomorrow if the per-host cost is unbounded.

use dcc_mcp_skills_bench::context::{self, HOST_SWEEP};
use dcc_mcp_skills_bench::thresholds;

#[test]
fn default_page_stays_within_the_token_cap() {
    let curve = context::scan();
    let peak = context::peak_tokens(&curve);
    assert!(
        peak <= thresholds::MAX_CONTEXT_TOKENS,
        "default list_skills page peaked at {peak} tokens (cap {});\n{curve_note}",
        thresholds::MAX_CONTEXT_TOKENS,
        curve_note = note(&curve)
    );
}

#[test]
fn adding_a_host_does_not_grow_the_page() {
    let curve = context::scan();
    let peak = context::peak_marginal_tokens(&curve);
    assert!(
        peak <= thresholds::MAX_MARGINAL_CONTEXT_TOKENS,
        "adding one host cost up to {peak} tokens (cap {}); the page is tracking \
         catalogue size instead of page size.\n{curve_note}",
        thresholds::MAX_MARGINAL_CONTEXT_TOKENS,
        curve_note = note(&curve)
    );
}

#[test]
fn the_curve_is_measured_at_every_host_count() {
    let curve = context::scan();
    assert_eq!(curve.len(), HOST_SWEEP.len());
    for point in &curve {
        assert!(point.returned > 0, "host {} returned nothing", point.hosts);
        assert!(
            point.total > 0,
            "host {} has an empty catalogue",
            point.hosts
        );
    }
}

/// The growth curve, printed with every failure.
fn note(curve: &[context::ContextPoint]) -> String {
    let mut lines = vec!["hosts  skills  returned  tokens  marginal".to_string()];
    for point in curve {
        lines.push(format!(
            "{:>5}  {:>6}  {:>8}  {:>6}  {:>8}",
            point.hosts, point.total, point.returned, point.tokens, point.marginal_tokens
        ));
    }
    lines.join("\n")
}
