//! Explicit `dcc-cua` fallback contract (PIP-3702).
//!
//! Some tasks have no scriptable interface: the target host only exposes a
//! GUI, or the best-matching skill is documentation-only. Ordering cannot
//! solve that — `dcc-cua` carries `layer: infrastructure`, which the rank
//! policy deliberately demotes to `0.35` so it stays out of neutral
//! discovery. Raising that coefficient is the wrong fix; it would push the
//! fallback into ordinary result pages.
//!
//! The route is therefore explicit, and it has three moving parts:
//!
//! 1. **The judgement** — [`crate::policy::evaluate_fallback`] decides that a
//!    result set cannot serve the request, from thresholds that live in
//!    [`crate::policy`] and nowhere else.
//! 2. **The probe** — [`CuaRuntimeProbe`] reports whether the project-owned
//!    `dcc-cua` runtime can actually be used right now.
//! 3. **The advice** — [`SearchFallback`] is what the caller puts on the wire:
//!    a reason code, a runtime state, and the official component commands.
//!
//! # Non-substitution contract
//!
//! `skills/dcc-cua/SKILL.md` already binds *explicit* `dcc-cua` requests to
//! the project-owned runtime. This module reuses that contract for the
//! *automatic* case instead of writing a second one. Concretely:
//!
//! * [`SearchFallback::skill`] is always [`crate::policy::FALLBACK_SKILL`] —
//!   `dcc-cua`, never anything else.
//! * When the runtime is not usable the advice is **blocked**, not re-routed.
//!   There is no code path here that yields a different provider.
//! * Never load or call generic Codex/OpenAI Computer Use, the `computer-use`
//!   Skill, `@oai/sky`, or a Browser/Chrome plugin as a substitute.

use serde::{Deserialize, Serialize};

use crate::policy::{FALLBACK_REASON_CUA_UNAVAILABLE, FALLBACK_SKILL, FallbackTrigger};

/// Providers that must never be substituted for `dcc-cua`.
///
/// Kept as data so the boundary is greppable and testable rather than living
/// only in prose. Nothing in this crate selects a provider; the list exists so
/// a test can assert the contract still holds.
pub const FORBIDDEN_SUBSTITUTES: &[&str] = &[
    "codex-computer-use",
    "openai-computer-use",
    "computer-use",
    "@oai/sky",
    "browser-plugin",
];

/// `dcc-mcp-cli components status dcc-cua` — read-only, always safe to run.
pub const PREFLIGHT_STATUS_CMD: &str = "dcc-mcp-cli components status dcc-cua";
/// `dcc-mcp-cli components ensure dcc-cua --yes` — mutates the filesystem.
///
/// Run only when installation or repair is authorized; it is published here as
/// advice for the caller, never executed by this crate.
pub const PREFLIGHT_ENSURE_CMD: &str = "dcc-mcp-cli components ensure dcc-cua --yes";
/// `dcc-cua manifest` — the runtime publishes its own capabilities.
pub const PREFLIGHT_MANIFEST_CMD: &str = "dcc-cua manifest";
/// `dcc-cua ping` — liveness check against the installed runtime.
pub const PREFLIGHT_PING_CMD: &str = "dcc-cua ping";

/// Official component-contract commands for inspecting and repairing the route.
#[must_use]
pub fn preflight_commands() -> Vec<String> {
    vec![
        PREFLIGHT_STATUS_CMD.to_string(),
        PREFLIGHT_ENSURE_CMD.to_string(),
        PREFLIGHT_MANIFEST_CMD.to_string(),
        PREFLIGHT_PING_CMD.to_string(),
    ]
}

/// Availability of the project-owned `dcc-cua` runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuaRuntimeState {
    /// `components status` reports `ready` and the binary answers `ping`.
    Ready,
    /// Installed and version-compatible, but not answering. `components ensure`
    /// may repair it.
    NotResponding,
    /// The binary is present but liveness was not checked — no probe ran.
    ///
    /// The route is still the answer: it is named and the caller is told to
    /// run the official preflight before use. This exists so a search handler
    /// can offer the route without spawning a subprocess per request.
    Unverified,
    /// Not installed at all. Installing requires explicit authorization.
    Missing,
    /// Installed but rejected by `components status` (for example a target or
    /// checksum mismatch).
    Incompatible,
    /// No probe was wired up, or the probe itself could not run.
    Unknown,
}

impl CuaRuntimeState {
    /// Whether the route is confirmed usable right now.
    #[must_use]
    pub fn is_usable(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Whether the advice is still worth giving, as opposed to a blocker.
    ///
    /// `Unverified` is actionable: the route is named and correct, the caller
    /// just has to run the official preflight first. Everything else that is
    /// not `Ready` is a blocker that must be repaired or reported.
    #[must_use]
    pub fn is_actionable(self) -> bool {
        matches!(self, Self::Ready | Self::Unverified)
    }

    /// One-line explanation used when the route is blocked.
    #[must_use]
    pub fn blocker(self) -> &'static str {
        match self {
            Self::Ready => "dcc-cua is ready",
            Self::Unverified => {
                "dcc-cua is present but liveness was not checked; run `dcc-mcp-cli components status dcc-cua` and `dcc-cua ping` before use"
            }
            Self::NotResponding => {
                "dcc-cua is installed but not responding; run `dcc-mcp-cli components ensure dcc-cua --yes` when repair is authorized"
            }
            Self::Missing => {
                "dcc-cua is not installed; install it with `dcc-mcp-cli components ensure dcc-cua --yes` when authorized"
            }
            Self::Incompatible => {
                "the installed dcc-cua is incompatible with this target; reinstall it from the official manifest"
            }
            Self::Unknown => {
                "the dcc-cua runtime could not be probed; run `dcc-mcp-cli components status dcc-cua` to inspect it"
            }
        }
    }
}

/// Availability probe for the project-owned `dcc-cua` runtime.
///
/// Implementations must use the official component contract
/// ([`preflight_commands`]) and must never report a generic computer-use
/// provider as a substitute for an unavailable runtime. Reporting
/// [`CuaRuntimeState::Unknown`] is always safer than guessing.
pub trait CuaRuntimeProbe {
    /// Report the current state of the `dcc-cua` runtime.
    fn probe(&self) -> CuaRuntimeState;
}

impl<T: CuaRuntimeProbe + ?Sized> CuaRuntimeProbe for &T {
    fn probe(&self) -> CuaRuntimeState {
        (**self).probe()
    }
}

/// Probe that always reports a fixed state.
///
/// Used by tests and by surfaces that resolve runtime availability out of
/// band (for example a gateway that already tracked the component).
#[derive(Debug, Clone, Copy)]
pub struct StaticCuaProbe {
    state: CuaRuntimeState,
}

impl StaticCuaProbe {
    /// Build a probe that always answers `state`.
    #[must_use]
    pub const fn new(state: CuaRuntimeState) -> Self {
        Self { state }
    }

    /// Probe that reports a healthy, ready runtime.
    #[must_use]
    pub const fn ready() -> Self {
        Self::new(CuaRuntimeState::Ready)
    }
}

impl CuaRuntimeProbe for StaticCuaProbe {
    fn probe(&self) -> CuaRuntimeState {
        self.state
    }
}

/// Advice attached to a search response when no scriptable interface can
/// serve the request.
///
/// The hits are still returned alongside this; the fallback is additive, never
/// a replacement for the result set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchFallback {
    /// Route to use. Always [`FALLBACK_SKILL`] — see the module contract.
    pub skill: String,
    /// Why the result set could not serve the request.
    ///
    /// One of `no_candidate`, `low_confidence`, or `no_executable_interface`.
    pub reason: String,
    /// State of the recommended route at the time of the search.
    pub runtime: CuaRuntimeState,
    /// `true` when the route cannot be used. The caller must repair it or
    /// report the blocker — **not** switch provider.
    pub blocked: bool,
    /// Reason code for the blocker, set only when `blocked` is `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    /// One sentence the calling agent can hand to the user.
    pub message: String,
    /// Official component commands for inspecting and repairing the route.
    #[serde(default)]
    pub preflight: Vec<String>,
}

/// Turn a trigger plus a runtime state into wire advice.
///
/// `reason` always carries the trigger, so a blocked response still says *why*
/// the fallback was needed; `blocked_reason` then says why it cannot be used.
#[must_use]
pub fn build_fallback(trigger: FallbackTrigger, runtime: CuaRuntimeState) -> SearchFallback {
    let why = match trigger {
        FallbackTrigger::NoCandidate => {
            "nothing in the catalog matched this request, so there is no scriptable interface to call"
        }
        FallbackTrigger::LowConfidence => {
            "the closest matches are lexical near-misses rather than a real interface for this request"
        }
        FallbackTrigger::NoExecutableInterface => {
            "the closest match is documentation-only and declares no callable tool"
        }
    };

    let (blocked, blocked_reason, message) = if runtime.is_actionable() {
        let tail = if runtime.is_usable() {
            String::new()
        } else {
            // Unverified: still the right route, just confirm it first.
            format!(
                " Run the official preflight before use — {}.",
                runtime.blocker()
            )
        };
        (
            false,
            None,
            format!(
                "{why}; route the task to `{FALLBACK_SKILL}` (project-owned UI control).{tail}"
            ),
        )
    } else {
        (
            true,
            Some(FALLBACK_REASON_CUA_UNAVAILABLE.to_string()),
            format!(
                "{why}; the only permitted route is `{FALLBACK_SKILL}`, which is currently unusable — {}. Do not substitute another provider; repair the route or report the blocker.",
                runtime.blocker()
            ),
        )
    };

    SearchFallback {
        skill: FALLBACK_SKILL.to_string(),
        reason: trigger.reason_code().to_string(),
        runtime,
        blocked,
        blocked_reason,
        message,
        preflight: preflight_commands(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{
        FALLBACK_REASON_LOW_CONFIDENCE, FALLBACK_REASON_NO_CANDIDATE,
        FALLBACK_REASON_NO_EXECUTABLE_INTERFACE,
    };

    #[test]
    fn ready_runtime_produces_an_actionable_recommendation() {
        let advice = build_fallback(FallbackTrigger::NoCandidate, CuaRuntimeState::Ready);
        assert_eq!(advice.skill, FALLBACK_SKILL);
        assert_eq!(advice.reason, FALLBACK_REASON_NO_CANDIDATE);
        assert!(!advice.blocked);
        assert!(advice.blocked_reason.is_none());
        assert!(advice.message.contains(FALLBACK_SKILL));
    }

    #[test]
    fn unusable_runtime_is_blocked_not_rerouted() {
        for state in [
            CuaRuntimeState::NotResponding,
            CuaRuntimeState::Missing,
            CuaRuntimeState::Incompatible,
            CuaRuntimeState::Unknown,
        ] {
            let advice = build_fallback(FallbackTrigger::LowConfidence, state);
            assert!(advice.blocked, "{state:?} must block the route");
            assert_eq!(
                advice.blocked_reason.as_deref(),
                Some(FALLBACK_REASON_CUA_UNAVAILABLE)
            );
            assert_eq!(advice.skill, FALLBACK_SKILL, "never another provider");
            assert_eq!(advice.reason, FALLBACK_REASON_LOW_CONFIDENCE);
        }
    }

    #[test]
    fn blocked_advice_keeps_the_original_trigger() {
        let advice = build_fallback(
            FallbackTrigger::NoExecutableInterface,
            CuaRuntimeState::Missing,
        );
        assert_eq!(advice.reason, FALLBACK_REASON_NO_EXECUTABLE_INTERFACE);
        assert_eq!(
            advice.blocked_reason.as_deref(),
            Some(FALLBACK_REASON_CUA_UNAVAILABLE)
        );
    }

    #[test]
    fn advice_never_names_a_forbidden_provider() {
        for trigger in [
            FallbackTrigger::NoCandidate,
            FallbackTrigger::LowConfidence,
            FallbackTrigger::NoExecutableInterface,
        ] {
            for state in [
                CuaRuntimeState::Ready,
                CuaRuntimeState::NotResponding,
                CuaRuntimeState::Missing,
                CuaRuntimeState::Incompatible,
                CuaRuntimeState::Unknown,
            ] {
                let advice = build_fallback(trigger, state);
                assert_eq!(advice.skill, FALLBACK_SKILL);
                for forbidden in FORBIDDEN_SUBSTITUTES {
                    assert!(
                        !advice.message.contains(forbidden),
                        "advice must not suggest {forbidden}"
                    );
                }
            }
        }
    }

    #[test]
    fn preflight_uses_the_official_component_contract() {
        let commands = preflight_commands();
        assert!(commands.contains(&PREFLIGHT_STATUS_CMD.to_string()));
        assert!(commands.contains(&PREFLIGHT_ENSURE_CMD.to_string()));
        assert!(commands.contains(&PREFLIGHT_MANIFEST_CMD.to_string()));
        assert!(commands.contains(&PREFLIGHT_PING_CMD.to_string()));
    }

    #[test]
    fn advice_round_trips_through_serde() {
        let advice = build_fallback(FallbackTrigger::NoCandidate, CuaRuntimeState::Missing);
        let json = serde_json::to_string(&advice).unwrap();
        let back: SearchFallback = serde_json::from_str(&json).unwrap();
        assert_eq!(back, advice);
        // `blocked_reason` is absent, not null, when the route is usable.
        let ready = build_fallback(FallbackTrigger::NoCandidate, CuaRuntimeState::Ready);
        let json = serde_json::to_string(&ready).unwrap();
        assert!(!json.contains("blocked_reason"));
    }

    #[test]
    fn static_probe_reports_its_state() {
        assert_eq!(StaticCuaProbe::ready().probe(), CuaRuntimeState::Ready);
        assert_eq!(
            StaticCuaProbe::new(CuaRuntimeState::Missing).probe(),
            CuaRuntimeState::Missing
        );
        // The blanket impl over `&T` makes a reference-to-probe usable
        // wherever a probe is expected.
        let probe = StaticCuaProbe::ready();
        let nested = &&probe;
        assert_eq!(nested.probe(), CuaRuntimeState::Ready);
    }
}
