//! Shared ranking policy applied by every search surface.
//!
//! Two independent concerns live here:
//!
//! 1. **Rank dampening** ([`apply_rank_policy`]) — coefficients applied to
//!    every row during scoring.
//! 2. **CUA fallback routing** ([`evaluate_fallback`]) — an explicit,
//!    post-ranking routing rule for requests retrieval cannot answer.

use serde::{Deserialize, Serialize};

use crate::query::SearchHit;
use crate::record::SearchRecord;

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

// ── CUA fallback routing ────────────────────────────────────────────────────
//
// Retrieval can legitimately have no answer: the target host may expose only a
// GUI with nothing scriptable, or nothing may match the request at all. The old
// contract returned a bare empty array, which left the caller with no next
// step. These rules turn that case into an explicit route to the project-owned
// `dcc-cua` skill.
//
// This is a **routing rule, not a ranking tweak**. `dcc-cua` carries
// `layer: infrastructure` and is deliberately damped to
// `LAYER_MULT_INFRASTRUCTURE` (0.35) in neutral discovery so it never pollutes
// ordinary results. Raising that coefficient until it wins would contradict
// that design, so the fallback is decided here, after ranking, and never
// touches scoring.

/// Skill that owns the project UI-control route.
///
/// A fallback always names this skill and nothing else. `skills/dcc-cua/SKILL.md`
/// defines a non-substitution contract: a DCC-CUA route must never be silently
/// swapped for generic Codex/OpenAI Computer Use, the `computer-use` Skill,
/// `@oai/sky`, or a browser automation plugin. When the route is broken it is
/// repaired or reported as a blocker — the provider is never changed.
pub const FALLBACK_TARGET_SKILL: &str = "dcc-cua";

/// Low-confidence gate: a top-1 score **strictly below** this value is not a
/// usable answer.
///
/// Measured against the fuzzy scorer in [`crate::ranking::FuzzyScorer`] through
/// `SkillCatalog::search_skills_with_fallback` (see
/// `crates/dcc-mcp-skills/src/catalog/tests/test_search_fallback.rs`):
///
/// | Query shape | Measured top-1 score |
/// |---|---|
/// | Query tokens match the skill and tool name (`maya shot export`) | 104 |
/// | Full multi-token name match (`photoshop gui workflow`) | 83 |
/// | Name tokens, partial (`bake textures`) | 77 |
/// | Single-token name prefix/substring (`export`, `maya`, `photoshop`) | 28–38 |
/// | Description-only noise (`render the farm queue`) | 5 |
/// | Nothing matched | 0 |
///
/// The gate sits in the empty band between 5 and 28, so a genuine name match
/// never trips it while description noise always does. Re-measure before
/// changing it: the value is overridable per call through
/// [`FallbackPolicy::min_top1_score`], and it is tuned for the fuzzy scorer —
/// callers running [`crate::SearchMode::Exact`] should pass a lower gate, since
/// the legacy substring table tops out at 23.
pub const FALLBACK_MIN_TOP1_SCORE: u32 = 12;

/// How many of the top hits are inspected before declaring that nothing
/// callable was found.
pub const FALLBACK_INTERFACE_SCAN: usize = 3;

/// Read-only preflight for the project route, in order.
///
/// These come from the official component contract in `skills/dcc-cua/SKILL.md`.
/// Never replace them with a download of an arbitrary executable.
pub const FALLBACK_PREFLIGHT: [&str; 3] = [
    "dcc-mcp-cli components status dcc-cua",
    "dcc-cua manifest",
    "dcc-cua ping",
];

/// Repair command for a broken route. Installation and repair mutate the host,
/// so this is only advertised when the route is already known to be unusable
/// and the caller has authorized the repair.
pub const FALLBACK_REPAIR_COMMAND: &str = "dcc-mcp-cli components ensure dcc-cua --yes";

/// Why retrieval had no answer. Serialized as a stable reason code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    /// Nothing matched, or the best hit scored below the confidence gate.
    LowConfidence,
    /// The best hits scored well but declare no callable interface
    /// (empty `tools.yaml` / no tool declaration), so there is nothing to invoke.
    NoExecutableInterface,
}

impl FallbackReason {
    /// Stable, low-cardinality reason code for telemetry and caller branching.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LowConfidence => "low_confidence",
            Self::NoExecutableInterface => "no_executable_interface",
        }
    }
}

/// Whether the project route can take the task right now. Serialized as a
/// stable status code — this is the code that distinguishes "route to
/// `dcc-cua`" from "`dcc-cua` is broken, repair it".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackStatus {
    /// The route was probed and is usable.
    Ready,
    /// No probe ran. The route is named but must be verified before use.
    Unverified,
    /// The route was probed and is unusable. The task is blocked on repair,
    /// **not** on switching to a generic computer-use provider.
    Blocked,
}

impl FallbackStatus {
    /// Stable status code.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Unverified => "unverified",
            Self::Blocked => "blocked",
        }
    }
}

/// Availability of the project-owned CUA route, as determined by the caller's
/// preflight.
///
/// The search crate performs no I/O, so availability is an **input**. It
/// defaults to [`Self::Unprobed`]: an unverified route is never reported as
/// ready.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CuaRouteStatus {
    /// A probe proved the route is installed and responsive.
    Ready,
    /// A probe ran and the route is unusable. `detail` is the exact blocker.
    Unavailable { detail: String },
    /// No probe ran.
    #[default]
    Unprobed,
}

/// Tunable gate for [`evaluate_fallback`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallbackPolicy {
    /// `false` disables fallback routing entirely and restores the legacy
    /// bare-empty-result behaviour.
    pub enabled: bool,
    /// Top-1 scores strictly below this count as low confidence.
    pub min_top1_score: u32,
    /// How many top hits are inspected for a callable interface.
    pub interface_scan: usize,
}

impl Default for FallbackPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            min_top1_score: FALLBACK_MIN_TOP1_SCORE,
            interface_scan: FALLBACK_INTERFACE_SCAN,
        }
    }
}

/// Routing advice returned when retrieval has no executable answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchFallback {
    /// Always [`FALLBACK_TARGET_SKILL`]. Never a generic computer-use provider.
    pub target: String,
    /// Why retrieval had no answer.
    pub reason: FallbackReason,
    /// Whether the route can take the task right now.
    pub status: FallbackStatus,
    /// Best score actually achieved — the caller's evidence for the reason.
    pub top_score: u32,
    /// One-line explanation the caller can relay to the user verbatim.
    pub message: String,
    /// Read-only preflight commands, in order.
    pub preflight: Vec<String>,
    /// Authorized repair step. Present only when [`FallbackStatus::Blocked`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair: Option<String>,
    /// Exact blocker. Present only when [`FallbackStatus::Blocked`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
}

/// Decide whether a ranked result set should be routed to the project-owned
/// CUA route.
///
/// Returns `None` when retrieval produced a usable answer, so the fallback
/// stays silent on the happy path.
///
/// The predicate is deliberately a single function: the gate must be constant,
/// centralized, and testable, not a set of `if`s scattered across call sites.
#[must_use]
pub fn evaluate_fallback<R: SearchRecord>(
    hits: &[SearchHit<R>],
    route: &CuaRouteStatus,
    policy: FallbackPolicy,
) -> Option<SearchFallback> {
    let reason = fallback_reason(hits, policy)?;
    let top_score = hits.first().map_or(0, |hit| hit.score);
    Some(build_fallback(reason, top_score, route))
}

/// Why `hits` have no executable answer, ignoring route availability.
///
/// Split out from [`evaluate_fallback`] so callers that must do I/O to learn
/// the route status can run this cheap predicate first and skip the probe on
/// the happy path.
#[must_use]
pub fn fallback_reason<R: SearchRecord>(
    hits: &[SearchHit<R>],
    policy: FallbackPolicy,
) -> Option<FallbackReason> {
    if !policy.enabled {
        return None;
    }

    // Routing advice is only useful when the caller is not already looking at
    // the route. `dcc-cua` declares no callable interface of its own, so a
    // request that already surfaced it would otherwise be told to route to
    // itself.
    if hits
        .first()
        .is_some_and(|hit| is_fallback_target(&hit.record))
    {
        return None;
    }

    let top_score = hits.first().map_or(0, |hit| hit.score);
    if top_score < policy.min_top1_score {
        return Some(FallbackReason::LowConfidence);
    }

    let any_callable = hits
        .iter()
        .take(policy.interface_scan)
        .any(|hit| hit.record.has_executable_interface());

    (!any_callable).then_some(FallbackReason::NoExecutableInterface)
}

/// Whether `record` *is* the project route rather than something to route to.
fn is_fallback_target<R: SearchRecord>(record: &R) -> bool {
    let name = record.skill_name().unwrap_or_else(|| record.tool_slug());
    name.trim().eq_ignore_ascii_case(FALLBACK_TARGET_SKILL)
}

fn build_fallback(
    reason: FallbackReason,
    top_score: u32,
    route: &CuaRouteStatus,
) -> SearchFallback {
    let status = match route {
        CuaRouteStatus::Ready => FallbackStatus::Ready,
        CuaRouteStatus::Unprobed => FallbackStatus::Unverified,
        CuaRouteStatus::Unavailable { .. } => FallbackStatus::Blocked,
    };

    let mut message = match reason {
        FallbackReason::LowConfidence => format!(
            "No skill scored at or above {FALLBACK_MIN_TOP1_SCORE} for this request (best score: \
             {top_score}). Route the task to the project-owned '{FALLBACK_TARGET_SKILL}' UI-control \
             route instead of browsing unrelated skills."
        ),
        FallbackReason::NoExecutableInterface => format!(
            "The best matches scored {top_score} but declare no callable interface, so there is \
             nothing to invoke. Route the task to the project-owned '{FALLBACK_TARGET_SKILL}' \
             UI-control route."
        ),
    };

    match route {
        CuaRouteStatus::Ready => {
            message.push_str(&format!(
                " Confirm the route with `{}` before handing work over.",
                FALLBACK_PREFLIGHT[0]
            ));
        }
        CuaRouteStatus::Unprobed => {
            message.push_str(&format!(
                " Verify the route before handing work over: run `{}`.",
                FALLBACK_PREFLIGHT.join("`, `")
            ));
        }
        CuaRouteStatus::Unavailable { detail } => {
            message.push_str(&format!(
                " The route is unavailable ({detail}); repair it with `{FALLBACK_REPAIR_COMMAND}` \
                 or report the blocker and stop — the project route is the only route."
            ));
        }
    }

    SearchFallback {
        target: FALLBACK_TARGET_SKILL.to_string(),
        reason,
        status,
        top_score,
        message,
        preflight: FALLBACK_PREFLIGHT
            .iter()
            .map(|c| (*c).to_string())
            .collect(),
        repair: matches!(status, FallbackStatus::Blocked)
            .then(|| FALLBACK_REPAIR_COMMAND.to_string()),
        blocker: match route {
            CuaRouteStatus::Unavailable { detail } => Some(detail.clone()),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[derive(Clone)]
    struct Row {
        slug: &'static str,
        summary: &'static str,
        callable: bool,
    }

    fn row(slug: &'static str, callable: bool) -> Row {
        Row {
            slug,
            summary: "export the current shot",
            callable,
        }
    }

    impl SearchRecord for Row {
        fn tool_slug(&self) -> &str {
            self.slug
        }
        fn backend_tool(&self) -> &str {
            self.slug
        }
        fn summary(&self) -> &str {
            self.summary
        }
        fn skill_name(&self) -> Option<&str> {
            Some(self.slug)
        }
        fn tags(&self) -> &[String] {
            &[]
        }
        fn dcc_type(&self) -> &str {
            "maya"
        }
        fn instance_id(&self) -> Uuid {
            Uuid::nil()
        }
        fn loaded(&self) -> bool {
            true
        }
        fn has_executable_interface(&self) -> bool {
            self.callable
        }
    }

    /// Build a ranked hit list. `entries` is `(score, callable)` per hit.
    fn hits(entries: &[(u32, bool)]) -> Vec<SearchHit<Row>> {
        entries
            .iter()
            .enumerate()
            .map(|(idx, (score, callable))| SearchHit {
                record: Row {
                    slug: "shot-export",
                    summary: "export the current shot",
                    callable: *callable,
                },
                rank: idx as u32 + 1,
                score: *score,
                match_reasons: Vec::new(),
            })
            .collect()
    }

    const READY: CuaRouteStatus = CuaRouteStatus::Ready;

    // ── Requirement: a confident, callable hit never triggers the fallback ──

    #[test]
    fn confident_callable_hit_does_not_trigger_fallback() {
        let hits = hits(&[(48, true), (20, true)]);
        assert_eq!(
            evaluate_fallback(&hits, &READY, FallbackPolicy::default()),
            None
        );
    }

    #[test]
    fn confidence_gate_boundary_is_exclusive() {
        let at_gate = hits(&[(FALLBACK_MIN_TOP1_SCORE, true)]);
        assert_eq!(
            evaluate_fallback(&at_gate, &READY, FallbackPolicy::default()),
            None
        );

        let below_gate = hits(&[(FALLBACK_MIN_TOP1_SCORE - 1, true)]);
        let fallback =
            evaluate_fallback(&below_gate, &READY, FallbackPolicy::default()).expect("fallback");
        assert_eq!(fallback.reason, FallbackReason::LowConfidence);
        assert_eq!(fallback.top_score, FALLBACK_MIN_TOP1_SCORE - 1);
    }

    #[test]
    fn disabled_policy_restores_the_legacy_empty_result_behaviour() {
        let hits = hits(&[]);
        assert_eq!(
            evaluate_fallback(
                &hits,
                &READY,
                FallbackPolicy {
                    enabled: false,
                    ..FallbackPolicy::default()
                }
            ),
            None
        );
    }

    // ── Requirement: an empty or low-confidence retrieval triggers the
    //    fallback with the matching reason code ──

    #[test]
    fn empty_retrieval_triggers_low_confidence() {
        let fallback =
            evaluate_fallback(&hits(&[]), &READY, FallbackPolicy::default()).expect("fallback");
        assert_eq!(fallback.reason, FallbackReason::LowConfidence);
        assert_eq!(fallback.reason.as_str(), "low_confidence");
        assert_eq!(fallback.top_score, 0);
    }

    #[test]
    fn description_only_noise_triggers_low_confidence() {
        // 4 = summary_fuzzy territory, well under the gate.
        let fallback = evaluate_fallback(&hits(&[(4, true)]), &READY, FallbackPolicy::default())
            .expect("fallback");
        assert_eq!(fallback.reason, FallbackReason::LowConfidence);
    }

    // ── Requirement: hits that declare no callable interface trigger the
    //    fallback with a *different* reason code ──

    #[test]
    fn non_callable_hits_trigger_no_executable_interface() {
        let fallback = evaluate_fallback(
            &hits(&[(60, false), (55, false)]),
            &READY,
            FallbackPolicy::default(),
        )
        .expect("fallback");
        assert_eq!(fallback.reason, FallbackReason::NoExecutableInterface);
        assert_eq!(fallback.reason.as_str(), "no_executable_interface");
        assert_eq!(fallback.top_score, 60);
        // Distinct from the low-confidence code.
        assert_ne!(
            fallback.reason.as_str(),
            FallbackReason::LowConfidence.as_str()
        );
    }

    #[test]
    fn interface_scan_stops_at_the_configured_depth() {
        // A callable hit inside the scan window means retrieval has an answer.
        let within_scan = hits(&[(60, false), (55, false), (50, true)]);
        assert_eq!(
            evaluate_fallback(&within_scan, &READY, FallbackPolicy::default()),
            None
        );

        // A callable hit beyond the scan window does not rescue the result.
        let beyond_scan = hits(&[(60, false), (55, false), (50, false), (45, true)]);
        let fallback =
            evaluate_fallback(&beyond_scan, &READY, FallbackPolicy::default()).expect("fallback");
        assert_eq!(fallback.reason, FallbackReason::NoExecutableInterface);

        // Widening the scan is a policy change, not a new code path.
        let widened = FallbackPolicy {
            interface_scan: 4,
            ..FallbackPolicy::default()
        };
        assert_eq!(evaluate_fallback(&beyond_scan, &READY, widened), None);
    }

    // ── Requirement: an unavailable route is a distinguishable blocker, never
    //    a provider swap and never a silent empty result ──

    #[test]
    fn unavailable_route_is_reported_as_blocked() {
        let unavailable = CuaRouteStatus::Unavailable {
            detail: "dcc-cua host binary not found on PATH".to_string(),
        };
        let fallback = evaluate_fallback(&hits(&[]), &unavailable, FallbackPolicy::default())
            .expect("fallback");

        assert_eq!(fallback.status, FallbackStatus::Blocked);
        assert_eq!(fallback.status.as_str(), "blocked");
        assert_eq!(
            fallback.blocker.as_deref(),
            Some("dcc-cua host binary not found on PATH")
        );
        assert_eq!(fallback.repair.as_deref(), Some(FALLBACK_REPAIR_COMMAND));

        // The blocker is distinguishable from the two non-blocked statuses.
        assert_ne!(fallback.status.as_str(), FallbackStatus::Ready.as_str());
        assert_ne!(
            fallback.status.as_str(),
            FallbackStatus::Unverified.as_str()
        );
    }

    #[test]
    fn unprobed_route_is_unverified_and_carries_no_blocker() {
        let fallback = evaluate_fallback(
            &hits(&[]),
            &CuaRouteStatus::Unprobed,
            FallbackPolicy::default(),
        )
        .expect("fallback");
        assert_eq!(fallback.status, FallbackStatus::Unverified);
        assert_eq!(fallback.blocker, None);
        assert_eq!(fallback.repair, None);
    }

    #[test]
    fn ready_route_advertises_read_only_preflight_only() {
        let fallback =
            evaluate_fallback(&hits(&[]), &READY, FallbackPolicy::default()).expect("fallback");
        assert_eq!(fallback.status, FallbackStatus::Ready);
        assert_eq!(fallback.preflight, FALLBACK_PREFLIGHT);
        // `components ensure` mutates the host: never advertised when usable.
        assert_eq!(fallback.repair, None);
    }

    // ── Requirement: non-substitution contract ──

    #[test]
    fn fallback_always_names_the_project_route_and_never_a_generic_provider() {
        for route in [
            READY,
            CuaRouteStatus::Unprobed,
            CuaRouteStatus::Unavailable {
                detail: "probe failed".to_string(),
            },
        ] {
            for reason_hits in [hits(&[]), hits(&[(60, false)])] {
                let fallback = evaluate_fallback(&reason_hits, &route, FallbackPolicy::default())
                    .expect("fallback");
                assert_eq!(fallback.target, FALLBACK_TARGET_SKILL);
                assert_eq!(fallback.target, "dcc-cua");
                assert!(fallback.message.contains("dcc-cua"));
                for banned in [
                    "computer-use",
                    "@oai/sky",
                    "openai",
                    "codex computer use",
                    "chrome plugin",
                ] {
                    assert!(
                        !fallback.message.to_ascii_lowercase().contains(banned),
                        "fallback message must not advertise a generic provider: {banned}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_request_already_served_by_the_route_is_not_told_to_route_to_itself() {
        // `dcc-cua` declares no callable interface, so without this guard an
        // exact-name lookup for it would emit "route to dcc-cua".
        let mut named = hits(&[(60, false)]);
        named[0].record = row(FALLBACK_TARGET_SKILL, false);
        assert_eq!(
            evaluate_fallback(&named, &READY, FallbackPolicy::default()),
            None
        );

        // Case-insensitive: the name comes from user-authored SKILL.md.
        let mut named = hits(&[(60, false)]);
        named[0].record = row("DCC-CUA", false);
        assert_eq!(
            evaluate_fallback(&named, &READY, FallbackPolicy::default()),
            None
        );
    }

    #[test]
    fn the_route_below_the_top_hit_is_still_valid_routing_advice() {
        // Top hit is non-callable, the route is a lower-ranked hit: advising
        // the caller to use it is useful, not circular.
        let mut ranked = hits(&[(60, false), (40, false)]);
        ranked[1].record = row(FALLBACK_TARGET_SKILL, false);
        let fallback =
            evaluate_fallback(&ranked, &READY, FallbackPolicy::default()).expect("fallback");
        assert_eq!(fallback.target, FALLBACK_TARGET_SKILL);
    }

    #[test]
    fn blocked_message_tells_the_caller_to_repair_not_switch() {
        let unavailable = CuaRouteStatus::Unavailable {
            detail: "manifest SHA-256 mismatch".to_string(),
        };
        let fallback = evaluate_fallback(&hits(&[]), &unavailable, FallbackPolicy::default())
            .expect("fallback");
        assert!(fallback.message.contains("manifest SHA-256 mismatch"));
        assert!(fallback.message.contains("report the blocker and stop"));
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
