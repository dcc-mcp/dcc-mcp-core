//! Regression tests for issue dcc-mcp-core#2642.
//!
//! `dcc-mcp-server gateway` inherits the host environment, and
//! `docs/guide/gateway.md` documents `DCC_MCP_GATEWAY_PERSIST=1`. clap's
//! default `bool` parser only accepts the literals `true`/`false`, so the
//! documented value used to abort startup with
//! `error: invalid value '1' for '--gateway-persist'`. The env-backed
//! boolean flags now go through clap's boolish value parser.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

use clap::Parser as _;
use dcc_mcp_sidecar::gateway_daemon::{GatewayArgs, build_gateway_config};

/// The env-backed boolean flags of [`GatewayArgs`]. Every `parse` and
/// `build_gateway_config` call in this binary reads them, and the tests run
/// in parallel threads, so they are only touched while [`env_lock`] is held.
const BOOL_ENV_KEYS: [&str; 4] = [
    "DCC_MCP_GATEWAY_PERSIST",
    "DCC_MCP_NO_ADMIN",
    "DCC_MCP_DAEMON",
    "DCC_MCP_SEMANTIC_SEARCH_ENABLED",
];

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Held for the whole duration of a test: clap reads the environment inside
/// `parse` and `build_gateway_config` reads it again, so the lock has to cover
/// every call, not just the mutations.
fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner())
}

/// Removes every env-backed boolean flag for the duration of a test so an
/// inherited value cannot change a default-value assertion, and restores the
/// original values on drop. Callers must hold [`env_lock`].
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
struct GatewayCli {
    #[command(flatten)]
    args: GatewayArgs,
}

fn parse(argv: &[&str]) -> Result<GatewayArgs, clap::Error> {
    GatewayCli::try_parse_from(argv).map(|cli| cli.args)
}

#[test]
fn gateway_persist_accepts_boolish_values() {
    let _lock = env_lock();
    let _env = ScopedBoolEnv::cleared();

    for value in ["1", "true", "TRUE", "yes", "on"] {
        let args = parse(&["gateway", &format!("--gateway-persist={value}")])
            .unwrap_or_else(|err| panic!("--gateway-persist={value} must parse: {err}"));
        assert!(
            args.gateway_persist,
            "--gateway-persist={value} must enable persist mode"
        );
    }
    for value in ["0", "false", "FALSE", "no", "off"] {
        let args = parse(&["gateway", &format!("--gateway-persist={value}")])
            .unwrap_or_else(|err| panic!("--gateway-persist={value} must parse: {err}"));
        assert!(
            !args.gateway_persist,
            "--gateway-persist={value} must leave persist mode off"
        );
    }
}

#[test]
fn gateway_persist_flag_without_value_stays_enabled() {
    let _lock = env_lock();
    let _env = ScopedBoolEnv::cleared();

    let args = parse(&["gateway", "--gateway-persist"]).expect("bare --gateway-persist must parse");
    assert!(args.gateway_persist);

    // A bare flag must not swallow the flag that follows it.
    let args = parse(&["gateway", "--gateway-persist", "--daemon"])
        .expect("bare --gateway-persist followed by --daemon must parse");
    assert!(args.gateway_persist);
    assert!(args.daemon);

    let args = parse(&["gateway"]).expect("no flag must parse");
    assert!(!args.gateway_persist);
}

#[test]
fn gateway_persist_env_accepts_one() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();
    env.set("DCC_MCP_GATEWAY_PERSIST", "1");

    let args = parse(&["gateway"]).expect("DCC_MCP_GATEWAY_PERSIST=1 must not abort startup");
    assert!(
        args.gateway_persist,
        "DCC_MCP_GATEWAY_PERSIST=1 must enable persist mode"
    );

    let cfg = build_gateway_config(&args, "persist-test");
    assert!(
        cfg.gateway_persist,
        "the daemon config must inherit persist mode from DCC_MCP_GATEWAY_PERSIST=1"
    );
}

#[test]
fn sibling_env_backed_flags_accept_one() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();

    for key in [
        "DCC_MCP_NO_ADMIN",
        "DCC_MCP_DAEMON",
        "DCC_MCP_SEMANTIC_SEARCH_ENABLED",
    ] {
        env.set(key, "1");
        let args = parse(&["gateway"])
            .unwrap_or_else(|err| panic!("{key}=1 must not abort startup: {err}"));
        let enabled = match key {
            "DCC_MCP_NO_ADMIN" => args.no_admin,
            "DCC_MCP_DAEMON" => args.daemon,
            "DCC_MCP_SEMANTIC_SEARCH_ENABLED" => args.semantic_search_enabled,
            other => unreachable!("unhandled env flag {other}"),
        };
        assert!(enabled, "{key}=1 must enable the flag");
    }
}
