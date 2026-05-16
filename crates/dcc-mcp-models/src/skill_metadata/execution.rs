use serde::{Deserialize, Serialize};

// ── ExecutionMode ─────────────────────────────────────────────────────────

/// How a tool is expected to execute with respect to request/response latency.
///
/// Authors declare `execution` in SKILL.md. The MCP server derives the
/// `deferredHint` annotation from this value (per MCP 2025-03-26 the hint
/// is server-set — end users should not set it directly). See issue #317.
///
/// ```yaml
/// tools:
///   - name: render_frames
///     execution: async          # sync | async ; default sync
///     timeout_hint_secs: 600    # optional u32
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    /// Returns quickly — callers expect a synchronous reply.
    #[default]
    Sync,
    /// May take long enough that clients should treat the call as deferred.
    /// Surfaces as `deferredHint: true` on the MCP tool annotation.
    Async,
}

impl ExecutionMode {
    /// Whether this mode should surface as a deferred hint in MCP tool
    /// annotations.
    #[must_use]
    pub fn is_deferred(self) -> bool {
        matches!(self, Self::Async)
    }
}

// ── ThreadAffinity (issue #332) ───────────────────────────────────────────

/// Where a tool is allowed to execute.
///
/// Skill authors declare `thread-affinity` in SKILL.md / `tools.yaml` for tools
/// that must run on the DCC application's main thread (e.g. anything that
/// touches `maya.cmds`, `bpy.ops`, `hou.*`, `pymxs.runtime`).
///
/// The HTTP server reads this value at dispatch time — main-affined tools are
/// routed through [`DeferredExecutor`] even when the caller used the async
/// `tools/call` path (#318). `Any` (default) tools execute on a Tokio worker.
///
/// This mirrors [`dcc_mcp_process::dispatcher::ThreadAffinity`] with the
/// `Named` variant dropped — named threads are an adapter concern that never
/// travels through the skill-metadata layer.
///
/// Examples:
///
/// ```rust
/// use dcc_mcp_models::ThreadAffinity;
///
/// assert_eq!(ThreadAffinity::default(), ThreadAffinity::Any);
/// let v = serde_json::to_string(&ThreadAffinity::Main).unwrap();
/// assert_eq!(v, "\"main\"");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadAffinity {
    /// No constraint — the tool may run on any worker thread.
    #[default]
    Any,
    /// Must run on the DCC application's main thread.
    Main,
}

impl ThreadAffinity {
    /// Parse a case-insensitive affinity string — returns `None` for unknown
    /// values so callers can decide between defaulting and rejecting.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "any" | "" => Some(Self::Any),
            "main" => Some(Self::Main),
            _ => None,
        }
    }

    /// Human-readable lowercase tag suitable for MCP `_meta` surfaces.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Main => "main",
        }
    }

    /// Whether the tool must run on the DCC main thread.
    #[must_use]
    pub fn is_main(self) -> bool {
        matches!(self, Self::Main)
    }
}

impl std::fmt::Display for ThreadAffinity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── RiskClass (RFC #998 Phase 1 — schema only) ────────────────────────────

/// Crash-risk classification for a tool action.
///
/// Schema-level field landed for the sidecar epic (see RFC #998 / issue
/// #1002).  This is intentionally **schema-only** in this release: the
/// gateway router parses the value, surfaces it in `_meta.dcc.risk_class`
/// on `tools/list`, and logs the routing intent — but every call still
/// executes through the existing in-process path.  Routing high-risk
/// actions to an out-of-process sidecar lands in Phase 2 once the
/// `dcc-mcp-host-rpc` crate is available.
///
/// Skill authors declare `risk_class` in `tools.yaml` for actions whose
/// historical failure mode is a **C++ abort** or **modal native dialog**
/// the in-process Python defence-in-depth cannot intercept (`playblast`,
/// `capture_viewport`, `cmds.render`, destructive `cmds.file`, heavy
/// simulation, `execute_python` / `execute_mel`):
///
/// ```yaml
/// tools:
///   - name: playblast
///     execution: async
///     affinity: main
///     timeout_hint_secs: 600
///     risk_class: high-crash    # opt in to sidecar routing once it lands
/// ```
///
/// Default is `low` — the vast majority of skills are cheap reads or pure
/// Python ops that benefit from in-process latency. `high-crash` is an
/// explicit per-action opt-in to crash isolation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskClass {
    /// Default — runs in-process. The current behaviour for every tool
    /// today.
    #[default]
    Low,
    /// Historically prone to host-killing C++ aborts or modal native
    /// dialogs the Python dispatcher cannot intercept. Once the sidecar
    /// substrate lands (see issue #1002), the gateway routes these
    /// actions through an out-of-process worker so a Maya / Blender /
    /// Houdini crash returns a structured `host-died` envelope instead
    /// of an `instance-offline` cascade.
    HighCrash,
}

impl RiskClass {
    /// Parse a case-insensitive risk-class tag — accepts both kebab-case
    /// (`"high-crash"`) and snake_case (`"high_crash"`) so YAML authors
    /// can use either convention.
    ///
    /// Returns `None` for unknown values so callers can decide between
    /// defaulting to `Low` and rejecting (the SKILL.md parser opts to
    /// reject so typos surface early).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "low" | "" => Some(Self::Low),
            "high-crash" | "high_crash" | "highcrash" => Some(Self::HighCrash),
            _ => None,
        }
    }

    /// Human-readable kebab-case tag suitable for MCP `_meta` surfaces
    /// and structured logs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::HighCrash => "high-crash",
        }
    }

    /// Whether this action should be routed to an out-of-process sidecar
    /// once the sidecar substrate is available.  Always false today
    /// because Phase 2 hasn't landed; flips to match `HighCrash` when
    /// the gateway router gains sidecar awareness.
    #[must_use]
    pub fn is_high_crash(self) -> bool {
        matches!(self, Self::HighCrash)
    }
}

impl std::fmt::Display for RiskClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod risk_class_tests {
    use super::RiskClass;

    #[test]
    fn default_is_low() {
        assert_eq!(RiskClass::default(), RiskClass::Low);
        assert!(!RiskClass::default().is_high_crash());
    }

    #[test]
    fn parse_accepts_kebab_snake_and_empty() {
        assert_eq!(RiskClass::parse("high-crash"), Some(RiskClass::HighCrash));
        assert_eq!(RiskClass::parse("high_crash"), Some(RiskClass::HighCrash));
        assert_eq!(RiskClass::parse("HighCrash"), Some(RiskClass::HighCrash));
        assert_eq!(RiskClass::parse("low"), Some(RiskClass::Low));
        assert_eq!(RiskClass::parse("LOW"), Some(RiskClass::Low));
        assert_eq!(RiskClass::parse(""), Some(RiskClass::Low));
        assert_eq!(RiskClass::parse("medium"), None);
    }

    #[test]
    fn serde_uses_kebab_case() {
        let json = serde_json::to_string(&RiskClass::HighCrash).unwrap();
        assert_eq!(json, "\"high-crash\"");
        let json = serde_json::to_string(&RiskClass::Low).unwrap();
        assert_eq!(json, "\"low\"");
        let parsed: RiskClass = serde_json::from_str("\"high-crash\"").unwrap();
        assert_eq!(parsed, RiskClass::HighCrash);
    }

    #[test]
    fn display_matches_serde() {
        assert_eq!(RiskClass::Low.to_string(), "low");
        assert_eq!(RiskClass::HighCrash.to_string(), "high-crash");
    }
}
