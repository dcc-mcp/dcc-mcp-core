use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

// ── JobConfig ──────────────────────────────────────────────────────────────

/// Job persistence & recovery configuration.
///
/// One of the orthogonal sub-configs that compose `McpHttpConfig`
/// (issue #852). Captured here as a pure value type so external
/// tooling (config validators, CLI inspectors) can depend on the
/// shape without pulling in the rest of the HTTP server crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    /// Path to a SQLite database file for persisting tracked jobs
    /// (issue #328). `None` means in-memory storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_storage_path: Option<PathBuf>,

    /// What to do with rows the previous process left in `Pending` /
    /// `Running` after a crash or restart (issue #567).
    #[serde(default)]
    pub job_recovery: JobRecoveryPolicy,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            job_storage_path: None,
            job_recovery: JobRecoveryPolicy::Drop,
        }
    }
}
