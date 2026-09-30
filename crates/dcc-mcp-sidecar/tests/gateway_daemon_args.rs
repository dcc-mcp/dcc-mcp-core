//! Regression tests for issue dcc-mcp-core#2642.
//!
//! `dcc-mcp-server gateway` inherits the host environment, and
//! `docs/guide/gateway.md` documents `DCC_MCP_GATEWAY_PERSIST=1`. clap's
//! default `bool` parser only accepts the literals `true`/`false`, so the
//! documented value used to abort startup with
//! `error: invalid value '1' for '--gateway-persist'`. The env-backed
//! boolean flags now go through clap's boolish value parser.

use std::sync::Mutex;

use clap::Parser as _;
use dcc_mcp_sidecar::gateway_daemon::{GatewayArgs, build_gateway_config};

/// Serialises the tests that mutate process environment variables: they run
/// in parallel threads inside one test binary.
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(clap::Parser)]
struct GatewayCli {
    #[command(flatten)]
    args: GatewayArgs,
}

fn parse(argv: &[&str]) -> Result<GatewayArgs, clap::Error> {
    GatewayCli::try_parse_from(argv).map(|cli| cli.args)
}

/// Set `key` for the duration of the closure and restore the previous value
/// afterwards. Follows the scoped-env convention used elsewhere in the
/// workspace.
fn with_env<T>(key: &str, value: &str, f: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let previous = std::env::var_os(key);
    // SAFETY: guarded by `ENV_LOCK`; no other test in this binary reads or
    // writes these variables while the closure runs.
    unsafe {
        std::env::set_var(key, value);
    }
    let result = f();
    // SAFETY: same critical section as above.
    unsafe {
        match previous {
            Some(previous) => std::env::set_var(key, previous),
            None => std::env::remove_var(key),
        }
    }
    result
}

#[test]
fn gateway_persist_accepts_boolish_values() {
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
    let args = with_env("DCC_MCP_GATEWAY_PERSIST", "1", || {
        parse(&["gateway"]).expect("DCC_MCP_GATEWAY_PERSIST=1 must not abort startup")
    });
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
    for key in [
        "DCC_MCP_NO_ADMIN",
        "DCC_MCP_DAEMON",
        "DCC_MCP_SEMANTIC_SEARCH_ENABLED",
    ] {
        let args = with_env(key, "1", || {
            parse(&["gateway"])
                .unwrap_or_else(|err| panic!("{key}=1 must not abort startup: {err}"))
        });
        let enabled = match key {
            "DCC_MCP_NO_ADMIN" => args.no_admin,
            "DCC_MCP_DAEMON" => args.daemon,
            "DCC_MCP_SEMANTIC_SEARCH_ENABLED" => args.semantic_search_enabled,
            other => unreachable!("unhandled env flag {other}"),
        };
        assert!(enabled, "{key}=1 must enable the flag");
    }
}
