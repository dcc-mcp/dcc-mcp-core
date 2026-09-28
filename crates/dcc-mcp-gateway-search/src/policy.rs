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

// ── Explicit dcc-cua fallback route (PIP-3702) ───────────────────────────────
//
// Every constant and every judgement below lives in this module on purpose.
// The fallback must never become a hard-coded `if` at a call site, because the
// whole point of the contract is that the trigger is reviewable in one place.

/// Skill that owns the fallback route for tasks with no scriptable interface.
///
/// This is the **only** route the fallback may ever recommend. The
/// non-substitution contract in `skills/dcc-cua/SKILL.md` forbids quietly
/// swapping in a generic computer-use provider when this one is unavailable.
pub const FALLBACK_SKILL: &str = "dcc-cua";

/// Reason code: nothing in the index scored above zero for the query.
pub const FALLBACK_REASON_NO_CANDIDATE: &str = "no_candidate";
/// Reason code: the best candidate scored below [`FallbackPolicy::min_confident_score`].
pub const FALLBACK_REASON_LOW_CONFIDENCE: &str = "low_confidence";
/// Reason code: the best candidate declares no executable tool interface
/// (empty `tools.yaml` / no tool declaration).
pub const FALLBACK_REASON_NO_EXECUTABLE_INTERFACE: &str = "no_executable_interface";
/// Reason code: the query needed the fallback, but `dcc-cua` itself cannot be
/// used. This is a hard blocker — never a licence to change provider.
pub const FALLBACK_REASON_CUA_UNAVAILABLE: &str = "cua_runtime_unavailable";

/// Default top-1 score at or above which a hit counts as confident.
///
/// Calibrated against two independently measured query-shape families, both
/// run on the production [`crate::FuzzyScorer`]. They disagree, and the
/// disagreement is the whole point of recording them together.
///
/// **Family A — name-match shapes** (measured on the full catalog):
///
/// | query shape | top-1 score |
/// |---|---|
/// | tokens match the skill *and* tool name (`maya shot export`) | 104 |
/// | full multi-token name match (`photoshop gui workflow`) | 83 |
/// | name tokens, partial (`bake textures`) | 77 |
/// | single-token name prefix/substring (`export`, `maya`, `photoshop`) | 28–38 |
/// | description-only noise (`render the farm queue`) | 5 |
/// | nothing matched | 0 |
///
/// This family shows a **clean, empty band between 5 and 28**.
///
/// **Family B — authored `search-hint` queries** (`benchmarks/skills/seeds.json`,
/// 26 real seeds; the first phrase of each skill's own `search-hint`, i.e. what
/// the skill author expects users to type):
///
/// | query set | top-1 score | fires below 16 |
/// |---|---|---|
/// | GUI-only tasks no skill can serve (n=10) | 5–15 | 10 / 10 |
/// | authored `search-hint` queries the corpus answers (n=26) | 8 and up | 9 / 26 |
/// | skill-name queries (n=26) | 52 and up | 0 / 26 |
///
/// This family shows the two distributions **overlapping in the 8–15 band**.
/// The 9 / 26 false positives are reproduced by
/// `tests/fallback_threshold_calibration.rs`; they are:
///
/// `greeting` (8), `layered architecture` (8), `modeling recipe` (9),
/// `USD stage` (10), `create sphere` (10), `chain` (11), `cancellation` (14),
/// `screenshot` (14), `structured schema` (15).
///
/// Note that these are **not** all generic one- and two-word queries —
/// `create sphere`, `structured schema`, `modeling recipe` and `layered
/// architecture` are domain-bearing multi-word queries, and `create sphere` is
/// a task the corpus can genuinely answer. The low scores are a property of
/// the corpus, not of the ranker: S1's vocabulary work raises them and clears
/// them at this same threshold.
///
/// **Reading the two together:** the gate is a recall-versus-noise trade-off,
/// not a clean separation. It is deliberately biased toward recall. The
/// fallback is *additive advice* — the hits are still returned — so a false
/// positive costs the caller one extra suggestion, while a false negative costs
/// the user the only exit they had. 16 sits above every measured
/// description-noise score and below every measured name match, and never
/// touches a name query.
///
/// [`pinning_test`]: the gap itself is asserted by
/// `tests/fallback_dcc_cua_route.rs::the_confidence_gate_sits_in_the_measured_gap`,
/// so a scorer change that compresses the band fails a test instead of silently
/// mis-routing traffic.
pub const FALLBACK_MIN_CONFIDENT_SCORE: u32 = 16;

/// Shortest query, in characters after trimming, that may be routed to the
/// fallback.
///
/// Bounded discovery requests (empty or near-empty queries) return a page of
/// everything and are never "unanswerable", so they must never fall back.
pub const FALLBACK_MIN_QUERY_LEN: usize = 3;

/// Why a result set was judged unable to serve the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackTrigger {
    /// No candidate scored above zero.
    NoCandidate,
    /// The best candidate is a lexical near-miss rather than a real match.
    LowConfidence,
    /// The best candidate matches on text but exposes nothing to call.
    NoExecutableInterface,
}

impl FallbackTrigger {
    /// Stable, machine-readable reason code for this trigger.
    #[must_use]
    pub fn reason_code(self) -> &'static str {
        match self {
            Self::NoCandidate => FALLBACK_REASON_NO_CANDIDATE,
            Self::LowConfidence => FALLBACK_REASON_LOW_CONFIDENCE,
            Self::NoExecutableInterface => FALLBACK_REASON_NO_EXECUTABLE_INTERFACE,
        }
    }
}

/// Thresholds for the explicit `dcc-cua` fallback route.
///
/// Every field defaults to the module constant of the same name, so a caller
/// can tune the route without any call site growing a hard-coded `if`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallbackPolicy {
    /// Top-1 score at or above which a hit is confident.
    pub min_confident_score: u32,
    /// Shortest query, in characters, eligible for the fallback.
    pub min_query_len: usize,
    /// Master switch. Disabled means the route never fires and never probes.
    pub enabled: bool,
}

impl Default for FallbackPolicy {
    fn default() -> Self {
        Self {
            min_confident_score: FALLBACK_MIN_CONFIDENT_SCORE,
            min_query_len: FALLBACK_MIN_QUERY_LEN,
            enabled: true,
        }
    }
}

/// Whether `candidate` names the fallback target itself.
///
/// Comparison is case-insensitive because skill names come from hand-written
/// `SKILL.md` frontmatter and are not normalised before they reach the ranker.
///
/// Used to stop the route from recommending `dcc-cua` for a request that
/// `dcc-cua` itself already answered. That is not a fallback, it is a
/// tautology — and it would look to the caller like a loop.
#[must_use]
pub fn is_fallback_target(candidate: Option<&str>) -> bool {
    candidate.is_some_and(|value| value.trim().eq_ignore_ascii_case(FALLBACK_SKILL))
}

/// Decide whether a result set must be routed to the `dcc-cua` fallback.
///
/// This is the single judgement behind the route. It is deliberately pure so
/// the whole contract can be tested without a runtime, a network, or a
/// `dcc-cua` binary.
///
/// * `query_len` — trimming/lowercasing is the caller's job; pass the number of
///   characters in the trimmed query.
/// * `top_score` — score of the rank-1 hit, or `None` when there is no hit.
/// * `top_has_interface` — whether the rank-1 record exposes at least one
///   callable tool. `true` when the count is unknown: absence of evidence is
///   not evidence of absence, and guessing otherwise would fire the route on
///   every record type that simply does not model tools.
///
/// Triggers are checked weakest-signal-last: an empty result is reported as
/// [`FallbackTrigger::NoCandidate`] rather than as a low score, because that is
/// the actionable distinction for the caller.
#[must_use]
pub fn evaluate_fallback(
    query_len: usize,
    top_score: Option<u32>,
    top_has_interface: bool,
    policy: FallbackPolicy,
) -> Option<FallbackTrigger> {
    if !policy.enabled || query_len < policy.min_query_len {
        return None;
    }
    let Some(score) = top_score else {
        return Some(FallbackTrigger::NoCandidate);
    };
    if score < policy.min_confident_score {
        return Some(FallbackTrigger::LowConfidence);
    }
    if !top_has_interface {
        return Some(FallbackTrigger::NoExecutableInterface);
    }
    None
}

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

    // ── fallback route (PIP-3702) ─────────────────────────────────────────

    const LONG_QUERY: usize = 32;

    #[test]
    fn confident_hit_with_an_interface_never_falls_back() {
        assert_eq!(
            evaluate_fallback(
                LONG_QUERY,
                Some(FALLBACK_MIN_CONFIDENT_SCORE),
                true,
                FallbackPolicy::default(),
            ),
            None,
            "a score exactly on the threshold is confident"
        );
        assert_eq!(
            evaluate_fallback(LONG_QUERY, Some(500), true, FallbackPolicy::default()),
            None
        );
    }

    #[test]
    fn empty_result_set_is_reported_as_no_candidate() {
        assert_eq!(
            evaluate_fallback(LONG_QUERY, None, true, FallbackPolicy::default()),
            Some(FallbackTrigger::NoCandidate)
        );
    }

    #[test]
    fn weak_top_score_is_reported_as_low_confidence() {
        assert_eq!(
            evaluate_fallback(
                LONG_QUERY,
                Some(FALLBACK_MIN_CONFIDENT_SCORE - 1),
                true,
                FallbackPolicy::default(),
            ),
            Some(FallbackTrigger::LowConfidence)
        );
    }

    #[test]
    fn confident_hit_without_an_interface_is_reported_separately() {
        assert_eq!(
            evaluate_fallback(LONG_QUERY, Some(200), false, FallbackPolicy::default()),
            Some(FallbackTrigger::NoExecutableInterface),
            "a documentation-only skill is not a low-confidence match"
        );
    }

    #[test]
    fn low_confidence_wins_over_missing_interface() {
        // Only one reason code is reported; the weakest, most generic signal
        // is the one the caller can act on, so it takes precedence.
        assert_eq!(
            evaluate_fallback(LONG_QUERY, Some(1), false, FallbackPolicy::default()),
            Some(FallbackTrigger::LowConfidence)
        );
    }

    #[test]
    fn discovery_requests_never_fall_back() {
        assert_eq!(
            evaluate_fallback(0, None, true, FallbackPolicy::default()),
            None,
            "an empty query is a discovery request, not an unanswered one"
        );
        assert_eq!(
            evaluate_fallback(
                FALLBACK_MIN_QUERY_LEN - 1,
                None,
                true,
                FallbackPolicy::default(),
            ),
            None
        );
        assert_eq!(
            evaluate_fallback(
                FALLBACK_MIN_QUERY_LEN,
                None,
                true,
                FallbackPolicy::default()
            ),
            Some(FallbackTrigger::NoCandidate)
        );
    }

    #[test]
    fn disabled_policy_never_fires() {
        let policy = FallbackPolicy {
            enabled: false,
            ..FallbackPolicy::default()
        };
        assert_eq!(evaluate_fallback(LONG_QUERY, None, false, policy), None);
        assert_eq!(evaluate_fallback(LONG_QUERY, Some(0), false, policy), None);
    }

    #[test]
    fn thresholds_are_configurable_from_one_place() {
        let strict = FallbackPolicy {
            min_confident_score: 400,
            ..FallbackPolicy::default()
        };
        assert_eq!(
            evaluate_fallback(LONG_QUERY, Some(399), true, strict),
            Some(FallbackTrigger::LowConfidence),
            "raising the threshold tightens the route without touching call sites"
        );
    }

    #[test]
    fn reason_codes_distinguish_the_two_headline_cases() {
        assert_eq!(
            FallbackTrigger::LowConfidence.reason_code(),
            FALLBACK_REASON_LOW_CONFIDENCE
        );
        assert_eq!(
            FallbackTrigger::NoExecutableInterface.reason_code(),
            FALLBACK_REASON_NO_EXECUTABLE_INTERFACE
        );
        assert_ne!(
            FallbackTrigger::LowConfidence.reason_code(),
            FallbackTrigger::NoExecutableInterface.reason_code()
        );
    }

    #[test]
    fn fallback_policy_defaults_come_from_the_module_constants() {
        let policy = FallbackPolicy::default();
        assert_eq!(policy.min_confident_score, FALLBACK_MIN_CONFIDENT_SCORE);
        assert_eq!(policy.min_query_len, FALLBACK_MIN_QUERY_LEN);
        assert!(policy.enabled);
    }

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
