//! Machine-wide standalone gateway daemon and auto-launch helper.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::Duration;

use clap::Args;
use dcc_mcp_gateway::{
    AdminPersistConfig, GatewayConfig, GatewayRunner, RelaySourceConfig, STANDALONE_GATEWAY_DCC,
};

const DAEMONIZED_ENV: &str = "DCC_MCP__DAEMONIZED";

/// Idle-timeout default when a gateway is auto-launched on behalf of another
/// process (Rust `ensure_gateway_running`, `dcc-mcp-cli` ensure, Python
/// guardian).
///
/// Defined here rather than inside `launcher` so consumers that enable only
/// `gateway-daemon` (like `dcc-mcp-cli`) can still reference it — the module
/// gate is on `gateway-auto`, but this constant is a value, not behaviour.
/// See [`dcc_mcp_gateway::gateway::idle_timeout::AUTO_ENSURE_DEFAULT`].
pub const AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_SECS: u64 =
    dcc_mcp_gateway::gateway::idle_timeout::AUTO_ENSURE_DEFAULT;

#[cfg(feature = "gateway-auto")]
mod guardian;
#[cfg(feature = "gateway-auto")]
mod launcher;

#[cfg(feature = "gateway-auto")]
pub use guardian::{
    GatewayGuardianHandle, GatewayGuardianSettings, GatewayGuardianStatus, spawn_gateway_guardian,
};
#[cfg(feature = "gateway-auto")]
pub use launcher::{EnsureGatewayOptions, ensure_gateway_running};

/// CLI parser for one relay discovery source.
#[derive(Debug, Clone)]
pub struct RelaySourceArg(pub RelaySourceConfig);

impl FromStr for RelaySourceArg {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (admin_url, public_base_url) = value.split_once('=').ok_or_else(|| {
            "expected ADMIN_URL=PUBLIC_BASE_URL, for example http://127.0.0.1:9872=http://127.0.0.1:9873"
                .to_string()
        })?;
        let admin_url = admin_url.trim();
        let public_base_url = public_base_url.trim();
        if admin_url.is_empty() || public_base_url.is_empty() {
            return Err("relay source admin and public URLs must be non-empty".into());
        }
        Ok(Self(RelaySourceConfig {
            admin_url: admin_url.to_string(),
            public_base_url: public_base_url.to_string(),
            poll_interval_secs: None,
        }))
    }
}

/// Clap argument ids of the env-backed boolean flags.
///
/// These are the derive-generated ids (snake_case field names), not the
/// `--kebab-case` long flags: `ArgMatches::value_source` is keyed by id, and
/// passing a long flag there panics.
pub const GATEWAY_PERSIST_ID: &str = "gateway_persist";
pub const SEMANTIC_SEARCH_ENABLED_ID: &str = "semantic_search_enabled";
pub const NO_ADMIN_ID: &str = "no_admin";

/// Environment variables backing the boolean flags above.
pub const GATEWAY_PERSIST_ENV: &str = "DCC_MCP_GATEWAY_PERSIST";
pub const SEMANTIC_SEARCH_ENABLED_ENV: &str = "DCC_MCP_SEMANTIC_SEARCH_ENABLED";
pub const NO_ADMIN_ENV: &str = "DCC_MCP_NO_ADMIN";

/// True when `matches` shows this flag was supplied on the command line.
///
/// `ValueSource::CommandLine` is the only source that means "the operator
/// typed it": `EnvVariable` and `DefaultValue` both mean the value was
/// inferred, and only an inferred value may be overridden by precedence.
///
/// Guards the [`clap::ArgMatches::value_source`] call with `try_contains_id`,
/// because `value_source` panics on an id this `ArgMatches` does not know.
/// That is not hypothetical: `GatewayArgs` is flattened into the `gateway`
/// subcommand by `dcc-mcp-server`, so the ids live in the subcommand's
/// matches, and a caller holding a different `ArgMatches` (or one from
/// another subcommand) must get `false` rather than a panic.
fn explicit_on_command_line(matches: &clap::ArgMatches, id: &str) -> bool {
    matches.try_contains_id(id).unwrap_or(false)
        && matches.value_source(id) == Some(clap::parser::ValueSource::CommandLine)
}

/// Read a `1`/`0`/`true`/`false`-style flag from the environment.
///
/// Mirrors the accepted inputs of clap's boolish parser *exactly*, so a value
/// the CLI would have rejected cannot sneak in through the environment and
/// change the effective configuration. Two consequences worth stating:
///
/// * No numeric fallback. `--gateway-persist=2` is rejected by clap, so
///   `DCC_MCP_GATEWAY_PERSIST=2` reads as `false` here rather than as "any
///   non-zero integer is true".
/// * No trimming. clap's boolish parser rejects `" 1"`, so an untrimmed env
///   value with stray whitespace reads as `false` too.
///
/// The accepted set is `1`/`0`, `true`/`false`, `yes`/`no`, `on`/`off`, and
/// the single letters `y`/`n`/`t`/`f`, each case-insensitively.
fn env_flag_enabled(key: &str) -> bool {
    matches!(
        std::env::var(key)
            .ok()
            .map(|raw| raw.to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on") | Some("y") | Some("t")
    )
}

/// CLI surface for the machine-wide gateway process.
///
/// The env-backed boolean flags use clap's boolish value parser: clap's
/// default `bool` parser only accepts the literals `true`/`false`, so a
/// documented setting such as `DCC_MCP_GATEWAY_PERSIST=1` used to abort
/// startup with `invalid value '1'` (issue dcc-mcp-core#2642). The boolish
/// parser accepts `1`/`0`, `true`/`false`, `yes`/`no` and `on`/`off`, which
/// is what `build_gateway_config` and `docs/guide/gateway.md` already assume.
///
/// The boolish parser fixes *parsing*, not *precedence*. An explicit
/// `--gateway-persist=false` must still beat `DCC_MCP_GATEWAY_PERSIST=1`,
/// which is why [`GatewayArgs::note_value_sources`] records the
/// `ArgMatches` value source and the effective-value helpers below consult it
/// instead of OR-ing the parsed flag with the raw environment.
#[derive(Debug, Args, Clone)]
pub struct GatewayArgs {
    /// Gateway host/interface to bind.
    #[arg(long, env = "DCC_MCP_GATEWAY_HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// Well-known gateway port.
    #[arg(long, env = "DCC_MCP_GATEWAY_PORT", default_value = "9765")]
    pub port: u16,

    /// Human-readable gateway owner label.
    #[arg(long, env = "DCC_MCP_GATEWAY_NAME")]
    pub name: Option<String>,

    /// Secondary gateway interface. Use 0.0.0.0 or a LAN IP to opt into LAN access.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_HOST", default_value = "127.0.0.1")]
    pub remote_host: String,

    /// Remote/LAN gateway port. 0 disables the remote listener.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_PORT", default_value = "59765")]
    pub remote_port: u16,

    /// Directory for the shared FileRegistry.
    #[arg(long, env = "DCC_MCP_REGISTRY_DIR")]
    pub registry_dir: Option<PathBuf>,

    /// Disable the Admin UI.
    #[arg(
        long,
        env = "DCC_MCP_NO_ADMIN",
        default_value = "false",
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub no_admin: bool,

    /// URL prefix for the Admin UI.
    #[arg(long, env = "DCC_MCP_ADMIN_PATH", default_value = "/admin")]
    pub admin_path: String,

    /// Seconds without a heartbeat before an instance is considered stale.
    #[arg(long, env = "DCC_MCP_STALE_TIMEOUT", default_value = "30")]
    pub stale_timeout_secs: u64,

    /// Discover LAN-local DCC MCP endpoints via mDNS/DNS-SD.
    #[cfg(feature = "mdns")]
    #[arg(
        long,
        env = "DCC_MCP_DISCOVER_MDNS",
        default_value = "false",
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub discover_mdns: bool,

    /// Tunnel relay discovery source, as `ADMIN_URL=PUBLIC_BASE_URL`.
    ///
    /// Repeat the flag or use comma-separated `DCC_MCP_RELAY_SOURCES` values.
    #[arg(
        long = "relay-source",
        env = "DCC_MCP_RELAY_SOURCES",
        value_delimiter = ',',
        value_name = "ADMIN_URL=PUBLIC_BASE_URL"
    )]
    pub relay_sources: Vec<RelaySourceArg>,

    /// Keep the gateway daemon alive even when no backends remain.
    /// Default: false. Use for studio/headless deployments.
    #[arg(
        long,
        env = "DCC_MCP_GATEWAY_PERSIST",
        default_value = "false",
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub gateway_persist: bool,

    /// Seconds to wait after the last backend exits before shutting down
    /// the gateway daemon. `0` disables idle timeout (same as `--gateway-persist`).
    /// Default: 30.
    ///
    /// `Option` distinguishes "the operator said nothing" from "the operator
    /// explicitly asked for the default value" — the previous `u64` forced a
    /// value comparison (`!= 30`) as an explicit-argument sentinel, which
    /// silently dropped an explicit `--gateway-idle-timeout-secs 30`.
    ///
    /// Deliberately no `default_value`: a clap default would populate
    /// `Some(30)` and make "supplied" indistinguishable from "not supplied"
    /// again. The default is applied by [`GatewayArgs::gateway_idle_timeout_secs`]
    /// instead, so `None` really does mean "no flag, no env var".
    #[arg(long, env = "DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS")]
    pub gateway_idle_timeout_secs: Option<u64>,

    /// Enable semantic search (requires `dcc-mcp-core[semantic]` ONNX
    /// runtime). When enabled, `mode=hybrid` combines fuzzy matching with
    /// embedding similarity. Default: false.
    #[arg(
        long,
        env = "DCC_MCP_SEMANTIC_SEARCH_ENABLED",
        default_value = "false",
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub semantic_search_enabled: bool,

    /// Detach from the terminal and run as a background daemon.
    /// On Unix this respawns a fresh child in a new session; on Windows
    /// the process respawns itself with DETACHED_PROCESS and
    /// CREATE_NEW_PROCESS_GROUP flags.
    #[arg(
        long,
        env = "DCC_MCP_DAEMON",
        default_value = "false",
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub daemon: bool,

    /// Write the daemon process ID to this file. Implicitly enables
    /// --daemon when a path is provided.
    #[arg(long, env = "DCC_MCP_PIDFILE", value_name = "PATH")]
    pub pidfile: Option<PathBuf>,

    /// Restart the running gateway daemon (requires --daemon and --pidfile).
    /// Gracefully stops the old process, then starts a new one.
    /// Prints the new PID, crate version, and log directory on success.
    #[arg(long, default_value = "false")]
    pub restart: bool,

    // ── Value-source bookkeeping ──────────────────────────────────────────
    // Populated by [`GatewayArgs::note_value_sources`], never by clap. They
    // exist because a resolved `false` cannot be distinguished from "not
    // supplied" by looking at the value alone, and an explicit CLI `false`
    // must win over an inherited env `true`.
    /// True when `--gateway-persist` was supplied on the command line.
    #[arg(skip)]
    pub gateway_persist_explicit: bool,

    /// True when `--semantic-search-enabled` was supplied on the command line.
    #[arg(skip)]
    pub semantic_search_enabled_explicit: bool,

    /// True when `--no-admin` was supplied on the command line.
    #[arg(skip)]
    pub no_admin_explicit: bool,
}

impl GatewayArgs {
    /// The defaults clap would apply with no argv and no environment.
    ///
    /// Written out literally rather than derived by running the clap command
    /// over an empty argv: several fields read an `env` variable, so a
    /// parse-based default could **fail** on a malformed value (say
    /// `DCC_MCP_GATEWAY_PORT=not-a-port`, where `port` is a `u16`). Rust
    /// callers assume `Default::default()` cannot panic, and this value also
    /// has to stay stable regardless of whatever the host exported.
    ///
    /// The trade-off is that these literals must be kept in step with the
    /// `#[arg(default_value = ...)]` attributes above; [`gateway_defaults_match_clap`]
    /// pins them from the test side.
    fn clap_defaults() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: None,
            remote_host: "127.0.0.1".to_string(),
            remote_port: 59765,
            registry_dir: None,
            no_admin: false,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: None,
            semantic_search_enabled: false,
            daemon: false,
            pidfile: None,
            restart: false,
            gateway_persist_explicit: false,
            semantic_search_enabled_explicit: false,
            no_admin_explicit: false,
        }
    }
}

impl Default for GatewayArgs {
    /// Constructs a `GatewayArgs` with clap's own defaults for the parsed
    /// fields and "nothing supplied" for the value-source bookkeeping.
    ///
    /// Handy in tests and for embedders that build the struct directly: they
    /// spread `..GatewayArgs::default()` instead of enumerating the
    /// bookkeeping fields, which are never set by clap.
    fn default() -> Self {
        Self::clap_defaults()
    }
}

impl GatewayArgs {
    /// Record which value source last satisfied each env-backed boolean flag.
    ///
    /// Clap resolves `env` + `default_value` before this point, so by the time
    /// the derive struct exists a `false` is ambiguous: the operator may have
    /// typed `--gateway-persist=false` or said nothing at all. Only the
    /// `ArgMatches` value source can tell those apart, and it is what makes an
    /// explicit CLI `false` win over a `true` inherited from the environment
    /// (issue dcc-mcp-core#2642 follow-up).
    ///
    /// The `&mut self` receiver only writes bookkeeping fields; every field it
    /// touches is also directly settable, so callers that construct
    /// `GatewayArgs` by hand (tests, embedders) keep their values.
    pub fn note_value_sources(&mut self, matches: &clap::ArgMatches) {
        self.gateway_persist_explicit = explicit_on_command_line(matches, GATEWAY_PERSIST_ID);
        self.semantic_search_enabled_explicit =
            explicit_on_command_line(matches, SEMANTIC_SEARCH_ENABLED_ID);
        self.no_admin_explicit = explicit_on_command_line(matches, NO_ADMIN_ID);
    }

    /// Did the operator type this flag explicitly on the command line?
    ///
    /// Unknown ids return `false`, which is the conservative reading: a flag
    /// we cannot attribute is treated as "not explicitly supplied", so the
    /// derived value (and therefore the env var) still applies.
    pub fn explicitly_supplied(&self, id: &str) -> bool {
        match id {
            GATEWAY_PERSIST_ID => self.gateway_persist_explicit,
            SEMANTIC_SEARCH_ENABLED_ID => self.semantic_search_enabled_explicit,
            NO_ADMIN_ID => self.no_admin_explicit,
            _ => false,
        }
    }

    /// Resolve the effective gateway persist flag.
    ///
    /// Precedence: an explicit CLI value beats `DCC_MCP_GATEWAY_PERSIST`;
    /// otherwise the env-var/default value clap already resolved wins. The
    /// former `args.gateway_persist || env == "1"` made an explicit
    /// `--gateway-persist=false` unreachable whenever the variable was set to
    /// `1` in the host environment.
    pub fn effective_gateway_persist(&self) -> bool {
        if self.gateway_persist_explicit {
            self.gateway_persist
        } else {
            self.gateway_persist || env_flag_enabled(GATEWAY_PERSIST_ENV)
        }
    }

    /// Resolve the effective semantic-search flag, using the same
    /// explicit-CLI-wins precedence as [`Self::effective_gateway_persist`].
    pub fn effective_semantic_search_enabled(&self) -> bool {
        if self.semantic_search_enabled_explicit {
            self.semantic_search_enabled
        } else {
            self.semantic_search_enabled || env_flag_enabled(SEMANTIC_SEARCH_ENABLED_ENV)
        }
    }

    /// Resolve the effective admin-disable flag.
    ///
    /// Explicit CLI values win over the environment. Programmatic callers
    /// retain the same env fallback as the other gateway boolean helpers.
    pub fn effective_no_admin(&self) -> bool {
        if self.no_admin_explicit {
            self.no_admin
        } else {
            self.no_admin || env_flag_enabled(NO_ADMIN_ENV)
        }
    }

    /// Effective idle timeout in seconds.
    ///
    /// `None` means the operator supplied neither `--gateway-idle-timeout-secs`
    /// nor `DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS`, in which case the server
    /// entry-point default applies. Callers that must distinguish "explicitly
    /// asked for 30" from "said nothing" should read the `Option` field
    /// directly — that distinction is the whole reason it is an `Option`.
    pub fn gateway_idle_timeout_secs(&self) -> u64 {
        self.gateway_idle_timeout_secs
            .unwrap_or(dcc_mcp_gateway::gateway::idle_timeout::SERVER_DEFAULT)
    }
}

/// Restart the gateway daemon by gracefully stopping the old process
/// and spawning a new one.
///
/// Behaviour matches `multica daemon restart`:
/// 1. Read the PID from `--pidfile`.
/// 2. If a live process owns that PID, send `taskkill /PID <pid> /F`
///    (Windows) or `kill <pid>` (Unix) and wait for it to exit.
/// 3. Spawn a detached replacement gateway (the new process daemonizes
///    itself via `DCC_MCP__DAEMONIZED=1`).
/// 4. Poll `/health` until the new gateway responds.
/// 5. Print the new PID, crate version, and log directory path.
///
/// Returns an error if no live process is found (use `gateway --daemon`
/// to start fresh).
pub async fn restart_gateway(args: &GatewayArgs) -> anyhow::Result<()> {
    let pidfile = args
        .pidfile
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--restart requires --pidfile <PATH>"))?;

    // ── 1. Read old PID ──────────────────────────────────────────────
    let old_pid = match dcc_mcp_gateway_ensure::read_pid_from_pidfile(Some(pidfile)) {
        Some(pid) => pid,
        None => {
            return Err(anyhow::anyhow!(
                "no live gateway found (pidfile '{}' missing or empty) — use --daemon to start a fresh gateway",
                pidfile.display()
            ));
        }
    };

    // ── 2. Check liveness and stop old process ─────────────────────
    if !dcc_mcp_gateway_ensure::is_process_alive(old_pid) {
        // Stale pidfile — clean up and spawn fresh.
        eprintln!(
            "WARN: pidfile '{}' points to a dead process (PID {}); removing stale pidfile",
            pidfile.display(),
            old_pid
        );
        let _ = std::fs::remove_file(pidfile);
        // Fall through to fresh start.
        return restart_spawn_new(args).await;
    }

    eprintln!("Stopping gateway daemon (pid {old_pid})...");
    dcc_mcp_gateway_ensure::stop_process(old_pid)
        .map_err(|e| anyhow::anyhow!("failed to stop old gateway (pid {old_pid}): {e}"))?;

    // Wait for the old process to exit (poll every 150 ms, up to 15 s).
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if !dcc_mcp_gateway_ensure::is_process_alive(old_pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    if dcc_mcp_gateway_ensure::is_process_alive(old_pid) {
        eprintln!("WARN: old gateway (pid {old_pid}) did not exit within 15 s; proceeding anyway");
    }

    // ── 3. Spawn new detached gateway ───────────────────────────────
    restart_spawn_new(args).await
}

/// Spawn the new detached gateway process and wait for it to become ready.
async fn restart_spawn_new(args: &GatewayArgs) -> anyhow::Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("cannot resolve current executable: {e}"))?;

    let child_args = restart_child_args(args);

    // Re-read pidfile after child daemonizes (child writes its new PID).
    let pidfile_path = args.pidfile.as_deref().unwrap(); // guaranteed Some by caller

    let mut cmd = Command::new(&exe);
    cmd.args(&child_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached_command(&mut cmd);

    let _child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn gateway child: {e}"))?;

    // ── 4. Wait for new gateway to become ready ────────────────────
    let timeout = Duration::from_secs(dcc_mcp_gateway_ensure::resolve_ensure_timeout_secs(0));
    dcc_mcp_gateway_ensure::wait_gateway_ready(&args.host, args.port, timeout)
        .await
        .map_err(|e| anyhow::anyhow!("gateway did not become ready: {e}"))?;

    // ── 5. Read new PID and print restart summary ───────────────────
    let new_pid = dcc_mcp_gateway_ensure::read_pid_from_pidfile(Some(pidfile_path))
        .ok_or_else(|| anyhow::anyhow!("could not read new PID from pidfile"))?;

    let version = env!("CARGO_PKG_VERSION");
    let log_dir = args
        .registry_dir
        .clone()
        .unwrap_or_else(dcc_mcp_gateway_ensure::default_registry_dir);

    eprintln!("Gateway daemon started (pid {new_pid}, version {version})");
    eprintln!("Logs: {}", log_dir.display());

    Ok(())
}

/// Preserve the resolved gateway configuration across the restart process boundary.
/// `--restart` is deliberately omitted; the replacement only daemonizes.
fn restart_child_args(args: &GatewayArgs) -> Vec<std::ffi::OsString> {
    let mut child_args: Vec<std::ffi::OsString> = Vec::new();
    child_args.push(std::ffi::OsString::from("gateway"));
    push_arg(&mut child_args, "--host", &args.host);
    push_arg(&mut child_args, "--port", &args.port.to_string());
    if let Some(ref name) = args.name {
        child_args.push(std::ffi::OsString::from("--name"));
        child_args.push(std::ffi::OsString::from(name));
    }
    push_arg(&mut child_args, "--remote-host", &args.remote_host);
    push_arg(
        &mut child_args,
        "--remote-port",
        &args.remote_port.to_string(),
    );
    // Forward the effective values, including false. The child inherits the
    // environment, so omitting a false flag revives an overridden env value.
    push_arg(
        &mut child_args,
        "--no-admin",
        &args.effective_no_admin().to_string(),
    );
    push_arg(&mut child_args, "--admin-path", &args.admin_path);
    push_arg(
        &mut child_args,
        "--stale-timeout-secs",
        &args.stale_timeout_secs.to_string(),
    );
    if let Some(ref dir) = args.registry_dir {
        child_args.push(std::ffi::OsString::from("--registry-dir"));
        child_args.push(std::ffi::OsString::from(dir.as_os_str()));
    }
    push_arg(
        &mut child_args,
        "--gateway-persist",
        &args.effective_gateway_persist().to_string(),
    );
    push_arg(
        &mut child_args,
        "--semantic-search-enabled",
        &args.effective_semantic_search_enabled().to_string(),
    );
    // Forward the flag only when it was actually supplied. Comparing the
    // *value* against the default (the old `!= 30`) conflated an explicit
    // `--gateway-idle-timeout-secs 30` with "not supplied" and silently
    // dropped the operator's choice.
    if let Some(idle_timeout) = args.gateway_idle_timeout_secs {
        push_arg(
            &mut child_args,
            "--gateway-idle-timeout-secs",
            &idle_timeout.to_string(),
        );
    }
    child_args.push(std::ffi::OsString::from("--daemon"));
    if let Some(ref pidfile) = args.pidfile {
        child_args.push(std::ffi::OsString::from("--pidfile"));
        child_args.push(std::ffi::OsString::from(pidfile.as_os_str()));
    }
    #[cfg(feature = "mdns")]
    push_arg(
        &mut child_args,
        "--discover-mdns",
        &args.discover_mdns.to_string(),
    );
    for rs in &args.relay_sources {
        child_args.push(std::ffi::OsString::from("--relay-source"));
        child_args.push(std::ffi::OsString::from(format!(
            "{}={}",
            rs.0.admin_url, rs.0.public_base_url
        )));
    }

    child_args
}

/// Push a `--flag` and its value.
fn push_arg(args: &mut Vec<std::ffi::OsString>, flag: &str, value: &str) {
    args.push(std::ffi::OsString::from(flag));
    args.push(std::ffi::OsString::from(value));
}

/// Build the [`GatewayConfig`] that the standalone daemon uses.
///
/// Extracted so the regression test can construct the exact same
/// runtime configuration without invoking the blocking `run` loop.
pub fn build_gateway_config(args: &GatewayArgs, gateway_name: &str) -> GatewayConfig {
    let admin_retention = std::env::var("DCC_MCP_GATEWAY_ADMIN_RETENTION_DAYS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(30)
        .clamp(1, 3650);
    GatewayConfig {
        host: args.host.clone(),
        gateway_port: args.port,
        remote_host: Some(args.remote_host.clone()),
        remote_gateway_port: args.remote_port,
        stale_timeout_secs: args.stale_timeout_secs,
        server_name: "dcc-mcp-gateway".to_string(),
        gateway_name: Some(gateway_name.to_string()),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        registry_dir: args.registry_dir.clone(),
        adapter_dcc: Some(STANDALONE_GATEWAY_DCC.to_string()),
        #[cfg(feature = "mdns")]
        discover_mdns: args.discover_mdns,
        relay_sources: args
            .relay_sources
            .iter()
            .map(|source| source.0.clone())
            .collect(),
        admin_enabled: !args.effective_no_admin(),
        admin_path: args.admin_path.clone(),
        admin_persist: AdminPersistConfig {
            sqlite_path: std::env::var_os("DCC_MCP_GATEWAY_ADMIN_DB").map(PathBuf::from),
            sqlite_retention_days: admin_retention,
            ..AdminPersistConfig::default()
        },
        gateway_persist: args.effective_gateway_persist(),
        gateway_idle_timeout_secs: args.gateway_idle_timeout_secs(),
        semantic_search_enabled: args.effective_semantic_search_enabled(),
        ..GatewayConfig::default()
    }
}

/// Run the standalone gateway until a shutdown signal arrives (or the
/// idle-timeout fires when no backends remain).
pub async fn run(args: GatewayArgs) -> anyhow::Result<()> {
    // ── Daemonize if requested ────────────────────────────────────────────
    let _pidfile = if args.daemon || args.pidfile.is_some() {
        daemonize_gateway(&args)?
    } else {
        None
    };

    let gateway_name = args.name.clone().unwrap_or_else(default_gateway_name);
    let cfg = build_gateway_config(&args, &gateway_name);
    let runner =
        GatewayRunner::new(cfg).map_err(|err| anyhow::anyhow!("creating GatewayRunner: {err}"))?;
    let mut outcome = runner
        .run_election()
        .await
        .map_err(|err| anyhow::anyhow!("running gateway election: {err}"))?;

    if !outcome.is_gateway {
        tracing::info!(
            host = %args.host,
            port = args.port,
            "standalone gateway found an existing owner; exiting"
        );
        return Ok(());
    }

    let gateway_persist = args.effective_gateway_persist();
    let gateway_idle_timeout_secs = args.gateway_idle_timeout_secs();

    tracing::info!(
        gateway_name = %gateway_name,
        host = %args.host,
        port = args.port,
        gateway_persist,
        gateway_idle_timeout_secs,
        "standalone gateway running"
    );

    // ── Wait for shutdown trigger ──────────────────────────────────────────
    let supervisor_exit = {
        let supervisor = outcome.gateway_supervisor.take();
        async move {
            if let Some(supervisor) = supervisor {
                let _ = supervisor.await;
            } else {
                std::future::pending::<()>().await;
            }
        }
    };
    let shutdown_reason = tokio::select! {
        sig = crate::select_shutdown_signal() => {
            sig.unwrap_or("signal")
        }
        _ = supervisor_exit => {
            "gateway_supervisor_exit"
        }
    };
    tracing::info!(shutdown_reason, "standalone gateway shutting down");

    if let Some(abort) = outcome.gateway_abort.take() {
        abort.abort();
    }
    if let Some(key) = outcome.sentinel_key.take() {
        let _ = runner.registry.deregister_async(key).await;
    }

    // Drop the pidfile guard so the file is cleaned up.
    drop(_pidfile);
    Ok(())
}

/// Daemonize the gateway process and return a pidfile guard for the child.
fn daemonize_gateway(args: &GatewayArgs) -> anyhow::Result<Option<PidfileGuard>> {
    if matches!(std::env::var(DAEMONIZED_ENV).as_deref(), Ok("1")) {
        configure_daemon_child()?;
        let guard = write_gateway_pidfile(&args.pidfile)?;
        if args.daemon && args.pidfile.is_none() {
            tracing::info!(pid = std::process::id(), "gateway running detached");
        }
        return Ok(guard);
    }

    let exe = std::env::current_exe().map_err(|err| {
        anyhow::anyhow!("cannot resolve current executable for detached respawn: {err}")
    })?;
    let mut cmd = Command::new(exe);
    cmd.args(std::env::args().skip(1));
    cmd.env(DAEMONIZED_ENV, "1");
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    configure_detached_command(&mut cmd);

    let child = cmd
        .spawn()
        .map_err(|err| anyhow::anyhow!("failed to respawn detached gateway: {err}"))?;

    // Write pidfile for the spawned child before the parent exits.
    if let Some(ref pidfile) = args.pidfile {
        write_pidfile_value(pidfile, child.id())?;
    }
    if args.daemon && args.pidfile.is_none() {
        tracing::info!(pid = child.id(), "gateway daemonized (detached child)");
    }
    std::process::exit(0);
}

#[cfg(unix)]
fn configure_daemon_child() -> anyhow::Result<()> {
    let session_id = unsafe { libc::setsid() };
    if session_id < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EPERM) {
            return Err(err).map_err(|err| anyhow::anyhow!("setsid failed in daemon child: {err}"));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn configure_daemon_child() -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn configure_detached_command(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn configure_detached_command(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn configure_detached_command(_cmd: &mut Command) {}

/// ```ignore
/// A sentinel that removes the pidfile on Drop.
/// ```
struct PidfileGuard {
    path: PathBuf,
}

impl Drop for PidfileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_gateway_pidfile(pidfile: &Option<PathBuf>) -> anyhow::Result<Option<PidfileGuard>> {
    let Some(path) = pidfile.as_ref() else {
        return Ok(None);
    };
    write_pidfile_value(path, std::process::id())?;
    Ok(Some(PidfileGuard { path: path.clone() }))
}

fn write_pidfile_value(path: &PathBuf, pid: u32) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            anyhow::anyhow!("creating pidfile directory {}: {err}", parent.display())
        })?;
    }
    std::fs::write(path, format!("{pid}\n"))
        .map_err(|err| anyhow::anyhow!("writing pidfile {}: {err}", path.display()))
}

fn default_gateway_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "local".to_string());
    format!("gateway-{host}-pid{}", std::process::id())
}

#[cfg(test)]
mod restart_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;
    use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
    use serde_json::json;

    #[test]
    fn relay_source_arg_maps_into_gateway_config() {
        let source: RelaySourceArg = "http://127.0.0.1:9872=http://127.0.0.1:9873"
            .parse()
            .expect("relay source arg should parse");

        assert_eq!(source.0.admin_url, "http://127.0.0.1:9872");
        assert_eq!(source.0.public_base_url, "http://127.0.0.1:9873");
        assert!(source.0.poll_interval_secs.is_none());
        assert!(
            "http://127.0.0.1:9872=".parse::<RelaySourceArg>().is_err(),
            "empty public relay URL must be rejected"
        );

        let args = GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: Some("relay-source-test".to_string()),
            remote_host: "127.0.0.1".to_string(),
            remote_port: 0,
            registry_dir: None,
            no_admin: true,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: vec![source],
            gateway_persist: false,
            gateway_idle_timeout_secs: Some(30),
            semantic_search_enabled: false,
            daemon: false,
            pidfile: None,
            restart: false,
            ..Default::default()
        };

        let cfg = build_gateway_config(&args, "relay-source-test");
        assert_eq!(cfg.relay_sources.len(), 1);
        assert_eq!(cfg.relay_sources[0].admin_url, "http://127.0.0.1:9872");
        assert_eq!(
            cfg.relay_sources[0].public_base_url,
            "http://127.0.0.1:9873"
        );
    }

    fn ephemeral_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    async fn spawn_ready_backend() -> (u16, tokio::sync::oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = axum::Router::new()
            .route(
                "/v1/readyz",
                axum::routing::get(|| async {
                    axum::Json(json!({
                        "process": true,
                        "dispatcher": true,
                        "dcc": true,
                    }))
                }),
            )
            .route(
                "/health",
                axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
            );
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await
                .ok();
        });
        (port, tx)
    }

    async fn wait_gateway_health_up(
        client: &reqwest::Client,
        port: u16,
        timeout: std::time::Duration,
    ) {
        let started = std::time::Instant::now();
        let url = format!("http://127.0.0.1:{port}/health");
        loop {
            if let Ok(response) = client.get(&url).send().await
                && response.status().is_success()
            {
                return;
            }
            assert!(
                started.elapsed() < timeout,
                "gateway did not answer /health within {timeout:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    async fn assert_gateway_health(client: &reqwest::Client, port: u16, context: &str) {
        let health = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            .unwrap_or_else(|err| panic!("gateway /health failed {context}: {err}"));
        assert!(
            health.status().is_success(),
            "gateway /health expected 2xx {context}, got {}",
            health.status()
        );
    }

    async fn wait_gateway_health_down(
        client: &reqwest::Client,
        port: u16,
        timeout: std::time::Duration,
    ) {
        let started = std::time::Instant::now();
        let url = format!("http://127.0.0.1:{port}/health");
        loop {
            match client.get(&url).send().await {
                Ok(response) if response.status().is_success() => {}
                _ => return,
            }
            assert!(
                started.elapsed() < timeout,
                "gateway stayed healthy after all routable backends disappeared"
            );
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }

    async fn cleanup_gateway_outcome(
        runner: &GatewayRunner,
        outcome: &mut dcc_mcp_gateway::ElectionOutcome,
    ) {
        if let Some(abort) = outcome.gateway_abort.take() {
            abort.abort();
        }
        if let Some(supervisor) = outcome.gateway_supervisor.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), supervisor).await;
        }
        if let Some(key) = outcome.sentinel_key.take() {
            let _ = runner.registry.deregister_async(key).await;
        }
    }

    /// Issue #1358 — the standalone gateway daemon must serve gateway-
    /// native endpoints **without any DCC backend being registered**. The
    /// daemon's `adapter_dcc = Some("gateway")` marker is what distinguishes
    /// it from a per-DCC server that happens to win the election.
    #[tokio::test]
    async fn standalone_daemon_serves_health_without_any_backend() {
        let dir = tempfile::tempdir().unwrap();
        let gw_port = ephemeral_port();
        let args = GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: gw_port,
            name: Some("standalone-daemon-test".to_string()),
            remote_host: "127.0.0.1".to_string(),
            remote_port: 0,
            registry_dir: Some(dir.path().to_path_buf()),
            no_admin: true,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: Some(30),
            semantic_search_enabled: false,
            daemon: false,
            pidfile: None,
            restart: false,
            ..Default::default()
        };
        let cfg = build_gateway_config(&args, args.name.as_deref().unwrap());
        assert_eq!(
            cfg.adapter_dcc.as_deref(),
            Some("gateway"),
            "daemon-mode config must stamp the standalone marker"
        );

        let runner = GatewayRunner::new(cfg).expect("creating GatewayRunner");
        let mut outcome = runner
            .run_election()
            .await
            .expect("standalone daemon must win election on a free port");
        assert!(
            outcome.is_gateway,
            "standalone daemon must elect itself as the gateway"
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let health = client
            .get(format!("http://127.0.0.1:{gw_port}/health"))
            .send()
            .await
            .expect("daemon must answer /health without a DCC backend");
        assert!(health.status().is_success(), "/health expected 200 OK");

        if let Some(abort) = outcome.gateway_abort.take() {
            abort.abort();
        }
        if let Some(key) = outcome.sentinel_key.take() {
            let _ = runner.registry.deregister_async(key).await;
        }
    }

    #[tokio::test]
    async fn standalone_daemon_idle_shutdown_tracks_routable_live_instances() {
        let dir = tempfile::tempdir().unwrap();
        let gw_port = ephemeral_port();
        let args = GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: gw_port,
            name: Some("standalone-daemon-liveness-e2e".to_string()),
            remote_host: "127.0.0.1".to_string(),
            remote_port: 0,
            registry_dir: Some(dir.path().to_path_buf()),
            no_admin: true,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: Some(1),
            semantic_search_enabled: false,
            daemon: false,
            pidfile: None,
            restart: false,
            ..Default::default()
        };
        let cfg = build_gateway_config(&args, args.name.as_deref().unwrap());
        let runner = GatewayRunner::new(cfg).expect("creating GatewayRunner");
        let mut outcome = runner
            .run_election()
            .await
            .expect("standalone daemon must win election on a free port");
        assert!(outcome.is_gateway);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        wait_gateway_health_up(&client, gw_port, std::time::Duration::from_secs(5)).await;

        let (maya_port, maya_stop) = spawn_ready_backend().await;
        let (photoshop_port, photoshop_stop) = spawn_ready_backend().await;
        let maya = ServiceEntry::new("maya", "127.0.0.1", maya_port);
        let maya_key = maya.key();
        let photoshop = ServiceEntry::new("photoshop", "127.0.0.1", photoshop_port);
        let photoshop_key = photoshop.key();
        runner
            .registry
            .register_async(maya)
            .await
            .expect("register maya backend");
        runner
            .registry
            .register_async(photoshop)
            .await
            .expect("register photoshop backend");

        tokio::time::sleep(std::time::Duration::from_secs(7)).await;
        assert_gateway_health(
            &client,
            gw_port,
            "while two routable DCC backends are registered",
        )
        .await;

        runner
            .registry
            .deregister_async(maya_key)
            .await
            .expect("deregister first backend");
        let _ = maya_stop.send(());
        tokio::time::sleep(std::time::Duration::from_secs(7)).await;
        assert_gateway_health(&client, gw_port, "while one routable DCC backend remains").await;

        let _ = photoshop_stop.send(());
        runner
            .registry
            .update_status_async(photoshop_key, ServiceStatus::Unreachable)
            .await
            .expect("mark remaining backend unreachable");
        wait_gateway_health_down(&client, gw_port, std::time::Duration::from_secs(16)).await;
        cleanup_gateway_outcome(&runner, &mut outcome).await;
    }

    #[test]
    fn gateway_args_restart_flag_defaults_to_false() {
        let args = GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: None,
            remote_host: "0.0.0.0".to_string(),
            remote_port: 59765,
            registry_dir: None,
            no_admin: false,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: Some(30),
            semantic_search_enabled: false,
            daemon: false,
            pidfile: None,
            restart: false,
            ..Default::default()
        };
        assert!(!args.restart, "restart flag must default to false");
    }

    #[test]
    fn gateway_args_restart_flag_can_be_set() {
        let mut args = GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: None,
            remote_host: "0.0.0.0".to_string(),
            remote_port: 59765,
            registry_dir: None,
            no_admin: false,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: Some(30),
            semantic_search_enabled: false,
            daemon: false,
            pidfile: Some(std::path::PathBuf::from("/tmp/gw.pid")),
            restart: false,
            ..Default::default()
        };
        args.restart = true;
        assert!(args.restart);
        assert!(args.pidfile.is_some());
    }

    #[tokio::test]
    async fn restart_gateway_fails_without_pidfile() {
        let args = GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: None,
            remote_host: "0.0.0.0".to_string(),
            remote_port: 0,
            registry_dir: None,
            no_admin: true,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: Some(30),
            semantic_search_enabled: false,
            daemon: true,
            pidfile: None, // no pidfile → restart must fail
            restart: true,
            ..Default::default()
        };
        let result = restart_gateway(&args).await;
        assert!(result.is_err(), "restart without --pidfile must fail");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("--restart requires --pidfile"),
            "error must mention --pidfile: {err}"
        );
    }

    /// Minimal clap wrapper so tests can parse `GatewayArgs` the same way
    /// `dcc-mcp-server gateway …` does.
    #[derive(Debug, clap::Parser)]
    struct GatewayArgsParser {
        #[command(flatten)]
        args: GatewayArgs,
    }

    fn parse_gateway_args(argv: &[&str]) -> GatewayArgs {
        GatewayArgsParser::parse_from(argv).args
    }

    fn base_args(idle_timeout: Option<u64>) -> GatewayArgs {
        GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: None,
            remote_host: "127.0.0.1".to_string(),
            remote_port: 0,
            registry_dir: None,
            no_admin: true,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: idle_timeout,
            semantic_search_enabled: false,
            daemon: false,
            pidfile: None,
            restart: false,
            ..Default::default()
        }
    }

    /// The standalone daemon CLI still *runs* with 30 s when the operator
    /// says nothing. No clap `default_value` is set, so `None` here means
    /// "no flag and no env var" and the accessor supplies the 30.
    #[test]
    fn daemon_cli_default_idle_timeout_is_the_server_default() {
        let args = parse_gateway_args(&["gateway"]);
        assert_eq!(args.gateway_idle_timeout_secs, None);
        assert_eq!(
            args.gateway_idle_timeout_secs(),
            dcc_mcp_gateway::gateway::idle_timeout::SERVER_DEFAULT
        );
        assert_eq!(args.gateway_idle_timeout_secs(), 30);

        // The config the daemon actually starts with must carry the 30 too.
        let cfg = build_gateway_config(&args, "idle-default-test");
        assert_eq!(cfg.gateway_idle_timeout_secs, 30);
    }

    /// Regression: an operator who explicitly passes `30` must be
    /// distinguishable from one who says nothing. The old `u64` + `!= 30`
    /// value comparison could not tell them apart and silently dropped the
    /// explicit flag.
    #[test]
    fn explicit_idle_timeout_equal_to_the_default_is_still_distinguishable() {
        let explicit = parse_gateway_args(&["gateway", "--gateway-idle-timeout-secs", "30"]);
        let implicit = parse_gateway_args(&["gateway"]);

        assert_eq!(explicit.gateway_idle_timeout_secs, Some(30));
        assert_eq!(implicit.gateway_idle_timeout_secs, None);
        assert_ne!(
            explicit.gateway_idle_timeout_secs, implicit.gateway_idle_timeout_secs,
            "explicit 30 must not collapse into 'not supplied'"
        );
        // Both resolve to the same effective value — the distinction is
        // provenance, not the number.
        assert_eq!(
            explicit.gateway_idle_timeout_secs(),
            implicit.gateway_idle_timeout_secs()
        );
    }

    #[test]
    fn explicit_zero_idle_timeout_is_distinguished_from_the_default() {
        let args = parse_gateway_args(&["gateway", "--gateway-idle-timeout-secs", "0"]);
        assert_eq!(args.gateway_idle_timeout_secs, Some(0));
        assert_eq!(args.gateway_idle_timeout_secs(), 0);
    }

    /// The value the daemon actually runs with: explicit when supplied,
    /// otherwise the server entry-point default.
    #[test]
    fn built_gateway_config_uses_the_effective_idle_timeout() {
        let explicit = build_gateway_config(&base_args(Some(120)), "idle-timeout-test");
        assert_eq!(explicit.gateway_idle_timeout_secs, 120);

        let implicit = build_gateway_config(&base_args(None), "idle-timeout-test");
        assert_eq!(
            implicit.gateway_idle_timeout_secs,
            dcc_mcp_gateway::gateway::idle_timeout::SERVER_DEFAULT
        );
        assert_eq!(implicit.gateway_idle_timeout_secs, 30);
    }

    /// The auto-ensure constant is a different entry point from the server
    /// default; guards against a future refactor unifying the two.
    #[test]
    fn auto_ensure_default_differs_from_the_server_default() {
        assert_eq!(AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_SECS, 300);
        assert_ne!(
            AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_SECS,
            dcc_mcp_gateway::gateway::idle_timeout::SERVER_DEFAULT
        );
    }
}
