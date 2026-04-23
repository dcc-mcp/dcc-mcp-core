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
