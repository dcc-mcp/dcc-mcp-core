/// Parse a semver string (`"0.12.29"`, `"v1.2.3-rc1"`) into a comparable triple.
///
/// Handles common variants:
/// - Leading `v` prefix stripped (`"v0.12.29"` → `(0, 12, 29)`)
/// - Pre-release suffixes ignored (`"1.0.0-rc1"` → `(1, 0, 0)`)
/// - Missing components default to `0` (`"1.2"` → `(1, 2, 0)`)
pub(crate) fn parse_semver(v: &str) -> (u64, u64, u64) {
    let parts: Vec<u64> = v
        .trim_start_matches('v')
        .split('.')
        .filter_map(|seg| seg.split('-').next()?.parse::<u64>().ok())
        .collect();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}

/// Returns `true` when `candidate` is strictly newer than `current`.
///
/// Uses numeric semver comparison, so `"0.12.29"` > `"0.12.6"`.
pub(crate) fn is_newer_version(candidate: &str, current: &str) -> bool {
    parse_semver(candidate) > parse_semver(current)
}

/// Election metadata describing one side of a gateway tiebreak (issue maya#137).
///
/// The comparison happens in three layers, each only consulted when the
/// previous layer is exactly equal:
///
/// 1. **Crate version** — the embedded `dcc-mcp-http` semver.
/// 2. **Adapter version** — the package wrapping the gateway (e.g.
///    `dcc_mcp_maya = "0.3.0"`); `None` is treated as the lowest value so a
///    challenger that exposes its adapter version always beats a peer that
///    omitted it.
/// 3. **Real-DCC tiebreaker** — when the resident sentinel has no
///    `adapter_dcc` set or reports `"unknown"` (a generic standalone
///    server) and the challenger advertises a real DCC, the challenger
///    wins.  Two real DCCs of the same crate+adapter version remain tied
///    so peers fall back to the existing first-wins port-bind contract.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ElectionInfo<'a> {
    pub(crate) crate_version: &'a str,
    pub(crate) adapter_version: Option<&'a str>,
    pub(crate) adapter_dcc: Option<&'a str>,
}

impl<'a> ElectionInfo<'a> {
    pub(crate) fn new(
        crate_version: &'a str,
        adapter_version: Option<&'a str>,
        adapter_dcc: Option<&'a str>,
    ) -> Self {
        Self {
            crate_version,
            adapter_version,
            adapter_dcc,
        }
    }
}

/// Treat `None`, empty, and `"unknown"` (case-insensitive) as the generic
/// standalone bucket.  Any other value is considered a real DCC for the
/// election tiebreaker.
fn is_unknown_dcc(dcc: Option<&str>) -> bool {
    match dcc {
        None => true,
        Some(s) => s.is_empty() || s.eq_ignore_ascii_case("unknown"),
    }
}

/// Three-layer election comparison (issue maya#137).
///
/// Returns `true` when `candidate` should preempt `current`.
pub(crate) fn is_newer_election(candidate: ElectionInfo<'_>, current: ElectionInfo<'_>) -> bool {
    let cand_crate = parse_semver(candidate.crate_version);
    let cur_crate = parse_semver(current.crate_version);
    if cand_crate != cur_crate {
        return cand_crate > cur_crate;
    }

    // Layer 2 — adapter version.  `None` is below any concrete value.
    let cand_adapter = candidate.adapter_version.map(parse_semver);
    let cur_adapter = current.adapter_version.map(parse_semver);
    if cand_adapter != cur_adapter {
        return cand_adapter > cur_adapter;
    }

    // Layer 3 — prefer real DCC over generic standalone.
    is_unknown_dcc(current.adapter_dcc) && !is_unknown_dcc(candidate.adapter_dcc)
}
