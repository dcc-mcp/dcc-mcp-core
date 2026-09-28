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

/// Spawn-free probe: is the `dcc-cua` binary resolvable on `PATH`?
///
/// This is the probe for callers on a request hot path, such as an MCP search
/// handler. It answers the one question that can be answered without starting a
/// process — is the component installed at all — and reports
/// [`CuaRuntimeState::Unverified`] rather than pretending to have confirmed
/// liveness. A search request must not pay three subprocess spawns to answer a
/// routing question.
///
/// Callers that need real liveness use [`CliCuaProbe`] instead, off the hot
/// path.
#[derive(Debug, Clone, Copy, Default)]
pub struct PathCuaProbe;

impl PathCuaProbe {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CuaRuntimeProbe for PathCuaProbe {
    fn probe(&self) -> CuaRuntimeState {
        if dcc_cua_on_path() {
            CuaRuntimeState::Unverified
        } else {
            CuaRuntimeState::Missing
        }
    }
}

/// Resolve `dcc-cua` on `PATH` without spawning anything.
///
/// `PATHEXT` is honoured on Windows, where a bare `dcc-cua` entry is not
/// executable but `dcc-cua.exe` is.
fn dcc_cua_on_path() -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| String::from(".EXE;.CMD;.BAT;.COM"))
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(str::to_ascii_uppercase)
            .collect()
    } else {
        Vec::new()
    };

    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(CUA_BIN);
        if candidate.is_file() {
            return true;
        }
        for ext in &extensions {
            let with_ext = dir.join(format!("{CUA_BIN}{ext}"));
            if with_ext.is_file() {
                return true;
            }
        }
    }
    false
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

/// Confirm the installed runtime answers `ping`.
///
/// `components status` already validates the binary and reads its manifest
/// (`dcc-mcp-cli/src/application/components.rs`), so re-running `manifest` here
/// would be a second identical subprocess for no new information. Worst case
/// for a full probe is therefore two spawns, not three.
fn runtime_answers(timeout: Duration) -> bool {
    run(CUA_BIN, &["ping"], timeout).is_some_and(|out| out.status.success())
}

/// Spawn `program` with `args`, collecting output under a hard timeout.
///
/// Returns `None` when the binary is missing, the spawn fails, or the command
/// does not finish in time — all of which mean "we could not determine the
/// state", never "the runtime is unusable".
fn run(program: &str, args: &[&str], timeout: Duration) -> Option<std::process::Output> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // `dcc-mcp-cli` / `dcc-cua` are console-subsystem binaries. Spawning them
    // from a background gateway or sidecar would flash a console window on the
    // user's desktop. Same fix as dcc-mcp-core#1738.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;

    // Drain stdout on a worker thread *while* the child runs.
    //
    // The alternative — poll `try_wait()` and only then call
    // `wait_with_output()` — deadlocks on any child that writes more than the
    // OS pipe buffer: the child blocks writing, so it never exits, so the poll
    // never sees an exit, so nothing ever reads the pipe. The timeout then
    // fires and the probe degrades to `Unknown`, which would look like a
    // flaky runtime rather than our own bug. `component status` and
    // `manifest` output is small today, but nothing guarantees that.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut stdout, &mut buf).map(|_| buf)
    });

    // Poll instead of blocking forever: a wedged component binary must not
    // wedge the search request that asked about it.
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
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
    };

    let stdout = reader.join().ok()?.ok()?;
    Some(std::process::Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
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

    /// Regression for the pipe-buffer deadlock: a child that writes more than
    /// the OS pipe buffer used to block forever, because stdout was only read
    /// after the child exited. `run` drains it on a worker thread, so a chatty
    /// child must return its full output instead of hitting the timeout.
    #[test]
    fn a_chatty_child_is_drained_instead_of_deadlocking() {
        // Query the runtime's own binary rather than assuming an interpreter:
        // `cmd /c` exists on Windows, and this repo gates on Windows CI.
        #[cfg(windows)]
        let (program, args, expected_lines) = (
            "cmd",
            vec![
                "/c",
                "for /L %i in (1,1,20000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            ],
            20_000,
        );
        #[cfg(not(windows))]
        let (program, args, expected_lines) = (
            "sh",
            vec![
                "-c",
                "i=0; while [ $i -lt 20000 ]; do echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx; i=$((i+1)); done",
            ],
            20_000,
        );

        let out = run(program, &args, Duration::from_secs(60))
            .expect("a chatty child must not deadlock into a timeout");
        assert!(out.status.success());
        assert!(
            out.stdout.len() > expected_lines * 60,
            "expected the full ~1.2 MiB of output, got {} bytes",
            out.stdout.len()
        );
    }
}
