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
