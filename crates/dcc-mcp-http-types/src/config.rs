//! Configuration value types exposed on the HTTP server wire surface.
//!
//! These are the enums and small value types that the Python binding,
//! CLI flags, and environment-variable plumbing branch on. They live
//! here (rather than in `dcc-mcp-http::config`) so external Rust
//! tooling — CLI drivers, config validators, adapter orchestrators —
//! can depend on just the enumeration contract without dragging in
//! `axum` / `tokio` / `reqwest` / `pyo3`.
//!
//! The full `McpHttpConfig` struct stays in `dcc-mcp-http::config`
//! until its sub-struct split lands; this module captures just the
//! self-contained enums for now.

use serde::{Deserialize, Serialize};

// ── ServerSpawnMode ────────────────────────────────────────────────────────

/// How the server and gateway HTTP listeners are driven.
///
/// Fixes **issue #303** — under PyO3-embedded interpreters (Maya on Windows),
/// `tokio::spawn` onto a multi-threaded runtime that no longer has an active
/// driver can cause background accept loops (specifically the gateway
/// listener) to be starved of scheduling time. The per-instance listener
/// survives because its accept loop is "warmed up" during the initial
/// `block_on`, but the gateway listener — spawned via an extra `tokio::spawn`
/// + `tokio::join!` layer — never gets its turn.
///
/// `ServerSpawnMode::Dedicated` avoids the failure mode entirely by running
/// each HTTP listener on its own OS thread that owns a `current_thread`
/// Tokio runtime. That thread is scheduled by the OS, not by a shared
/// worker pool, and cannot be starved by a hanging block_on elsewhere.
///
/// | Mode | When to use | Behaviour |
/// |------|-------------|-----------|
/// | `Ambient`   | Standalone binary (`dcc-mcp-server`, library tests) | Spawns `axum::serve` onto the caller's Tokio runtime via `tokio::spawn`. |
/// | `Dedicated` | Python bindings (`PyMcpHttpServer`) / embedded DCC hosts | Each listener gets its own OS thread + `current_thread` runtime. Immune to PyO3 worker starvation. |
///
/// Defaults: `Ambient`. The Python bindings override this to `Dedicated`
/// automatically when constructing `McpHttpServer` via `PyMcpHttpServer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerSpawnMode {
    /// Spawn listeners as background tasks on the caller's Tokio runtime.
    /// Correct for `#[tokio::main]` binaries that keep a thread in the
    /// runtime for the process lifetime.
    #[default]
    Ambient,

    /// Spawn each listener on a dedicated OS thread with its own
    /// `current_thread` runtime. Correct for PyO3-embedded interpreters
    /// where the parent runtime's worker pool cannot be relied upon after
    /// `block_on` returns.
    Dedicated,
}

// ── JobRecoveryPolicy ──────────────────────────────────────────────────────

/// What `McpHttpServer::start` does with rows that the previous process
/// left in `Pending` / `Running` after a crash or restart (issue #567).
///
/// | Variant | Behaviour |
/// |---------|-----------|
/// | [`JobRecoveryPolicy::Drop`]    | Each in-flight row is rewritten to `JobStatus::Interrupted` with `error = "server restart"`. Clients re-subscribing after reconnect see one clean terminal transition. **This is today's behaviour and the default.** |
/// | [`JobRecoveryPolicy::Requeue`] | Reserved for a future release that persists the original tool arguments alongside the `jobs` row. Until that lands the variant is **accepted but treated as `Drop`** — the server logs a `WARN` at startup so operators know the requested policy is not yet active, but startup itself never fails. The accepted-but-degraded contract gives DCC adapters (`dcc-mcp-maya`, `dcc-mcp-houdini`) a stable knob to plumb through today without forcing a config-shape break when the real implementation lands. |
///
/// String form (used by the Python binding and the `--job-recovery` CLI
/// flag): `"drop"` / `"requeue"`. Defaults to `Drop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobRecoveryPolicy {
    /// Rewrite every `Pending` / `Running` row to `Interrupted` on startup.
    /// Always safe; never re-runs a partially-applied tool.
    #[default]
    Drop,
    /// Reserved policy: would re-submit idempotent in-flight jobs from the
    /// persisted spec. Accepted today but treated as [`Self::Drop`] with a
    /// `WARN` log at startup until tool-arg persistence lands.
    Requeue,
}

impl JobRecoveryPolicy {
    /// Lower-case wire identifier used by docs, the Python binding, and the
    /// `--job-recovery` CLI flag.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Requeue => "requeue",
        }
    }

    /// Parse the wire identifier. `&str` is matched case-insensitively to
    /// be tolerant of env-var plumbing (`DCC_MCP_*_JOB_RECOVERY=Requeue`).
    ///
    /// # Errors
    ///
    /// Returns a descriptive `Err` when `value` does not match any known
    /// variant, naming the rejected value and the accepted set.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "drop" => Ok(Self::Drop),
            "requeue" => Ok(Self::Requeue),
            other => Err(format!(
                "unknown job_recovery policy {other:?}; expected \"drop\" or \"requeue\""
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ServerSpawnMode ────────────────────────────────────────────────

    #[test]
    fn server_spawn_mode_defaults_to_ambient() {
        assert_eq!(ServerSpawnMode::default(), ServerSpawnMode::Ambient);
    }

    #[test]
    fn server_spawn_mode_wire_is_snake_case() {
        // `ambient` / `dedicated` is the wire form the Python binding
        // and env-var plumbing round-trip. Pin it so a future derive
        // tweak cannot silently break downstream consumers.
        assert_eq!(
            serde_json::to_string(&ServerSpawnMode::Ambient).unwrap(),
            "\"ambient\""
        );
        assert_eq!(
            serde_json::to_string(&ServerSpawnMode::Dedicated).unwrap(),
            "\"dedicated\""
        );

        let back: ServerSpawnMode = serde_json::from_str("\"dedicated\"").unwrap();
        assert_eq!(back, ServerSpawnMode::Dedicated);
    }

    // ── JobRecoveryPolicy ──────────────────────────────────────────────

    /// Issue #567: the policy enum defaults to `Drop` so existing callers
    /// inherit today's behaviour without touching their config.
    #[test]
    fn job_recovery_default_is_drop() {
        assert_eq!(JobRecoveryPolicy::default(), JobRecoveryPolicy::Drop);
    }

    /// Issue #567: the wire identifier round-trips to the same shape the
    /// Python binding exposes.
    #[test]
    fn job_recovery_as_str_matches_wire() {
        assert_eq!(JobRecoveryPolicy::Drop.as_str(), "drop");
        assert_eq!(JobRecoveryPolicy::Requeue.as_str(), "requeue");
    }

    /// Issue #567: env-var plumbing (`DCC_MCP_*_JOB_RECOVERY=Requeue`) and
    /// the Python setter share the same case-insensitive parser.
    #[test]
    fn job_recovery_parse_is_case_insensitive() {
        for raw in ["drop", "Drop", "DROP", "  drop  "] {
            assert_eq!(JobRecoveryPolicy::parse(raw), Ok(JobRecoveryPolicy::Drop));
        }
        for raw in ["requeue", "Requeue", "REQUEUE"] {
            assert_eq!(
                JobRecoveryPolicy::parse(raw),
                Ok(JobRecoveryPolicy::Requeue)
            );
        }
    }

    /// Issue #567: unknown policies surface a descriptive error that
    /// names the rejected value and the accepted set.
    #[test]
    fn job_recovery_parse_rejects_unknown() {
        let err = JobRecoveryPolicy::parse("retry").unwrap_err();
        assert!(err.contains("retry"), "error message: {err}");
        assert!(err.contains("drop"), "error message: {err}");
        assert!(err.contains("requeue"), "error message: {err}");
    }

    /// The snake_case JSON form matches the CLI / env-var string form,
    /// so operators can read either serialisation interchangeably.
    #[test]
    fn job_recovery_wire_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&JobRecoveryPolicy::Drop).unwrap(),
            "\"drop\""
        );
        assert_eq!(
            serde_json::to_string(&JobRecoveryPolicy::Requeue).unwrap(),
            "\"requeue\""
        );

        let back: JobRecoveryPolicy = serde_json::from_str("\"requeue\"").unwrap();
        assert_eq!(back, JobRecoveryPolicy::Requeue);
    }
}
