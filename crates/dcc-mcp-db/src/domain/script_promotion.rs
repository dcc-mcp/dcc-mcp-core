//! Durable repeat-counter contract for escape-hatch scripts (dcc-mcp-core#2297-A3).
//!
//! Every materialised escape-hatch script carries a redaction-safe `sha256`
//! (see `ScriptExecutionTelemetry` in `dcc-mcp-gateway-admin`). Counting those
//! hashes by scanning the audit log is O(audits) per query and loses history
//! on restart, so the count is persisted instead: one row per
//! `(sha256, dcc_type, tool_name)`, bumped by an idempotent upsert, promoted
//! to `proposed` once it reaches [`ScriptPromotionPolicy::min_repeats`].

use serde::{Deserialize, Serialize};

/// Number of repeats before a script becomes a skill-promotion candidate.
///
/// Aligned with the `min_repeats = 3` threshold used by the A1
/// `SkillPromotionProposal` payload so both steps agree on the default.
pub const DEFAULT_SCRIPT_PROMOTION_MIN_REPEATS: u32 = 3;

/// Lower bound for [`ScriptPromotionPolicy::min_repeats`]. A value of `0`
/// would promote every script on its first execution.
const MIN_REPEATS_FLOOR: u32 = 1;

/// Placeholder DCC type used when a script execution arrives without a
/// resolved backend (`AdminAuditRecord::dcc_type == None`).
pub const UNKNOWN_DCC_TYPE: &str = "unknown";

/// Workflow state of one persisted counter row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptPromotionProposalState {
    /// Observed, but below the promotion threshold.
    #[default]
    Pending,
    /// Reached the threshold; a promotion proposal may be emitted.
    Proposed,
    /// An operator rejected the proposal; never auto-promoted again.
    Dismissed,
}

impl ScriptPromotionProposalState {
    /// Wire format persisted in `script_promotion_counters.proposal_state`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Proposed => "proposed",
            Self::Dismissed => "dismissed",
        }
    }

    /// Parse the persisted wire format, falling back to [`Self::Pending`] for
    /// unknown values so a newer writer never breaks an older reader.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw {
            "proposed" => Self::Proposed,
            "dismissed" => Self::Dismissed,
            _ => Self::Pending,
        }
    }

    /// `true` when the row has crossed the promotion threshold.
    #[must_use]
    pub const fn is_proposed(self) -> bool {
        matches!(self, Self::Proposed)
    }

    /// `true` when the state must not be recomputed by a bump.
    ///
    /// `proposed` and `dismissed` are sticky: repeat observations never
    /// demote a row, only an explicit operator action can.
    #[must_use]
    pub const fn is_sticky(self) -> bool {
        matches!(self, Self::Proposed | Self::Dismissed)
    }
}

impl std::fmt::Display for ScriptPromotionProposalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One persisted repeat counter row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptPromotionCounter {
    /// Redaction-safe hash of the materialised script body.
    pub sha256: String,
    /// DCC the script ran in (`"maya"`, …) or [`UNKNOWN_DCC_TYPE`].
    pub dcc_type: String,
    /// Tool slug that produced the execution.
    pub tool_name: String,
    /// Number of observations recorded for this key.
    pub count: u64,
    /// Epoch milliseconds of the first observation.
    pub first_seen_ms: u64,
    /// Epoch milliseconds of the most recent observation.
    pub last_seen_ms: u64,
    /// Promotion workflow state derived from `count` and the policy.
    pub proposal_state: ScriptPromotionProposalState,
}

impl ScriptPromotionCounter {
    /// `true` when this counter should be offered for skill promotion.
    #[must_use]
    pub const fn is_candidate(&self) -> bool {
        self.proposal_state.is_proposed()
    }
}

/// Bump request handed to the admin SQLite writer thread.
///
/// Serialised end-to-end so the hot path stays non-blocking: the audit sink
/// only has to build this value and hand it to the lane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptPromotionBumpJson {
    /// Redaction-safe hash of the materialised script body.
    pub sha256: String,
    /// DCC the script ran in, or [`UNKNOWN_DCC_TYPE`].
    pub dcc_type: String,
    /// Tool slug that produced the execution.
    pub tool_name: String,
    /// Epoch milliseconds the execution was observed.
    pub observed_at_ms: u64,
    /// Per-call threshold override; defaults to the policy default when absent.
    #[serde(default)]
    pub min_repeats: Option<u32>,
}

impl ScriptPromotionBumpJson {
    /// Resolve the threshold for this bump.
    #[must_use]
    pub fn policy(&self) -> ScriptPromotionPolicy {
        match self.min_repeats {
            Some(min_repeats) => ScriptPromotionPolicy::new(min_repeats),
            None => ScriptPromotionPolicy::default(),
        }
    }
}

/// Repeat threshold that promotes a repeated script to a candidate.
///
/// Configurable per deployment through
/// `DCC_MCP_SCRIPT_PROMOTION_MIN_REPEATS` ([`ScriptPromotionPolicy::from_env`])
/// or through the gateway `AdminPersistConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptPromotionPolicy {
    /// Observations required before `proposal_state` becomes `proposed`.
    pub min_repeats: u32,
}

impl ScriptPromotionPolicy {
    /// Build a policy, clamping to at least one repeat.
    #[must_use]
    pub const fn new(min_repeats: u32) -> Self {
        let min_repeats = if min_repeats < MIN_REPEATS_FLOOR {
            MIN_REPEATS_FLOOR
        } else {
            min_repeats
        };
        Self { min_repeats }
    }

    /// Build a policy from `DCC_MCP_SCRIPT_PROMOTION_MIN_REPEATS`.
    ///
    /// Unset, empty, or unparsable values fall back to the default so a bad
    /// environment can never disable promotion silently.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var(crate::domain::env::ENV_SCRIPT_PROMOTION_MIN_REPEATS)
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok())
        {
            Some(min_repeats) => Self::new(min_repeats),
            None => Self::default(),
        }
    }

    /// Resolve the effective threshold: an explicit configuration value wins,
    /// then `DCC_MCP_SCRIPT_PROMOTION_MIN_REPEATS`, then the default.
    ///
    /// Call this once at wiring time — it reads the environment, so it does
    /// not belong on the per-call hot path.
    #[must_use]
    pub fn resolve(configured: Option<u32>) -> Self {
        match configured {
            Some(min_repeats) => Self::new(min_repeats),
            None => Self::from_env(),
        }
    }

    /// State a counter with `count` observations should carry.
    #[must_use]
    pub const fn state_for(self, count: u64) -> ScriptPromotionProposalState {
        if count >= self.min_repeats as u64 {
            ScriptPromotionProposalState::Proposed
        } else {
            ScriptPromotionProposalState::Pending
        }
    }

    /// `true` when `count` observations satisfy the threshold.
    #[must_use]
    pub const fn is_candidate(self, count: u64) -> bool {
        count >= self.min_repeats as u64
    }
}

impl Default for ScriptPromotionPolicy {
    fn default() -> Self {
        Self::new(DEFAULT_SCRIPT_PROMOTION_MIN_REPEATS)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_SCRIPT_PROMOTION_MIN_REPEATS, ScriptPromotionBumpJson, ScriptPromotionPolicy,
        ScriptPromotionProposalState,
    };

    #[test]
    fn default_policy_matches_a1_min_repeats() {
        let policy = ScriptPromotionPolicy::default();
        assert_eq!(policy.min_repeats, 3);
        assert_eq!(policy.min_repeats, DEFAULT_SCRIPT_PROMOTION_MIN_REPEATS);
    }

    #[test]
    fn policy_clamps_zero_to_one() {
        assert_eq!(ScriptPromotionPolicy::new(0).min_repeats, 1);
        assert_eq!(ScriptPromotionPolicy::new(7).min_repeats, 7);
    }

    #[test]
    fn state_promotes_at_threshold() {
        let policy = ScriptPromotionPolicy::new(3);
        assert_eq!(policy.state_for(1), ScriptPromotionProposalState::Pending);
        assert_eq!(policy.state_for(2), ScriptPromotionProposalState::Pending);
        assert_eq!(policy.state_for(3), ScriptPromotionProposalState::Proposed);
        assert_eq!(policy.state_for(9), ScriptPromotionProposalState::Proposed);
    }

    #[test]
    fn proposal_state_roundtrips_through_wire_format() {
        for state in [
            ScriptPromotionProposalState::Pending,
            ScriptPromotionProposalState::Proposed,
            ScriptPromotionProposalState::Dismissed,
        ] {
            assert_eq!(ScriptPromotionProposalState::parse(state.as_str()), state);
        }
        assert_eq!(
            ScriptPromotionProposalState::parse("something-new"),
            ScriptPromotionProposalState::Pending
        );
    }

    #[test]
    fn sticky_states_are_proposed_and_dismissed() {
        assert!(ScriptPromotionProposalState::Proposed.is_sticky());
        assert!(ScriptPromotionProposalState::Dismissed.is_sticky());
        assert!(!ScriptPromotionProposalState::Pending.is_sticky());
    }

    #[test]
    fn resolve_prefers_explicit_config() {
        assert_eq!(ScriptPromotionPolicy::resolve(Some(6)).min_repeats, 6);
    }

    #[test]
    fn resolve_without_config_uses_env_or_default() {
        let resolved = ScriptPromotionPolicy::resolve(None);
        assert_eq!(resolved, ScriptPromotionPolicy::from_env());
        assert!(resolved.min_repeats >= 1);
        // The suite never sets the override, so this is the documented default.
        if std::env::var_os(crate::domain::env::ENV_SCRIPT_PROMOTION_MIN_REPEATS).is_none() {
            assert_eq!(resolved.min_repeats, DEFAULT_SCRIPT_PROMOTION_MIN_REPEATS);
        }
    }

    #[test]
    fn bump_json_defaults_to_the_policy_threshold() {
        let bump = ScriptPromotionBumpJson {
            sha256: "a".repeat(64),
            dcc_type: "maya".into(),
            tool_name: "execute_python".into(),
            observed_at_ms: 1,
            min_repeats: None,
        };
        assert_eq!(bump.policy(), ScriptPromotionPolicy::default());
    }
}
