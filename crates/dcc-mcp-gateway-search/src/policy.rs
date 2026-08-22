//! Shared ranking policy applied by every search surface.

/// Domain skill layer.
pub const LAYER_DOMAIN: &str = "domain";
/// Thin script/CLI harness layer.
pub const LAYER_THIN_HARNESS: &str = "thin-harness";
/// Infrastructure fallback layer.
pub const LAYER_INFRASTRUCTURE: &str = "infrastructure";
/// Authoring example layer, hidden unless explicitly requested.
pub const LAYER_EXAMPLE: &str = "example";

/// Unknown discovery source.
pub const PATH_SOURCE_UNKNOWN: &str = "unknown";
/// Package-bundled discovery source.
pub const PATH_SOURCE_BUNDLED: &str = "bundled";
/// Platform-wide discovery source.
pub const PATH_SOURCE_PLATFORM: &str = "platform";
/// Local development discovery source.
pub const PATH_SOURCE_LOCAL_DEV: &str = "local_dev";
/// Environment-configured discovery source.
pub const PATH_SOURCE_ENV_VAR: &str = "env_var";
/// Explicit caller-provided discovery source.
pub const PATH_SOURCE_EXPLICIT_ARG: &str = "explicit_arg";
/// Admin-configured discovery source.
pub const PATH_SOURCE_ADMIN_CUSTOM: &str = "admin_custom";

const LAYER_MULT_INFRASTRUCTURE: f64 = 0.35;
const LAYER_MULT_THIN_HARNESS: f64 = 0.20;
const PATH_SOURCE_MULT_BUNDLED: f64 = 0.70;
const PATH_SOURCE_MULT_PLATFORM: f64 = 0.85;

/// Context controlling policy exceptions for an explicit search intent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RankPolicy {
    /// The query is an exact, case-insensitive record-name match.
    pub exact_name: bool,
    /// The caller explicitly filtered by a known architectural layer.
    pub explicit_layer: bool,
}

/// Coefficient for an architectural skill layer.
///
/// `None` excludes authoring examples from neutral discovery.
#[must_use]
pub fn layer_multiplier(layer: Option<&str>, explicit: bool) -> Option<f64> {
    if explicit {
        return Some(1.0);
    }
    match layer.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case(LAYER_EXAMPLE) => None,
        Some(value) if value.eq_ignore_ascii_case(LAYER_INFRASTRUCTURE) => {
            Some(LAYER_MULT_INFRASTRUCTURE)
        }
        Some(value) if value.eq_ignore_ascii_case(LAYER_THIN_HARNESS) => {
            Some(LAYER_MULT_THIN_HARNESS)
        }
        _ => Some(1.0),
    }
}

/// Coefficient for a discovery path source.
#[must_use]
pub fn path_source_multiplier(source: Option<&str>) -> f64 {
    match source.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case(PATH_SOURCE_BUNDLED) => PATH_SOURCE_MULT_BUNDLED,
        Some(value) if value.eq_ignore_ascii_case(PATH_SOURCE_PLATFORM) => {
            PATH_SOURCE_MULT_PLATFORM
        }
        _ => 1.0,
    }
}

/// Apply shared layer/source policy to a raw scorer result.
///
/// Exact-name searches bypass all dampening and may surface examples.
#[must_use]
pub fn apply_rank_policy(
    raw_score: u32,
    layer: Option<&str>,
    path_source: Option<&str>,
    policy: RankPolicy,
) -> Option<u32> {
    if policy.exact_name {
        return Some(raw_score);
    }
    let layer = layer_multiplier(layer, policy.explicit_layer)?;
    let multiplier = layer * path_source_multiplier(path_source);
    let adjusted = (f64::from(raw_score) * multiplier).round() as u32;
    Some(if raw_score > 0 { adjusted.max(1) } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_search_demotes_fallback_layers_and_bundled_sources() {
        assert_eq!(
            apply_rank_policy(
                100,
                Some(LAYER_INFRASTRUCTURE),
                Some(PATH_SOURCE_BUNDLED),
                RankPolicy::default(),
            ),
            Some(24)
        );
        assert_eq!(
            apply_rank_policy(
                100,
                Some(LAYER_THIN_HARNESS),
                Some(PATH_SOURCE_PLATFORM),
                RankPolicy::default(),
            ),
            Some(17)
        );
    }

    #[test]
    fn examples_require_explicit_layer_or_exact_name() {
        assert_eq!(
            apply_rank_policy(100, Some(LAYER_EXAMPLE), None, RankPolicy::default(),),
            None
        );
        assert_eq!(
            apply_rank_policy(
                100,
                Some(LAYER_EXAMPLE),
                None,
                RankPolicy {
                    explicit_layer: true,
                    ..RankPolicy::default()
                },
            ),
            Some(100)
        );
        assert_eq!(
            apply_rank_policy(
                100,
                Some(LAYER_EXAMPLE),
                Some(PATH_SOURCE_BUNDLED),
                RankPolicy {
                    exact_name: true,
                    ..RankPolicy::default()
                },
            ),
            Some(100)
        );
    }
}
