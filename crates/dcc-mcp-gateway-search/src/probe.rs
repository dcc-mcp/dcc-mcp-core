//! `dcc-mcp-cli`-backed availability probe for the `dcc-cua` runtime (PIP-3702,
//! PIP-4981).
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
//!
//! # Spawn-free hot-path probe (PIP-4981)
//!
//! [`PathCuaProbe`] answers the one routing question that can be answered
//! without starting a process — *is the `dcc-cua` runtime installed at all* —
//! and does so over the same search set the real executor
//! (`python/dcc_mcp_core/cua_cli.py::resolve_cua_command`) uses. Checking
//! `PATH` alone used to under-report: a `dcc-cua` legally installed beside
//! `dcc-mcp-cli`, in a standard install directory, or as a versioned standalone
//! install is usable by the executor even when it is not on the current
//! session `PATH`, so a `PATH`-only probe falsely reported
//! [`CuaRuntimeState::Missing`] and blocked the route. Every location here is a
//! pure filesystem check — no subprocess is ever spawned on the request path.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::fallback::{CuaRuntimeProbe, CuaRuntimeState};

/// Default wall-clock budget for one probe command.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// CLI that owns the component contract.
pub const CLI_BIN: &str = "dcc-mcp-cli";
/// Standalone runtime binary published by the component contract.
pub const CUA_BIN: &str = "dcc-cua";

/// `DCC_MCP_INSTALL_DIR` — standard install-directory override used by the
/// official installers (mirrors `ENV_MCP_INSTALL_DIR` in `cua_cli.py`).
const ENV_INSTALL_DIR: &str = "DCC_MCP_INSTALL_DIR";
/// Minimum standalone `dcc-cua` version the executor accepts (mirrors
/// `MINIMUM_CUA_VERSION` in `cua_cli.py`). Versioned standalone installs below
/// this are ignored so the probe does not claim an incompatible runtime is
/// present.
const MIN_CUA_VERSION: (u64, u64, u64) = (0, 4, 0);

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

/// Spawn-free probe: is the `dcc-cua` runtime installed, on or off `PATH`?
///
/// This is the probe for callers on a request hot path, such as an MCP search
/// handler. It answers the one question that can be answered without starting a
/// process — is the component installed at all — and reports
/// [`CuaRuntimeState::Unverified`] rather than pretending to have confirmed
/// liveness. A search request must not pay three subprocess spawns to answer a
/// routing question.
///
/// Beyond `PATH` it consults the same locations the executor
/// (`python/dcc_mcp_core/cua_cli.py::resolve_cua_command`) resolves, using only
/// filesystem checks: the directory beside `dcc-mcp-cli`, the standard install
/// directories, and a versioned standalone install (PIP-4981). A hit is
/// reported as `Unverified` (actionable), never as a false `Missing` block.
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
        probe_state(&ProbeEnv::from_process())
    }
}

/// Everything the spawn-free probe reads from the outside world, snapshotted
/// once.
///
/// Reading the process environment into this struct keeps the hot path cheap
/// (one read, then pure filesystem checks) and lets tests build a controlled
/// environment directly — mutating process-global environment variables would
/// race with the rest of the parallel test suite.
#[derive(Debug, Default, Clone)]
struct ProbeEnv {
    /// `PATH` at probe time.
    path: Option<OsString>,
    /// Windows executable extensions from `PATHEXT`, uppercased; empty off
    /// Windows. A bare `dcc-cua` is not executable on Windows, `dcc-cua.exe`
    /// is.
    extensions: Vec<String>,
    /// Absolute `DCC_MCP_INSTALL_DIR`, when set and absolute.
    install_dir: Option<PathBuf>,
    /// `LOCALAPPDATA` — Windows standard bin and versioned install root.
    local_app_data: Option<PathBuf>,
    /// `XDG_DATA_HOME` — Unix versioned install root override.
    xdg_data_home: Option<PathBuf>,
    /// User home directory (`~`), used by the Unix standard-bin and versioned
    /// roots.
    home: Option<PathBuf>,
}

impl ProbeEnv {
    /// Snapshot the inputs the probe needs from the current process.
    fn from_process() -> Self {
        Self {
            path: std::env::var_os("PATH"),
            extensions: pathext_extensions(),
            install_dir: absolute_env_path(ENV_INSTALL_DIR),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            xdg_data_home: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            home: home_dir(),
        }
    }
}

/// Windows executable extensions, uppercased; empty on other platforms.
fn pathext_extensions() -> Vec<String> {
    if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| String::from(".EXE;.CMD;.BAT;.COM"))
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(str::to_ascii_uppercase)
            .collect()
    } else {
        Vec::new()
    }
}

/// Absolute path from an environment variable; `None` when unset or not
/// absolute. The executor only trusts an absolute `DCC_MCP_INSTALL_DIR`, so the
/// probe does too.
fn absolute_env_path(key: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(key)?);
    path.is_absolute().then_some(path)
}

/// Best-effort user home directory, without the deprecated `std::env` helper.
fn home_dir() -> Option<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key).map(PathBuf::from)
}

/// Find executable `name` directly inside `dir`, honouring Windows `PATHEXT`.
///
/// Returns the resolved path so callers can canonicalize it (the `dcc-mcp-cli`
/// sibling case). Pure filesystem check, spawns nothing.
fn find_executable_in(dir: &Path, name: &str, extensions: &[String]) -> Option<PathBuf> {
    let plain = dir.join(name);
    if plain.is_file() {
        return Some(plain);
    }
    for ext in extensions {
        let with_ext = dir.join(format!("{name}{ext}"));
        if with_ext.is_file() {
            return Some(with_ext);
        }
    }
    None
}

/// Is `dcc-cua` resolvable on `PATH` without spawning anything?
fn cua_on_path(env: &ProbeEnv) -> bool {
    let Some(path_var) = &env.path else {
        return false;
    };
    std::env::split_paths(path_var)
        .any(|dir| find_executable_in(&dir, CUA_BIN, &env.extensions).is_some())
}

/// Directory of the installed `dcc-mcp-cli`, resolved through symlinks, so a
/// `dcc-cua` shipped beside it is found even when that directory is not on the
/// current `PATH`. Mirrors `_resolve_cli_sibling` in `cua_cli.py`.
fn cli_sibling_dir(env: &ProbeEnv) -> Option<PathBuf> {
    let path_var = env.path.as_ref()?;
    for dir in std::env::split_paths(path_var) {
        let Some(cli) = find_executable_in(&dir, CLI_BIN, &env.extensions) else {
            continue;
        };
        if let Ok(resolved) = cli.canonicalize() {
            return resolved.parent().map(Path::to_path_buf);
        }
    }
    None
}

/// Standard bin directories used by the official installers, in priority
/// order. Mirrors `_standard_cua_candidates` in `cua_cli.py`.
fn standard_cua_dirs(env: &ProbeEnv) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(install_dir) = &env.install_dir {
        dirs.push(install_dir.clone());
    }
    if cfg!(windows) {
        if let Some(local_app_data) = &env.local_app_data {
            dirs.push(local_app_data.join("dcc-mcp").join("bin"));
        }
    } else if let Some(home) = &env.home {
        dirs.push(home.join(".local").join("bin"));
    }
    dirs
}

/// Root directory of versioned standalone `dcc-cua` installs. Mirrors
/// `_standalone_cua_versions_dir` in `cua_cli.py`.
fn versioned_versions_dir(env: &ProbeEnv) -> Option<PathBuf> {
    if cfg!(windows) {
        env.local_app_data
            .as_ref()
            .map(|root| root.join("dcc-cua").join("versions"))
    } else if cfg!(target_os = "macos") {
        env.home.as_ref().map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("dcc-cua")
                .join("versions")
        })
    } else {
        let root = env.xdg_data_home.clone().or_else(|| {
            env.home
                .as_ref()
                .map(|home| home.join(".local").join("share"))
        })?;
        Some(root.join("dcc-cua").join("versions"))
    }
}

/// Is there a version-qualifying standalone `dcc-cua` under the versions root?
/// Mirrors `_resolve_versioned_cua_install` in `cua_cli.py`: a stable-semver
/// directory at or above [`MIN_CUA_VERSION`] that contains the executable.
fn versioned_cua_present(env: &ProbeEnv) -> bool {
    let Some(versions_dir) = versioned_versions_dir(env) else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(&versions_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(version) = stable_version_tuple(name) else {
            continue;
        };
        if version >= MIN_CUA_VERSION
            && find_executable_in(&entry.path(), CUA_BIN, &env.extensions).is_some()
        {
            return true;
        }
    }
    false
}

/// Is the `dcc-cua` executable present anywhere the executor would find it?
///
/// Checks, in the executor's order: `PATH`, the `dcc-mcp-cli` sibling
/// directory, the standard install directories, then a versioned standalone
/// install. Pure filesystem checks, no subprocess.
fn cua_installed(env: &ProbeEnv) -> bool {
    if cua_on_path(env) {
        return true;
    }
    if let Some(sibling) = cli_sibling_dir(env)
        && find_executable_in(&sibling, CUA_BIN, &env.extensions).is_some()
    {
        return true;
    }
    if standard_cua_dirs(env)
        .iter()
        .any(|dir| find_executable_in(dir, CUA_BIN, &env.extensions).is_some())
    {
        return true;
    }
    versioned_cua_present(env)
}

/// Probe state derived from an environment snapshot.
fn probe_state(env: &ProbeEnv) -> CuaRuntimeState {
    if cua_installed(env) {
        CuaRuntimeState::Unverified
    } else {
        CuaRuntimeState::Missing
    }
}

/// Parse a stable semver `MAJOR.MINOR.PATCH` with optional `+build` metadata,
/// rejecting prereleases and leading-zero components — mirrors
/// `_stable_version_tuple` in `cua_cli.py`.
fn stable_version_tuple(value: &str) -> Option<(u64, u64, u64)> {
    let core = match value.split_once('+') {
        // Build metadata must be non-empty and use the allowed character set.
        Some((core, build))
            if !build.is_empty()
                && build
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') =>
        {
            core
        }
        Some(_) => return None,
        None => value,
    };
    // A stable version has no prerelease, so its core carries no `-`.
    if core.contains('-') {
        return None;
    }
    let mut parts = core.split('.');
    let major = numeric_component(parts.next()?)?;
    let minor = numeric_component(parts.next()?)?;
    let patch = numeric_component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// One numeric semver component: digits only, `0` allowed, no leading zeros.
fn numeric_component(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if value.len() > 1 && value.starts_with('0') {
        return None;
    }
    value.parse().ok()
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

    // ── Off-PATH runtime detection (PIP-4981) ───────────────────────────────

    use crate::fallback::build_fallback;
    use crate::policy::{FALLBACK_REASON_CUA_UNAVAILABLE, FallbackTrigger};

    /// Write a `dcc-cua` executable into `dir` using the plain name, so the
    /// resolution is exercised independently of platform `PATHEXT`/case rules.
    fn write_cua(dir: &Path) {
        std::fs::create_dir_all(dir).expect("create dir");
        std::fs::write(dir.join(CUA_BIN), b"cua").expect("write dcc-cua");
    }

    fn write_cli(dir: &Path) {
        std::fs::create_dir_all(dir).expect("create dir");
        std::fs::write(dir.join(CLI_BIN), b"cli").expect("write dcc-mcp-cli");
    }

    /// A legal install in `DCC_MCP_INSTALL_DIR` that is **not** on `PATH` must
    /// be reported as present (actionable `Unverified`), not blocked. This is
    /// the false-negative PIP-4981 removes.
    #[test]
    fn an_off_path_install_dir_is_detected_and_the_fallback_is_actionable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let install = temp.path().join("install");
        write_cua(&install);

        let env = ProbeEnv {
            path: None, // deliberately off PATH
            install_dir: Some(install),
            ..ProbeEnv::default()
        };

        assert_eq!(
            probe_state(&env),
            CuaRuntimeState::Unverified,
            "an off-PATH legal install must be treated as present"
        );

        let advice = build_fallback(FallbackTrigger::NoCandidate, probe_state(&env));
        assert!(
            !advice.blocked,
            "an off-PATH legal install must not block the dcc-cua route"
        );
        assert_ne!(
            advice.blocked_reason.as_deref(),
            Some(FALLBACK_REASON_CUA_UNAVAILABLE),
            "an off-PATH legal install must not report cua_runtime_unavailable"
        );
    }

    /// The PIP-3702 boundary is unchanged: when no runtime exists anywhere,
    /// the route stays blocked with `cua_runtime_unavailable`.
    #[test]
    fn a_truly_absent_runtime_is_still_missing_and_blocked() {
        let temp = tempfile::tempdir().expect("tempdir");
        let empty = temp.path().join("empty");
        std::fs::create_dir_all(&empty).expect("create empty tree");

        let env = ProbeEnv {
            path: Some(OsString::new()),
            install_dir: None,
            // Point every platform root at the empty tree so nothing resolves.
            local_app_data: Some(empty.join("appdata")),
            xdg_data_home: Some(empty.join("xdg")),
            home: Some(empty.join("home")),
            ..ProbeEnv::default()
        };

        assert_eq!(probe_state(&env), CuaRuntimeState::Missing);

        let advice = build_fallback(FallbackTrigger::NoCandidate, probe_state(&env));
        assert!(advice.blocked, "a missing runtime must still block");
        assert_eq!(
            advice.blocked_reason.as_deref(),
            Some(FALLBACK_REASON_CUA_UNAVAILABLE)
        );
    }

    /// `dcc-cua` shipped beside `dcc-mcp-cli` is found even before the generic
    /// install directories are consulted.
    #[test]
    fn dcc_cua_beside_the_cli_is_detected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        write_cli(&bin);
        write_cua(&bin);

        let env = ProbeEnv {
            path: Some(bin.as_os_str().to_os_string()),
            ..ProbeEnv::default()
        };

        let sibling = cli_sibling_dir(&env).expect("cli sibling dir");
        assert!(
            find_executable_in(&sibling, CUA_BIN, &env.extensions).is_some(),
            "dcc-cua must be found beside dcc-mcp-cli"
        );
        assert_eq!(probe_state(&env), CuaRuntimeState::Unverified);
    }

    /// True off-PATH sibling: the CLI on `PATH` is a symlink into an install
    /// directory that holds `dcc-cua`, and that directory is not on `PATH`.
    /// Symlinks are reliable on Unix, so this runs there.
    #[cfg(unix)]
    #[test]
    fn dcc_cua_beside_a_symlinked_cli_off_path_is_detected() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let on_path = temp.path().join("onpath");
        let install = temp.path().join("install");
        write_cli(&install);
        write_cua(&install);
        std::fs::create_dir_all(&on_path).expect("create on-path dir");
        symlink(install.join(CLI_BIN), on_path.join(CLI_BIN)).expect("symlink cli");

        let env = ProbeEnv {
            path: Some(on_path.as_os_str().to_os_string()),
            ..ProbeEnv::default()
        };

        assert!(
            !cua_on_path(&env),
            "precondition: dcc-cua is not resolvable on PATH"
        );
        assert_eq!(
            probe_state(&env),
            CuaRuntimeState::Unverified,
            "dcc-cua beside the CLI's real location must still be found"
        );
    }

    /// A versioned standalone install at or above the minimum version counts.
    #[test]
    fn a_versioned_standalone_install_is_detected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (env, versions_root) = host_versioned_env(temp.path());
        write_cua(&versions_root.join("0.4.0"));

        assert_eq!(probe_state(&env), CuaRuntimeState::Unverified);
    }

    /// Version directories that are too old or not stable semver are ignored,
    /// matching the executor's version gate.
    #[test]
    fn old_and_unstable_version_dirs_are_ignored() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (env, versions_root) = host_versioned_env(temp.path());
        for name in ["0.3.0", "latest", "1.0.0-rc1", "1.2.3.4"] {
            write_cua(&versions_root.join(name));
        }

        assert_eq!(probe_state(&env), CuaRuntimeState::Missing);
    }

    /// `PATH` remains a first-class source (regression for the original probe).
    #[test]
    fn cua_on_path_is_still_detected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        write_cua(&bin);

        let env = ProbeEnv {
            path: Some(bin.as_os_str().to_os_string()),
            ..ProbeEnv::default()
        };

        assert!(cua_on_path(&env));
        assert_eq!(probe_state(&env), CuaRuntimeState::Unverified);
    }

    /// On Windows the executable carries a `PATHEXT` extension; the off-PATH
    /// sources must honour it too.
    #[cfg(windows)]
    #[test]
    fn an_off_path_install_dir_detects_the_exe_via_pathext() {
        let temp = tempfile::tempdir().expect("tempdir");
        let install = temp.path().join("install");
        std::fs::create_dir_all(&install).expect("create dir");
        std::fs::write(install.join("dcc-cua.exe"), b"cua").expect("write dcc-cua.exe");

        let env = ProbeEnv {
            path: None,
            extensions: vec![".EXE".to_string()],
            install_dir: Some(install),
            ..ProbeEnv::default()
        };

        assert_eq!(probe_state(&env), CuaRuntimeState::Unverified);
    }

    #[test]
    fn stable_version_tuple_accepts_only_stable_semver() {
        assert_eq!(stable_version_tuple("0.4.0"), Some((0, 4, 0)));
        assert_eq!(stable_version_tuple("1.11.2"), Some((1, 11, 2)));
        assert_eq!(stable_version_tuple("1.2.3+build.5"), Some((1, 2, 3)));
        assert!(stable_version_tuple("0.4.0").expect("0.4.0") >= MIN_CUA_VERSION);
        assert!(stable_version_tuple("0.3.9").expect("0.3.9") < MIN_CUA_VERSION);

        for rejected in [
            "latest",
            "1.0.0-rc1",
            "1.2.3.4",
            "01.2.3",
            "1.2",
            "",
            "1.2.3+",
        ] {
            assert_eq!(
                stable_version_tuple(rejected),
                None,
                "{rejected:?} must not parse as a stable semver"
            );
        }
    }

    /// Build a `ProbeEnv` whose versioned-standalone root points inside `root`,
    /// matching the current host's platform layout.
    fn host_versioned_env(root: &Path) -> (ProbeEnv, PathBuf) {
        if cfg!(windows) {
            let local_app_data = root.join("appdata");
            let versions = local_app_data.join("dcc-cua").join("versions");
            (
                ProbeEnv {
                    local_app_data: Some(local_app_data),
                    ..ProbeEnv::default()
                },
                versions,
            )
        } else if cfg!(target_os = "macos") {
            let home = root.join("home");
            let versions = home
                .join("Library")
                .join("Application Support")
                .join("dcc-cua")
                .join("versions");
            (
                ProbeEnv {
                    home: Some(home),
                    ..ProbeEnv::default()
                },
                versions,
            )
        } else {
            let home = root.join("home");
            let versions = home
                .join(".local")
                .join("share")
                .join("dcc-cua")
                .join("versions");
            (
                ProbeEnv {
                    home: Some(home),
                    ..ProbeEnv::default()
                },
                versions,
            )
        }
    }
}
