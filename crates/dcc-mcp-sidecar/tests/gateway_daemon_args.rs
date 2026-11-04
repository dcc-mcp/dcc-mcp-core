//! Regression tests for issue dcc-mcp-core#2642 and its follow-up.
//!
//! `dcc-mcp-server gateway` inherits the host environment, and
//! `docs/guide/gateway.md` documents `DCC_MCP_GATEWAY_PERSIST=1`. clap's
//! default `bool` parser only accepts the literals `true`/`false`, so the
//! documented value used to abort startup with
//! `error: invalid value '1' for '--gateway-persist'`. The env-backed
//! boolean flags now go through clap's boolish value parser.
//!
//! Boolish parsing only fixed the *parse* half. The follow-up covers
//! *precedence*: an explicit `--gateway-persist=false` used to be swallowed by
//! a `true` inherited from `DCC_MCP_GATEWAY_PERSIST`, leaving operators unable
//! to turn the flag off from the command line.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

use clap::Parser as _;
use dcc_mcp_sidecar::gateway_daemon::{GATEWAY_PERSIST_ID, GatewayArgs, build_gateway_config};

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

impl From<GatewayCli> for GatewayArgs {
    fn from(cli: GatewayCli) -> Self {
        cli.args
    }
}

fn parse(argv: &[&str]) -> Result<GatewayArgs, clap::Error> {
    GatewayCli::try_parse_from(argv).map(|cli| cli.args)
}

/// Parse `argv` the way a real entry point does, keeping the `ArgMatches`
/// value sources so `explicitly_supplied` is meaningful.
fn parse_annotated(argv: &[&str]) -> GatewayArgs {
    use clap::{CommandFactory as _, FromArgMatches as _};

    let matches = GatewayCli::command()
        .try_get_matches_from(argv)
        .expect("argv must parse");
    let cli = GatewayCli::from_arg_matches(&matches).expect("matches must convert");
    let mut args: GatewayArgs = cli.into();
    args.note_value_sources(&matches);
    args
}

/// Parse `argv` with `GatewayArgs` nested under a `gateway` subcommand, the
/// shape `dcc-mcp-server` actually uses, and annotate value sources.
///
/// This mirrors the caller in `dcc-mcp-server`: a subcommand is a nested
/// parser, so the flags' value sources live in the subcommand's `ArgMatches`.
/// A flat-parser test passes even when the nested wiring is broken, which is
/// exactly how the subcommand-name bug shipped.
fn parse_annotated_nested(argv_tail: &[&str]) -> GatewayArgs {
    use clap::{CommandFactory as _, FromArgMatches as _};

    #[derive(clap::Parser)]
    enum Cmd {
        Gateway(GatewayArgs),
    }
    #[derive(clap::Parser)]
    struct Top {
        #[command(subcommand)]
        command: Cmd,
    }

    let mut argv = vec!["dcc-mcp-server", "gateway"];
    argv.extend_from_slice(argv_tail);

    let matches = Top::command()
        .try_get_matches_from(argv)
        .expect("argv must parse");
    let top = Top::from_arg_matches(&matches).expect("matches must convert");

    let Cmd::Gateway(mut args) = top.command;
    let sub = matches
        .subcommand_matches("gateway")
        .expect("the gateway subcommand was on the command line");
    args.note_value_sources(sub);
    args
}

#[test]
fn explicit_cli_false_beats_env_true_through_a_nested_subcommand() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();
    env.set("DCC_MCP_GATEWAY_PERSIST", "1");

    // The real argv shape: `dcc-mcp-server gateway --gateway-persist=false`.
    let args = parse_annotated_nested(&["--gateway-persist=false", "--port", "19765"]);
    assert!(
        !args.effective_gateway_persist(),
        "an explicit --gateway-persist=false must win over the env var when the flags are nested under the `gateway` subcommand"
    );

    // And the env must still apply when the flag is absent.
    let args = parse_annotated_nested(&["--port", "19766"]);
    assert!(
        args.effective_gateway_persist(),
        "DCC_MCP_GATEWAY_PERSIST=1 must still apply when no flag was supplied"
    );
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

// ── Precedence: an explicit CLI value beats the environment ───────────────

#[test]
fn explicit_cli_false_beats_env_true() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();
    env.set("DCC_MCP_GATEWAY_PERSIST", "1");

    let args = parse_annotated(&["gateway", "--gateway-persist=false"]);
    assert!(
        args.explicitly_supplied(GATEWAY_PERSIST_ID),
        "an explicit --gateway-persist=false must be recorded as supplied"
    );
    assert!(
        !args.effective_gateway_persist(),
        "an explicit --gateway-persist=false must win over DCC_MCP_GATEWAY_PERSIST=1"
    );

    let cfg = build_gateway_config(&args, "persist-cli-wins");
    assert!(
        !cfg.gateway_persist,
        "the daemon config must honour an explicit --gateway-persist=false"
    );
}

#[test]
fn explicit_cli_true_beats_env_false() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();
    env.set("DCC_MCP_GATEWAY_PERSIST", "0");

    let args = parse_annotated(&["gateway", "--gateway-persist=true"]);
    assert!(
        args.effective_gateway_persist(),
        "an explicit --gateway-persist=true must win over DCC_MCP_GATEWAY_PERSIST=0"
    );
}

#[test]
fn env_still_applies_without_an_explicit_cli_flag() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();

    // env only
    env.set("DCC_MCP_GATEWAY_PERSIST", "1");
    let args = parse_annotated(&["gateway"]);
    assert!(
        !args.explicitly_supplied(GATEWAY_PERSIST_ID),
        "no flag on the command line must not be recorded as supplied"
    );
    assert!(
        args.effective_gateway_persist(),
        "env alone must still work"
    );

    // CLI only
    env.set("DCC_MCP_GATEWAY_PERSIST", "");
    let args = parse_annotated(&["gateway", "--gateway-persist"]);
    assert!(
        args.effective_gateway_persist(),
        "CLI alone must still work"
    );

    // neither
    let env2 = ScopedBoolEnv::cleared();
    let args = parse_annotated(&["gateway"]);
    assert!(
        !args.effective_gateway_persist(),
        "no flag and no env must stay off"
    );
    drop(env2);
}

#[test]
fn semantic_search_shares_the_explicit_cli_precedence() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();
    env.set("DCC_MCP_SEMANTIC_SEARCH_ENABLED", "1");

    let args = parse_annotated(&["gateway", "--semantic-search-enabled=false"]);
    assert!(
        !args.effective_semantic_search_enabled(),
        "an explicit --semantic-search-enabled=false must win over the env var"
    );

    let cfg = build_gateway_config(&args, "semantic-cli-wins");
    assert!(!cfg.semantic_search_enabled);
}

/// The env path must accept exactly what the boolish CLI parser accepts —
/// no more, no less. A numeric fallback like "any non-zero integer is true"
/// would let `DCC_MCP_GATEWAY_PERSIST=2` read as `true` while
/// `--gateway-persist=2` is rejected by clap, which is the regression this
/// pins.
#[test]
fn env_accepts_only_the_boolish_spellings_the_cli_accepts() {
    let _lock = env_lock();
    let env = ScopedBoolEnv::cleared();

    // Every spelling clap's boolish parser accepts must also work via env.
    for value in [
        "1", "true", "TRUE", "True", "yes", "YES", "Yes", "on", "ON", "On", "y", "Y", "t", "T",
    ] {
        env.set("DCC_MCP_GATEWAY_PERSIST", value);
        let args = parse_annotated(&["gateway"]);
        assert!(
            args.effective_gateway_persist(),
            "DCC_MCP_GATEWAY_PERSIST={value} must enable persist mode"
        );
        assert!(
            parse(&["gateway", &format!("--gateway-persist={value}")]).is_ok(),
            "--gateway-persist={value} is expected to be accepted by clap"
        );
    }

    for value in [
        "0", "false", "FALSE", "False", "no", "NO", "No", "off", "OFF", "Off", "n", "N", "f", "F",
    ] {
        env.set("DCC_MCP_GATEWAY_PERSIST", value);
        let args = parse_annotated(&["gateway"]);
        assert!(
            !args.effective_gateway_persist(),
            "DCC_MCP_GATEWAY_PERSIST={value} must leave persist mode off"
        );
    }

    // Values clap's boolish parser rejects on the command line must not
    // become `true` through the environment. `2` is the important one: an
    // earlier numeric fallback read it as `true`, the opposite of the CLI.
    //
    // The two arms are checked separately on purpose. clap validates the env
    // var during `parse`, so an invalid value makes the *argv* parse fail
    // before `effective_gateway_persist()` is ever reached; conflating them
    // would test clap's validation, not our helper.
    for value in [
        "2", "-1", "42", "maybe", "enabled", "1.0", "01", " 1", "1 ", " 1x",
    ] {
        // Arm 1: the CLI rejects the value.
        env.set("DCC_MCP_GATEWAY_PERSIST", "");
        assert!(
            parse(&["gateway", &format!("--gateway-persist={value}")]).is_err(),
            "--gateway-persist={value:?} is expected to be rejected by clap; if it is accepted \
             this test's premise is stale and the pair must be re-checked"
        );

        // Arm 2: given that rejection, the env path must not read it as true.
        // clap sees the invalid env var during parsing, so the effective value
        // is taken straight from the (absent) CLI flag: false either way.
        env.set("DCC_MCP_GATEWAY_PERSIST", value);
        assert!(
            parse(&["gateway"]).is_err(),
            "DCC_MCP_GATEWAY_PERSIST={value:?} must be rejected by clap, never enabled"
        );

        // Arm 3: the helper's own verdict, independent of clap's parse. The
        // env var is the only input here, so this is the arm that actually
        // pins `env_flag_enabled`.
        env.set("DCC_MCP_GATEWAY_PERSIST", value);
        assert!(
            !GatewayArgs::default().effective_gateway_persist(),
            "env_flag_enabled must read DCC_MCP_GATEWAY_PERSIST={value:?} as false, \
             matching the CLI's rejection of --gateway-persist={value:?}"
        );

        // Sanity: the same helper still resolves a valid value to true, so
        // arm 3 is not simply asserting `false` for every input.
        env.set("DCC_MCP_GATEWAY_PERSIST", "1");
        assert!(
            GatewayArgs::default().effective_gateway_persist(),
            "env_flag_enabled must still read DCC_MCP_GATEWAY_PERSIST=1 as true"
        );
    }
}

/// `GatewayArgs::default()` is written out literally (so it cannot panic on a
/// malformed env var), which means it can drift from the `#[arg(default_value
/// = ...)]` attributes. Parse an empty argv and compare, so drift fails here
/// rather than silently changing behaviour for embedders.
#[test]
fn gateway_defaults_match_clap() {
    let _lock = env_lock();
    const CASE_ENV: &str = "DCC_MCP_TEST_GATEWAY_DEFAULTS";
    if std::env::var_os(CASE_ENV).is_none() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "gateway_defaults_match_clap", "--nocapture"]);
        // Non-boolean env-backed fields must also use their declared defaults.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("DCC_MCP_") {
                command.env_remove(key);
            }
        }
        let output = command.env(CASE_ENV, "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("gateway defaults verified"));
        return;
    }
    // Clear the env-backed keys understood by this test binary so the parsed
    // result reflects clap's declared defaults, not the host environment.
    let _env = ScopedBoolEnv::cleared();

    let parsed = parse(&["gateway"]).expect("an empty gateway argv must parse");
    let defaults = GatewayArgs::default();

    assert_eq!(defaults.host, parsed.host, "default host drifted from clap");
    assert_eq!(defaults.port, parsed.port, "default port drifted from clap");
    assert_eq!(
        defaults.remote_host, parsed.remote_host,
        "default remote host drifted from clap"
    );
    assert_eq!(
        defaults.remote_port, parsed.remote_port,
        "default remote port drifted from clap"
    );
    assert_eq!(
        defaults.admin_path, parsed.admin_path,
        "default admin path drifted from clap"
    );
    assert_eq!(
        defaults.stale_timeout_secs, parsed.stale_timeout_secs,
        "default stale timeout drifted from clap"
    );
    assert_eq!(
        defaults.gateway_persist, parsed.gateway_persist,
        "default gateway persist drifted from clap"
    );
    assert_eq!(
        defaults.semantic_search_enabled, parsed.semantic_search_enabled,
        "default semantic search drifted from clap"
    );
    assert_eq!(
        defaults.no_admin, parsed.no_admin,
        "default no-admin drifted from clap"
    );
    assert_eq!(
        defaults.daemon, parsed.daemon,
        "default daemon drifted from clap"
    );
    assert_eq!(
        defaults.restart, parsed.restart,
        "default restart drifted from clap"
    );
    assert_eq!(
        defaults.gateway_idle_timeout_secs, parsed.gateway_idle_timeout_secs,
        "default idle timeout drifted from clap"
    );

    // The bookkeeping fields must start out as "not supplied".
    assert!(!defaults.explicitly_supplied(GATEWAY_PERSIST_ID));
    assert!(defaults.relay_sources.is_empty());
    assert!(defaults.pidfile.is_none());
    assert!(defaults.name.is_none());
    assert!(defaults.registry_dir.is_none());
    println!("gateway defaults verified");
}

#[test]
fn default_gateway_args_ignore_a_malformed_port_env_var() {
    let _lock = env_lock();
    let _env = ScopedBoolEnv::cleared();
    // `port` is a `u16` reading `DCC_MCP_GATEWAY_PORT`, so a malformed value
    // is exactly the shape that would make an env-reading `Default::default()`
    // panic. Rust callers assume `Default` cannot panic.
    let previous = std::env::var_os("DCC_MCP_GATEWAY_PORT");
    // SAFETY: the caller holds `ENV_LOCK`.
    unsafe { std::env::set_var("DCC_MCP_GATEWAY_PORT", "not-a-port") };

    let args = GatewayArgs::default();
    assert_eq!(args.port, 9765, "the default port must not come from env");

    // SAFETY: same critical section.
    unsafe {
        match previous {
            Some(value) => std::env::set_var("DCC_MCP_GATEWAY_PORT", value),
            None => std::env::remove_var("DCC_MCP_GATEWAY_PORT"),
        }
    }
}

#[test]
fn unknown_flag_id_is_not_explicit() {
    let _lock = env_lock();
    let _env = ScopedBoolEnv::cleared();

    let args = parse_annotated(&["gateway", "--gateway-persist"]);
    assert!(
        !args.explicitly_supplied("does-not-exist"),
        "unknown ids must read as not-supplied so the derived value still applies"
    );
}
