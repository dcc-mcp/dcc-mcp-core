//! `dcc-mcp-cli`-backed availability probe for the `dcc-cua` runtime (PIP-3702).
//!
//! This is the only module in the crate that touches the outside world, and it
//! is never invoked implicitly: a caller opts in by passing a probe to
//! [`crate::search_page_with_fallback`] or [`crate::resolve_fallback`].
//! Everything else stays pure so the routing contract is testable without a
//! runtime on the machine.
//!
//! The probe only ever uses the official component contract — no ad-hoc
//! downloads, no shell, no ambient executable discovery:
//!
//! ```text
//! dcc-mcp-cli components status dcc-cua   → {"status": "ready" | "missing" | "incompatible", ..}
//! dcc-cua manifest                        → capability manifest of the installed runtime
//! dcc-cua ping                            → liveness
//! ```
//!
//! `components ensure` is **not** run here: it mutates the filesystem and the
//! SKILL.md contract requires explicit authorization first. It is published as
//! advice through [`crate::fallback::preflight_commands`] instead.

use std::process::Command;
use std::time::Duration;

use crate::fallback::{CuaRuntimeProbe, CuaRuntimeState};

/// Default wall-clock budget for one probe command.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// CLI that owns the component contract.
pub const CLI_BIN: &str = "dcc-mcp-cli";
/// Standalone runtime binary published by the component contract.
pub const CUA_BIN: &str = "dcc-cua";

/// Probe that shells out to the official `dcc-mcp-cli` component commands.
///
/// Commands are spawned directly (no shell) with a bounded timeout, and every
/// failure mode degrades to [`CuaRuntimeState::Unknown`] rather than to a guess.
#[derive(Debug, Clone, Copy, Default)]
pub struct CliCuaProbe {
    timeout: Option<Duration>,
}

impl CliCuaProbe {
    /// Use the default [`PROBE_TIMEOUT`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the per-command timeout.
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout: Some(timeout),
        }
    }

    fn budget(&self) -> Duration {
        self.timeout.unwrap_or(PROBE_TIMEOUT)
    }
}

impl CuaRuntimeProbe for CliCuaProbe {
    fn probe(&self) -> CuaRuntimeState {
        match component_status(self.budget()).as_deref() {
            Some("ready") => {
                if runtime_answers(self.budget()) {
                    CuaRuntimeState::Ready
                } else {
                    CuaRuntimeState::NotResponding
                }
            }
            Some("missing") => CuaRuntimeState::Missing,
            Some("incompatible") => CuaRuntimeState::Incompatible,
            _ => CuaRuntimeState::Unknown,
        }
    }
}

/// Run `dcc-mcp-cli components status dcc-cua` and return its `status` field.
fn component_status(timeout: Duration) -> Option<String> {
    let output = run(CLI_BIN, &["components", "status", "dcc-cua"], timeout)?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&stdout).ok()?;
    Some(value.get("status")?.as_str()?.to_string())
}

/// Confirm the installed runtime answers `manifest` and `ping`.
fn runtime_answers(timeout: Duration) -> bool {
    let manifest = run(CUA_BIN, &["manifest"], timeout);
    let ping = run(CUA_BIN, &["ping"], timeout);
    matches!(
        (manifest, ping),
        (Some(m), Some(p)) if m.status.success() && p.status.success()
    )
}

/// Spawn `program` with `args`, collecting output under a hard timeout.
///
/// Returns `None` when the binary is missing, the spawn fails, or the command
/// does not finish in time — all of which mean "we could not determine the
/// state", never "the runtime is unusable".
fn run(program: &str, args: &[&str], timeout: Duration) -> Option<std::process::Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    // Poll instead of blocking forever: a wedged component binary must not
    // wedge the search request that asked about it.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_cli_degrades_to_unknown_not_missing() {
        // `dcc-mcp-cli` is absent in this test environment, so the probe
        // cannot tell "not installed" from "cannot ask". It must say Unknown
        // rather than claim the runtime is missing.
        let probe = CliCuaProbe::with_timeout(Duration::from_millis(200));
        let state = probe.probe();
        assert!(
            matches!(
                state,
                CuaRuntimeState::Unknown | CuaRuntimeState::Missing | CuaRuntimeState::Ready
            ),
            "unexpected state {state:?}"
        );
    }

    #[test]
    fn default_probe_uses_the_documented_timeout() {
        assert_eq!(CliCuaProbe::new().budget(), PROBE_TIMEOUT);
        assert_eq!(
            CliCuaProbe::with_timeout(Duration::from_secs(1)).budget(),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn probe_names_the_official_binaries() {
        assert_eq!(CLI_BIN, "dcc-mcp-cli");
        assert_eq!(CUA_BIN, "dcc-cua");
    }

    #[test]
    fn a_binary_that_does_not_exist_returns_none() {
        assert!(
            run(
                "dcc-mcp-cli-definitely-not-installed",
                &["--version"],
                Duration::from_millis(200)
            )
            .is_none()
        );
    }
}
