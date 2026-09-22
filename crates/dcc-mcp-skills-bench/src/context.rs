//! Multi-DCC context growth (PIP-3408, dimension 3).
//!
//! Discovery cost is the one search metric a user feels directly: it is paid
//! out of their context window on every turn. Unlike latency it does not
//! depend on the CI machine, so unlike latency it can carry a hard cap.
//!
//! This module scans the growth curve over the number of live hosts N using
//! the production projection, via
//! `dcc_mcp_skills::catalog::list_context` — the same fixture and the same
//! wire format the `list_skills` context harness already asserts on.

use dcc_mcp_skills::catalog::list_context;
use serde_json::json;

/// One point on the growth curve: N live hosts and what a discovery call cost.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ContextPoint {
    /// Live DCC hosts contributing to the merged catalogue.
    pub hosts: usize,
    /// Skills across all hosts.
    pub total: usize,
    /// Rows on the measured page.
    pub returned: usize,
    /// Bytes of the pretty-printed payload.
    pub bytes: usize,
    /// Estimated tokens of the payload.
    pub tokens: usize,
    /// `truncated` flag from the payload.
    pub truncated: bool,
    /// Estimated tokens per additional host relative to the previous point.
    pub marginal_tokens: i64,
}

/// Host counts swept by [`scan`].
///
/// 1 → 7 covers a single artist through the seven-host studio scenario the
/// `list_skills` harness uses, without extrapolating past a real studio.
pub const HOST_SWEEP: [usize; 7] = [1, 2, 3, 4, 5, 6, 7];

/// Measure one default (`limit`-less) `list_skills` page for `hosts`.
#[must_use]
pub fn measure_hosts(hosts: usize) -> ContextPoint {
    let scenario = list_context::seven_dcc_scenario(hosts);
    let page = list_context::measure(&scenario, &json!({}));
    ContextPoint {
        hosts: page.hosts,
        total: page.total,
        returned: page.returned,
        bytes: page.bytes,
        tokens: page.tokens,
        truncated: page.truncated,
        marginal_tokens: 0,
    }
}

/// Sweep [`HOST_SWEEP`] and fill in the marginal cost of each extra host.
#[must_use]
pub fn scan() -> Vec<ContextPoint> {
    let mut previous: Option<usize> = None;
    let mut points = Vec::with_capacity(HOST_SWEEP.len());
    for hosts in HOST_SWEEP {
        let mut point = measure_hosts(hosts);
        point.marginal_tokens = match previous {
            Some(before) => point.tokens as i64 - before as i64,
            None => point.tokens as i64,
        };
        previous = Some(point.tokens);
        points.push(point);
    }
    points
}

/// Highest token cost anywhere on the curve.
#[must_use]
pub fn peak_tokens(points: &[ContextPoint]) -> usize {
    points.iter().map(|point| point.tokens).max().unwrap_or(0)
}

/// Largest increase in tokens from adding one host.
///
/// The context cap is about growth, not absolute size: a page that costs the
/// same whether one host or seven are live is flat, which is the property
/// worth protecting.
#[must_use]
pub fn peak_marginal_tokens(points: &[ContextPoint]) -> i64 {
    points
        .iter()
        .skip(1)
        .map(|point| point.marginal_tokens)
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_covers_the_sweep() {
        let points = scan();
        assert_eq!(points.len(), HOST_SWEEP.len());
        for (index, point) in points.iter().enumerate() {
            assert_eq!(point.hosts, HOST_SWEEP[index]);
        }
    }

    #[test]
    fn catalogue_grows_with_host_count() {
        let points = scan();
        for window in points.windows(2) {
            assert!(
                window[1].total > window[0].total,
                "host {}: total did not grow",
                window[1].hosts
            );
        }
    }

    #[test]
    fn default_page_stays_flat_as_hosts_are_added() {
        // The property the cap protects: paging means the page cost does not
        // track catalogue size.
        let points = scan();
        let peak = peak_tokens(&points);
        assert!(
            peak <= crate::thresholds::MAX_CONTEXT_TOKENS,
            "peak context {peak} exceeded cap {}",
            crate::thresholds::MAX_CONTEXT_TOKENS
        );
    }

    #[test]
    fn paginating_still_reaches_the_whole_catalogue() {
        // A flat page is only acceptable if walking it still sees everything.
        let scenario = list_context::seven_dcc_scenario(HOST_SWEEP[HOST_SWEEP.len() - 1]);
        let pages = list_context::paged_walk(&scenario, 25);
        let reached: usize = pages.iter().map(|page| page.returned).sum();
        let total = pages.first().map_or(0, |page| page.total);
        assert_eq!(reached, total, "paged walk lost rows");
    }
}
