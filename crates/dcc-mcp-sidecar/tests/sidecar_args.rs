//! Regression tests for the `sidecar` subcommand's env-backed boolean flags.
//!
//! `--legacy-gateway-election` reads `DCC_MCP_LEGACY_GATEWAY_ELECTION` but
//! kept clap's default `bool` parser, which accepts only the literals
//! `true`/`false`. Operators write `=1` for every sibling variable
//! (`DCC_MCP_GATEWAY_PERSIST`, `DCC_MCP_NO_ADMIN`, ...), and that value used
//! to abort startup with `error: invalid value '1' for
//! '--legacy-gateway-election'` (issue dcc-mcp-core#2642). The flag now uses
//! the same boolish parser as its siblings.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

use clap::Parser as _;
use dcc_mcp_sidecar::SidecarArgs;

/// Env-backed boolean flags of [`SidecarArgs`]. Touched only while
/// [`env_lock`] is held, because the tests in this binary run in parallel
/// threads and clap reads the environment during `parse`.
const BOOL_ENV_KEYS: [&str; 1] = ["DCC_MCP_LEGACY_GATEWAY_ELECTION"];

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner())
}

/// Clears every env-backed boolean flag for the duration of a test and
/// restores the original values on drop. Callers must hold [`env_lock`].
struct ScopedBoolEnv {
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl ScopedBoolEnv {
    fn cleared() -> Self {
        let mut previous = Vec::new();
        for key in BOOL_ENV_KEYS {
            previous.push((key, std::env::var_os(key)));
            // SAFETY: the caller holds `ENV_LOCK`, so no other test in this
            // binary reads or writes these variables concurrently.
            unsafe { std::env::remove_var(key) };
        }
        Self { previous }
    }

    fn set(&self, key: &str, value: &str) {
        debug_assert!(BOOL_ENV_KEYS.contains(&key));
        // SAFETY: same critical section as `cleared`.
        unsafe { std::env::set_var(key, value) };
    }
}

impl Drop for ScopedBoolEnv {
    fn drop(&mut self) {
        // SAFETY: same critical section as `cleared`.
        unsafe {
            for (key, previous) in self.previous.iter().rev() {
                match previous {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

#[derive(clap::Parser)]
struct SidecarCli {
    #[command(flatten)]
    args: SidecarArgs,
}

/// Minimal argv: `--dcc`, `--host-rpc` and `--watch-pid` are required.
const BASE: &[&str] = &["sidecar", "--dcc", "maya", "--host-rpc", "stub://localhost"];

fn parse(extra: &[&str]) -> Result<SidecarArgs, clap::Error> {
    let mut argv = BASE.to_vec();
    argv.extend_from_slice(extra);
    argv.push("--watch-pid");
    argv.push("4242");
    SidecarCli::try_parse_from(argv).map(|cli| cli.args)
}

/// The real `dcc-mcp-server` nests `SidecarArgs` under a `sidecar`
/// subcommand. Exercise that shape too: a boolish parser has to keep working
/// through the nested parser, and a flat-parser-only test would miss a
/// regression here.
fn parse_nested(extra: &[&str]) -> Result<SidecarArgs, clap::Error> {
    #[derive(clap::Parser)]
    enum Cmd {
        Sidecar(SidecarArgs),
    }
    #[derive(clap::Parser)]
    struct Top {
        #[command(subcommand)]
        command: Cmd,
    }

    let mut argv = vec!["dcc-mcp-server", "sidecar", "--dcc", "blender"];
    argv.extend_from_slice(&["--host-rpc", "ws://127.0.0.1:9000"]);
    argv.extend_from_slice(extra);
    argv.extend_from_slice(&["--watch-pid", "4242"]);

    Top::try_parse_from(argv).map(|top| match top.command {
        Cmd::Sidecar(args) => args,
    })
}

#[test]
fn legacy_gateway_election_env_accepts_one_through_a_nested_subcommand() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();
    env.set("DCC_MCP_LEGACY_GATEWAY_ELECTION", "1");

    let args = parse_nested(&[])
        .unwrap_or_else(|err| panic!("nested DCC_MCP_LEGACY_GATEWAY_ELECTION=1 must parse: {err}"));
    assert!(
        args.legacy_gateway_election,
        "the boolish parser must survive being nested under a `sidecar` subcommand"
    );
}

#[test]
fn legacy_gateway_election_env_accepts_boolish_values() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();

    for value in ["1", "true", "TRUE", "yes", "on"] {
        env.set("DCC_MCP_LEGACY_GATEWAY_ELECTION", value);
        let args = parse(&[]).unwrap_or_else(|err| {
            panic!("DCC_MCP_LEGACY_GATEWAY_ELECTION={value} must not abort startup: {err}")
        });
        assert!(
            args.legacy_gateway_election,
            "DCC_MCP_LEGACY_GATEWAY_ELECTION={value} must enable legacy election"
        );
    }

    for value in ["0", "false", "FALSE", "no", "off"] {
        env.set("DCC_MCP_LEGACY_GATEWAY_ELECTION", value);
        let args = parse(&[]).unwrap_or_else(|err| {
            panic!("DCC_MCP_LEGACY_GATEWAY_ELECTION={value} must not abort startup: {err}")
        });
        assert!(
            !args.legacy_gateway_election,
            "DCC_MCP_LEGACY_GATEWAY_ELECTION={value} must leave legacy election off"
        );
    }
}

#[test]
fn legacy_gateway_election_accepts_boolish_cli_values() {
    let _lock = env_lock();
    let _env = ScopedBoolEnv::cleared();

    for value in ["1", "true", "yes", "on"] {
        let args = parse(&[&format!("--legacy-gateway-election={value}")])
            .unwrap_or_else(|err| panic!("--legacy-gateway-election={value} must parse: {err}"));
        assert!(
            args.legacy_gateway_election,
            "--legacy-gateway-election={value} must enable legacy election"
        );
    }
    for value in ["0", "false", "no", "off"] {
        let args = parse(&[&format!("--legacy-gateway-election={value}")])
            .unwrap_or_else(|err| panic!("--legacy-gateway-election={value} must parse: {err}"));
        assert!(
            !args.legacy_gateway_election,
            "--legacy-gateway-election={value} must leave legacy election off"
        );
    }
}

#[test]
fn legacy_gateway_election_bare_flag_stays_enabled() {
    let _lock = env_lock();
    let _env = ScopedBoolEnv::cleared();

    let args =
        parse(&["--legacy-gateway-election"]).expect("a bare --legacy-gateway-election must parse");
    assert!(args.legacy_gateway_election);

    // A bare flag must not swallow the flag that follows it.
    let args = parse(&["--legacy-gateway-election", "--no-ensure-gateway"])
        .expect("a bare flag followed by --no-ensure-gateway must parse");
    assert!(args.legacy_gateway_election);
    assert!(args.no_ensure_gateway);

    let args = parse(&[]).expect("no flag must parse");
    assert!(!args.legacy_gateway_election);
}
