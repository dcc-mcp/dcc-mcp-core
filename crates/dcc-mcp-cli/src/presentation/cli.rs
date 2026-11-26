use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
#[cfg(test)]
use base64::Engine;
use clap::{Parser, Subcommand, ValueEnum};
use dcc_mcp_client::McpHttpClient;
use dcc_mcp_skills::validator::IssueSeverity;
use dcc_mcp_skills::{SkillValidationReport, validate_skill_dir};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::output::{ExitCode, OutputFormat, OutputWriter, failure_envelope, to_json};
use super::update_maintenance::UpdateMaintenance;

use crate::application::adapter_import;
use crate::application::call_attribution::{
    attach_agent_session_id, attach_batch_agent_session_id,
};
use crate::application::capabilities_result;
use crate::application::client::DccMcpClient;
use crate::application::control_plane::DccControlPlane;
use crate::application::dcc_discovery::discover_dcc_installations;
use crate::application::doctor::{DoctorRequest, run_doctor};
use crate::application::gateway_ctrl;
use crate::application::gateway_ensure;
use crate::application::gateway_profile;
use crate::application::host_capabilities::CapabilitiesCommand;
use crate::application::install::InstallService;
use crate::application::marketplace::check_marketplace_updates;
use crate::application::marketplace::new_service;
use crate::domain::install::InstallRequest;
use crate::domain::rest::{
    Endpoint, ReloadSkillsRequest, SearchRequest, StatsRequest, StopInstanceRequest,
    WaitReadyRequest,
};
use crate::domain::start_instance::StartInstanceRequest;
use crate::infra::http::HttpGateway;

pub(crate) mod capabilities_output;
mod cli_args;
mod dcc_types_output;
mod gateway_cmd;
mod image_artifacts;
mod job_progress;
mod lint;
mod marketplace_output;
mod record_replay;
pub(crate) mod result_output;
mod transport_args;
mod ui_control;
mod ui_control_output;

#[cfg(test)]
use image_artifacts::{BASE64_STANDARD, MATERIALIZED_IMAGE_PLACEHOLDER, prune_image_artifacts};
use image_artifacts::{default_image_artifact_root, materialize_call_images};
use job_progress::JobProgressReporter;
use marketplace_output::reload_marketplace_value;
use record_replay::run_record_replay;
use transport_args::TransportArgs;
use ui_control_output::compact_ui_control_result;

pub(crate) use cli_args::{Command, UiControlAction, UiControlArgs};
use cli_args::{
    build_load_skill_request, command_has_configured_timeout_env, command_has_distinct_per_timeout,
    endpoint_for_mcp, parse_json_object, parse_marketplace_target, parse_output_format,
    read_batch_request, read_call_arguments, resolve_query,
};
use gateway_cmd::{ensure_gateway_for_command, run_gateway_cmd};

use super::marketplace_cmd::{self, MarketplaceAction};
#[cfg(test)]
use super::update_cmd::UpdateAction;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:9765";
const DEFAULT_SMOKE_TIMEOUT_SECS: u64 = 5;
const DEFAULT_CALL_TIMEOUT_SECS: u64 = 30;
const DEFAULT_WAIT_READY_TIMEOUT_SECS: u64 = 30;
const DEFAULT_START_INSTANCE_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Parser)]
#[command(name = "dccmcp", about, version)]
pub struct Args {
    #[arg(long, global = true, env = "DCC_MCP_BASE_URL")]
    base_url: Option<String>,
    /// Select a gateway profile. Use `local` for the local FileRegistry path.
    #[arg(long, global = true, env = "DCC_MCP_GATEWAY_PROFILE")]
    gateway: Option<String>,
    /// Disable the default local gateway auto-start before agent control commands.
    #[arg(long, env = "DCC_MCP_CLI_NO_AUTO_GATEWAY", default_value = "false")]
    no_auto_gateway: bool,
    /// Check and stage a verified CLI update before running this command.
    #[arg(
        long,
        global = true,
        env = "DCC_MCP_CLI_AUTO_UPDATE",
        default_value = "false"
    )]
    auto_update: bool,
    /// Explicit gateway binary for auto-start. Defaults to discovery/cache/current CLI fallback.
    #[arg(long, env = "DCC_MCP_GATEWAY_BIN")]
    auto_gateway_bin: Option<PathBuf>,
    /// Seconds to wait for an auto-started gateway to become healthy.
    #[arg(
        long,
        env = "DCC_MCP_CLI_AUTO_GATEWAY_TIMEOUT_SECS",
        default_value = "10"
    )]
    auto_gateway_timeout_secs: u64,
    /// Output format: human, json, ndjson, or toon. Auto-detects from TTY when omitted.
    #[arg(
        long,
        global = true,
        env = "DCC_MCP_OUTPUT",
        value_parser = parse_output_format
    )]
    output: Option<OutputFormat>,
    /// Keep redundant MCP JSON text mirrors in CLI output for legacy consumers.
    #[arg(long, global = true)]
    raw_result: bool,
    /// Select a JSON pointer from the output; repeat for multiple fields. Missing = null.
    #[arg(long, global = true, value_parser = result_output::parse_pointer)]
    select: Vec<String>,
    /// Non-interactive mode: zero prompts, missing input fails immediately (exit code 2).
    #[arg(long, global = true, env = "DCC_MCP_NON_INTERACTIVE")]
    non_interactive: bool,
    /// Timeout override: per request for call; total budget for wait-ready/start-instance.
    /// call --wait-timeout-secs independently limits total job polling.
    #[arg(long, global = true, env = "DCC_MCP_TIMEOUT_SECS")]
    timeout_secs: Option<u64>,
    #[command(flatten)]
    transport: TransportArgs,
    #[command(subcommand)]
    command: Command,
}

impl Args {
    /// Keep clap's value sources so explicit gateway booleans override env values.
    fn try_parse_annotated_from<I, T>(argv: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        use clap::{CommandFactory, FromArgMatches};

        let matches = Self::command().try_get_matches_from(argv)?;
        let mut args = Self::from_arg_matches(&matches)?;
        if matches!(
            &args.command,
            Command::Verify { .. }
                | Command::Components {
                    action: super::components_cmd::ComponentsAction::InstallVerified(_)
                }
        ) {
            // A fixed-byte offline installation cannot apply an unrelated staged
            // update, launch a gateway, or refresh an update cache on entry.
            args.no_update_maintenance = true;
            args.no_auto_gateway = true;
        }
        if matches!(&args.command, Command::Doctor { .. }) {
            // Diagnostics must not mutate the inspected installation or update caches.
            // Keep the existing bounded version and adapter-import probes unchanged.
            args.no_update_maintenance = true;
        }
        if let Command::Gateway { ref mut daemon, .. } = args.command
            && let Some(gateway_matches) = matches.subcommand_matches("gateway")
        {
            daemon.note_value_sources(gateway_matches);
        }
        Ok(args)
    }
}

// clap keeps flattened command arguments by value; this parser enum is short-lived.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum Command {
    /// Run health + MCP + REST smoke checks against a service.
    Smoke {
        /// MCP URL or base URL. Accepts either http://host:port or http://host:port/mcp.
        #[arg(long)]
        url: Option<String>,
        /// Query used for the REST dynamic-capability search check.
        #[arg(long, default_value = "sphere")]
        query: String,
        /// Result limit used for the REST dynamic-capability search check.
        #[arg(long, default_value = "5")]
        limit: usize,
        /// Per-request timeout for smoke checks.
        #[arg(long, default_value = "5")]
        timeout_secs: u64,
    },
    /// Check the configured gateway or per-DCC REST endpoint.
    Health,
    /// Query persisted gateway tool-call statistics with composable filters.
    Stats {
        /// Time window: 1h, 24h, 7d, or all.
        #[arg(long, default_value = "all", value_parser = ["1h", "24h", "7d", "all"])]
        range: String,
        #[arg(long)]
        dcc_type: Option<String>,
        #[arg(long)]
        skill: Option<String>,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long, value_parser = ["success", "failure"])]
        status: Option<String>,
        #[arg(long)]
        instance_id: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Report local defaults and startup diagnostics without launching services.
    Doctor {
        /// FileRegistry directory to inspect. Defaults to core's shared registry path.
        #[arg(long)]
        registry_dir: Option<PathBuf>,
        /// Gateway host to probe without starting it.
        #[arg(long, default_value = "127.0.0.1")]
        gateway_host: String,
        /// Gateway port to probe without starting it.
        #[arg(long, default_value = "9765")]
        gateway_port: u16,
    },
    /// List live DCC instances from the local registry or selected gateway profile.
    List,
    /// List adapter-backed DCC types from the release catalog.
    DccTypes {
        /// Read a custom adapter catalog instead of the release catalog.
        #[arg(long, env = "DCC_MCP_CATALOG_PATH")]
        catalog: Option<PathBuf>,
    },
    /// Detect locally installed DCC applications without launching them.
    Discover {
        /// Optional DCC type or human alias to filter, such as `maya` or `3ds Max`.
        #[arg(long)]
        dcc_type: Option<String>,
        /// Additional roots to scan. Repeat for local or mounted volumes.
        #[arg(long = "path")]
        paths: Vec<PathBuf>,
    },
    /// Search callable tools through local MCP or the selected gateway profile.
    Search {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        dcc_type: Option<String>,
        /// Filter to a full instance UUID or unique >=4-character prefix.
        #[arg(long)]
        instance_id: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Describe one tool slug.
    Describe { tool_slug: String },
    /// Load a skill on a local or gateway-managed DCC instance.
    LoadSkill {
        #[arg(value_name = "SKILL_NAME")]
        skill_name: Option<String>,
        #[arg(long)]
        dcc_type: Option<String>,
        #[arg(long)]
        dcc: Option<String>,
        #[arg(long)]
        instance_id: Option<String>,
        #[arg(long, value_name = "BOOL")]
        activate_groups: Option<bool>,
        #[arg(long = "json")]
        request_json: Option<String>,
    },
    /// Invoke one tool slug, or an ordered batch with --batch.
    Call {
        #[arg(value_name = "TOOL_SLUG", required_unless_present = "batch")]
        tool_slug: Option<String>,
        /// Invoke an ordered gateway batch instead of one tool.
        #[arg(
            long,
            conflicts_with_all = ["tool_slug", "dcc_type", "instance_id", "meta_json"]
        )]
        batch: bool,
        /// JSON array of batch call steps. Requires --batch.
        #[arg(long, value_name = "JSON", requires = "batch", conflicts_with_all = ["arguments_json", "json_file"])]
        steps: Option<String>,
        /// DCC type for direct backend-tool calls without a dotted gateway slug.
        #[arg(long)]
        dcc_type: Option<String>,
        /// Full instance UUID or unique >=4-character prefix for direct calls.
        #[arg(long)]
        instance_id: Option<String>,
        #[arg(long = "json", default_value = "{}")]
        arguments_json: String,
        /// Read call arguments from a UTF-8 JSON file, or '-' for stdin.
        #[arg(long, value_name = "PATH", conflicts_with = "arguments_json")]
        json_file: Option<PathBuf>,
        #[arg(long)]
        meta_json: Option<String>,
        /// Per-request timeout for the tool call. Increase for renders and other long-running sync tools.
        #[arg(long, env = "DCC_MCP_CLI_CALL_TIMEOUT_SECS", default_value = "30")]
        timeout_secs: u64,
    },
    /// Compatibility alias for `call --batch`.
    #[command(hide = true)]
    CallBatch {
        /// JSON object containing `calls` and optional `stop_on_error`.
        #[arg(long = "json", default_value = "{\"calls\":[]}")]
        request_json: String,
        /// Read the batch request from a UTF-8 JSON file, or '-' for stdin.
        #[arg(long, value_name = "PATH", conflicts_with = "request_json")]
        json_file: Option<PathBuf>,
        #[arg(long, env = "DCC_MCP_CLI_CALL_TIMEOUT_SECS", default_value = "30")]
        timeout_secs: u64,
    },
    /// Run the scoped DCC UI Control fallback through stable app-ui tools.
    UiControl {
        #[command(subcommand)]
        action: UiControlAction,
    },
    /// Wait until a local or gateway-managed instance reports readiness bits.
    WaitReady {
        #[arg(long)]
        dcc_type: Option<String>,
        #[arg(long)]
        instance_id: Option<String>,
        #[arg(long, value_delimiter = ',')]
        require: Vec<String>,
        #[arg(long, default_value = "30")]
        timeout_secs: u64,
        #[arg(long, default_value = "1")]
        interval_secs: u64,
    },
    /// Ask running DCC instances to re-scan installed skill paths.
    ReloadSkills {
        #[arg(long)]
        dcc_type: Option<String>,
        /// Full instance UUID or unique >=4-character prefix.
        #[arg(long)]
        instance_id: Option<String>,
    },
    /// Ask a test-owned instance to stop through its advertised safe-stop hook.
    StopInstance {
        #[arg(long)]
        dcc_type: String,
        #[arg(long)]
        instance_id: String,
        #[arg(long)]
        expected_owner: Option<String>,
        #[arg(long)]
        expected_session: Option<String>,
    },
    /// Build an auditable DCC adapter installation plan.
    Install {
        #[arg(long)]
        dcc_type: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long, env = "DCC_MCP_CATALOG_PATH")]
        catalog: Option<PathBuf>,
        /// Python interpreter used for pip-based adapter package installs.
        #[arg(long, env = "DCC_MCP_INSTALL_PYTHON")]
        python: Option<String>,
        /// Execute the install plan with consent gating.
        #[arg(long, short = 'x')]
        execute: bool,
    },
    /// Search and manage DCC-MCP marketplace sources.
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceAction,
    },
    /// Validate local SKILL.md packages before loading them at runtime.
    Lint(LintArgs),
    /// Check for and apply gateway-controlled binary updates.
    Update {
        #[command(subcommand)]
        action: Option<UpdateAction>,
    },
    /// Gateway lifecycle management.
    Gateway {
        #[command(subcommand)]
        action: Option<GatewayAction>,
        #[command(flatten)]
        daemon: dcc_mcp_sidecar::gateway_daemon::GatewayArgs,
    },
    /// Use the shared MCP client against any Streamable HTTP endpoint.
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Debug, Subcommand)]
enum McpAction {
    /// Perform the MCP initialize handshake and print the negotiated session.
    Initialize(McpInitializeArgs),
    /// Work with MCP tools.
    Tools {
        #[command(subcommand)]
        action: McpToolsAction,
    },
    /// Work with MCP resources.
    Resources {
        #[command(subcommand)]
        action: McpResourcesAction,
    },
    /// Work with MCP prompts.
    Prompts {
        #[command(subcommand)]
        action: McpPromptsAction,
    },
}

#[derive(Debug, Subcommand)]
enum McpToolsAction {
    /// List all tools, following MCP pagination cursors.
    List(McpConnectionArgs),
    /// Call one MCP tool by its server-visible name.
    Call(McpToolCallArgs),
}

#[derive(Debug, Subcommand)]
enum McpResourcesAction {
    /// List all resources, following MCP pagination cursors.
    List(McpConnectionArgs),
    /// Read one resource by URI.
    Read(McpResourceReadArgs),
}

#[derive(Debug, Subcommand)]
enum McpPromptsAction {
    /// List all prompts, following MCP pagination cursors.
    List(McpConnectionArgs),
    /// Render one prompt with JSON arguments.
    Get(McpPromptGetArgs),
}

#[derive(Debug, Clone, clap::Args)]
struct McpConnectionArgs {
    /// MCP endpoint, for example http://127.0.0.1:9765/mcp.
    #[arg(long, env = "DCC_MCP_MCP_URL")]
    url: Option<String>,
    /// Timeout for each HTTP request.
    #[arg(long, default_value = "30")]
    timeout_secs: u64,
}

#[derive(Debug, Clone, clap::Args)]
struct McpInitializeArgs {
    #[command(flatten)]
    connection: McpConnectionArgs,
    /// Client name advertised in initialize.
    #[arg(long, default_value = "dccmcp")]
    client_name: String,
    /// Client version advertised in initialize.
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    client_version: String,
}

#[derive(Debug, Clone, clap::Args)]
struct McpToolCallArgs {
    #[command(flatten)]
    connection: McpConnectionArgs,
    /// Server-visible MCP tool name.
    #[arg(long)]
    name: String,
    /// JSON object passed as MCP tool arguments.
    #[arg(long = "json", default_value = "{}")]
    arguments_json: String,
    /// Optional JSON object passed as MCP `_meta`.
    #[arg(long)]
    meta_json: Option<String>,
}

#[derive(Debug, Clone, clap::Args)]
struct McpResourceReadArgs {
    #[command(flatten)]
    connection: McpConnectionArgs,
    /// Resource URI.
    #[arg(long)]
    uri: String,
}

#[derive(Debug, Clone, clap::Args)]
struct McpPromptGetArgs {
    #[command(flatten)]
    connection: McpConnectionArgs,
    /// Prompt name.
    #[arg(long)]
    name: String,
    /// JSON object containing prompt argument values.
    #[arg(long = "arguments-json", default_value = "{}")]
    arguments_json: String,
}

fn apply_staged_update() -> bool {
    // Apply any staged binary update before running commands (CLI restart
    // is the user's next invocation after `update apply`). Package-managed
    // installs are not excluded: `dcc-mcp-cli update` is the supported flow
    // everywhere, and the replacement lands on the running binary. See
    // application::package_manager for the provenance marker and the
    // metadata drift it reports.
    match dcc_mcp_updater::Updater::apply_staged_update(env!("CARGO_PKG_NAME")) {
        Ok(true) => {
            if let Err(e) = refresh_marker_after_update() {
                // Never fatal: the replacement already landed, so failing to
                // re-record it only costs an extra unpack on a later launch.
                eprintln!("warning: could not refresh the package manager marker: {e}");
            }
            eprintln!("info: staged binary update applied; restarting");
            true
        }
        Ok(false) => false,
        Err(e) => {
            eprintln!("warning: failed to apply staged binary update: {e}");
            false
        }
    }
}

/// Keep the package manager marker in sync with a binary that was replaced.
///
/// Only package-managed installs keep a marker, and only those installs have
/// a wrapper that fingerprints the binary, so a direct install has nothing to
/// update and the helper stays quiet.
fn refresh_marker_after_update() -> anyhow::Result<()> {
    let executable = crate::application::current_exe::current_exe()?;
    crate::application::package_manager::record_self_updated_size(&executable)
}

#[derive(Debug, Subcommand)]
enum MarketplaceAction {
    /// Add a marketplace source (raw URL, local file, or GitHub owner/repo).
    Add {
        #[arg(value_name = "SOURCE")]
        source: String,
    },
    /// List configured marketplace sources.
    List,
    /// Search marketplace entries across configured sources.
    Search {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        dcc: Option<String>,
        /// Use this source for the query instead of configured sources.
        #[arg(long = "source")]
        sources: Vec<String>,
        #[arg(long)]
        limit: Option<usize>,
        /// Bypass JSON Schema validation of marketplace entries.
        #[arg(long)]
        skip_validation: bool,
    },
    /// Inspect one marketplace entry by exact name.
    Inspect {
        name: String,
        /// Use this source for the query instead of configured sources.
        #[arg(long = "source")]
        sources: Vec<String>,
        /// Bypass JSON Schema validation of marketplace entries.
        #[arg(long)]
        skip_validation: bool,
    },
    /// Install a marketplace skill package to the local marketplace root.
    Install {
        name: String,
        #[arg(long)]
        dcc: Option<String>,
        /// Use this source for the query instead of configured sources.
        #[arg(long = "source")]
        sources: Vec<String>,
        /// Replace an existing installed package.
        #[arg(long)]
        force: bool,
        /// Bypass JSON Schema validation of marketplace entries.
        #[arg(long)]
        skip_validation: bool,
    },
    /// Remove an installed marketplace skill package.
    Uninstall {
        name: String,
        #[arg(long)]
        dcc: String,
    },
    /// List installed marketplace skill packages.
    ListInstalled {
        #[arg(long)]
        dcc: Option<String>,
    },
    /// List installed packages that have newer versions in the catalog.
    Outdated {
        #[arg(long)]
        dcc: Option<String>,
        /// Only check these package names.
        #[arg(value_name = "NAME")]
        names: Vec<String>,
    },
    /// Upgrade installed marketplace packages to the latest catalog version.
    Update {
        /// Upgrade a specific package by name.
        name: Option<String>,
        /// Upgrade all outdated packages.
        #[arg(long, short = 'a')]
        all: bool,
        /// Filter to installed packages for this DCC.
        #[arg(long)]
        dcc: Option<String>,
    },
    /// Install a skill directly from a GitHub repo (no marketplace.json needed).
    ///
    /// Clones the repo, discovers SKILL.md files, and installs to the
    /// marketplace root. Supports owner/repo, full URL, and @subpath syntax.
    AddRepo {
        /// GitHub owner/repo, full URL, or owner/repo@subpath.
        repo_ref: String,
        /// Override the DCC type (required when SKILL.md doesn't declare one).
        #[arg(long)]
        dcc: Option<String>,
        /// List available skills in the repo without installing.
        #[arg(long)]
        list: bool,
        /// Replace an existing installation.
        #[arg(long)]
        force: bool,
    },
    /// Create a zip package and SHA-256 digest for a local marketplace package.
    Pack(marketplace_cmd::MarketplacePackArgs),
    /// Upsert a package entry into a local marketplace.json catalog.
    Publish(Box<marketplace_cmd::MarketplacePublishArgs>),
}

#[derive(Debug, clap::Args)]
struct LintArgs {
    /// Skill directory or directory tree to scan.
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,

    /// Maximum recursion depth below each PATH.
    #[arg(long, default_value = "2")]
    max_depth: usize,

    /// Exit non-zero when warnings are present.
    #[arg(long, default_value = "false")]
    warnings_as_errors: bool,
}

#[derive(Debug, Subcommand)]
enum UpdateAction {
    /// Check whether a newer version is available.
    Check {
        /// Binary name to check in the gateway update manifest.
        #[arg(long)]
        binary: Option<String>,
        /// Current version to compare against. Defaults to this CLI version.
        #[arg(long)]
        current_version: Option<String>,
    },
    /// Download the latest CLI version and stage it for the next launch.
    Apply,
}

#[derive(Debug, Clone, clap::Args)]
struct GatewayStartArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value = "9765")]
    port: u16,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    registry_dir: Option<PathBuf>,
    #[arg(long, default_value = "0.0.0.0")]
    remote_host: String,
    #[arg(long, default_value = "59765")]
    remote_port: u16,
    #[arg(long, default_value = "0")]
    gateway_idle_timeout_secs: u64,
    #[arg(long)]
    gateway_bin: Option<PathBuf>,
    #[arg(long, default_value = "30")]
    wait_timeout_secs: u64,
}

#[derive(Debug, Clone, clap::Args)]
struct GatewayStopArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value = "9765")]
    port: u16,
    #[arg(long)]
    registry_dir: Option<PathBuf>,
    #[arg(long, default_value = "10")]
    wait_timeout_secs: u64,
}

#[derive(Debug, Clone, clap::Args)]
struct GatewayStatusArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value = "9765")]
    port: u16,
    #[arg(long)]
    registry_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, clap::Args)]
struct GatewayRestartArgs {
    #[command(flatten)]
    start: GatewayStartArgs,
    #[arg(long, default_value = "10")]
    stop_timeout_secs: u64,
}

#[derive(Debug, Subcommand)]
enum GatewayAction {
    /// Register a named remote gateway profile.
    Register {
        /// Gateway base URL, for example https://workstation.example:19293.
        url: String,
        /// Profile name to store.
        #[arg(long)]
        name: String,
    },
    /// List configured remote gateway profiles and the active selection.
    List,
    /// Select the active gateway profile (`local` switches back to local mode).
    Set {
        /// Profile name, or `local`.
        name: String,
    },
    /// Manage the local machine-wide gateway daemon.
    Daemon {
        #[command(subcommand)]
        action: GatewayDaemonAction,
    },
    /// Check gateway reachability; launch if it is not already running.
    Ensure(GatewayStartArgs),
    /// Start the gateway (alias for ensure with pidfile tracking).
    Start(GatewayStartArgs),
    /// Stop the running gateway (PID from pidfile).
    Stop(GatewayStopArgs),
    /// Query gateway health and process status.
    Status(GatewayStatusArgs),
}

#[derive(Debug, Subcommand)]
enum GatewayDaemonAction {
    /// Start the gateway daemon.
    Start(GatewayStartArgs),
    /// Restart the gateway daemon using pidfile-based stop/start.
    Restart(GatewayRestartArgs),
    /// Stop the gateway daemon.
    Stop(GatewayStopArgs),
    /// Query gateway daemon health and PID status.
    Status(GatewayStatusArgs),
}

pub async fn run() -> anyhow::Result<()> {
    apply_staged_update();
    run_with_args(Args::parse()).await
}

fn apply_staged_update() {
    // Apply any staged binary update before running commands (CLI restart
    // is the user's next invocation after `update apply`).
    match dcc_mcp_updater::Updater::apply_staged_update(runtime_cli_name()) {
        Ok(true) => eprintln!("info: staged binary update applied"),
        Ok(false) => { /* no update was staged */ }
        Err(e) => eprintln!("warning: failed to apply staged binary update: {e}"),
    }
    Ok(parsed)
}

fn restart_after_update() -> anyhow::Result<()> {
    let executable = std::env::current_exe()?;
    std::process::Command::new(executable)
        .args(std::env::args_os().skip(1))
        .env("DCC_MCP_UPDATE_JUST_APPLIED", "1")
        .spawn()?;
    Ok(())
}

fn command_shows_maintenance(command: &Command, requested: bool) -> bool {
    requested
        || !matches!(
            command,
            Command::Call { .. }
                | Command::CallBatch { .. }
                | Command::List
                | Command::Search { .. }
                | Command::Describe { .. }
                | Command::Capabilities { .. }
                | Command::DescribeCapabilities { .. }
        )
}

pub(crate) fn runtime_cli_name() -> &'static str {
    let name = std::env::current_exe().ok().and_then(|path| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    });
    match name.as_deref() {
        Some("dcc-mcp-cli") => "dcc-mcp-cli",
        Some("dccmcp") => "dccmcp",
        _ => "dccmcp",
    }
}

async fn run_with_args(args: Args) -> anyhow::Result<()> {
    let Args {
        base_url,
        gateway,
        no_auto_gateway,
        auto_update,
        auto_gateway_bin,
        auto_gateway_timeout_secs,
        output,
        raw_result,
        select,
        non_interactive,
        timeout_secs: global_timeout_secs,
        transport: transport_args,
        command,
    } = args;

    let transport = transport_args.transport;
    let output = match &command {
        Command::Feedback(args) => args.resolve_output(output),
        Command::Install { json: true, .. }
        | Command::Uninstall { json: true, .. }
        | Command::Verify { json: true, .. } => Ok(OutputFormat::Json),
        _ => Ok(output.unwrap_or_else(OutputFormat::auto_detect)),
    };
    let writer = OutputWriter::new(output.map_err(anyhow::Error::msg)?);
    if let Command::Verify {
        dcc_type,
        python,
        catalog,
        dcc_path,
        ..
    } = &command
    {
        let report = InstallService::bundled().verify(InstallRequest {
            dcc_type: dcc_type.clone(),
            version: None,
            catalog_path: catalog.clone(),
            python: Some(python.clone()),
            dcc_path: dcc_path.clone(),
            plugin_source: None,
            adobe_debug_root: None,
        });
        let code = report.exit_code;
        writer.write_data(&result_output::select_fields(to_json(report)?, &select))?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }
    if let Command::Components {
        action: action @ super::components_cmd::ComponentsAction::InstallVerified(_),
    } = command
    {
        // Dispatch before constructing gateway/profile/doctor infrastructure.
        let value = super::components_cmd::run(action).await?;
        return writer.write_data(&result_output::select_fields(value, &select));
    }
    let explicit_update_command = matches!(&command, Command::Update { .. });
    let canonicalize_result =
        !raw_result && matches!(&command, Command::Call { .. } | Command::CallBatch { .. });
    let quiet_control = !command_shows_maintenance(&command, show_maintenance);
    // Staged application already ran in run(), controlled only by the explicit skip flag.
    let output_maintenance =
        UpdateMaintenance::from_skip_flag(no_update_maintenance || quiet_control);
    let marketplace_update_check = output_maintenance
        .run(|| {
            (!crate::application::update::is_background_refresh())
                .then(|| tokio::spawn(check_marketplace_updates()))
        })
        .flatten();

    if global_timeout_secs.is_some()
        && (command_has_distinct_per_timeout(&command, global_timeout_secs)
            || command_has_configured_timeout_env(&command))
    {
        let _ = writer.diagnostic(
            "warning: --timeout-secs is set globally; per-command timeout flags are ignored",
        );
    }

    let profile_path = gateway_profile::default_profile_path();
    let profile_store = gateway_profile::GatewayProfileStore::load(&profile_path)?;
    let gateway_target = profile_store.resolve(gateway.as_deref(), base_url.as_deref())?;
    let endpoint = gateway_target.endpoint_or_default(DEFAULT_BASE_URL);
    let base_url = endpoint.base_url.clone();
    let control = DccControlPlane::new(
        gateway_target.clone(),
        endpoint.clone(),
        None,
        require_gateway,
    )
    .with_auto_gateway_enabled(!no_auto_gateway)
    .with_transport(transport);
    let doctor = DoctorContext::new(
        profile_path.clone(),
        profile_store,
        gateway_target.clone(),
        auto_gateway_bin.clone(),
        !no_auto_gateway,
        require_gateway,
        &endpoint,
    )?;
    if !no_auto_gateway {
        ensure_gateway_for_command(
            &base_url,
            &command,
            &gateway_target,
            auto_gateway_bin.clone(),
            auto_gateway_timeout_secs,
        )
        .await?;
    }

    if auto_update && !matches!(&command, Command::Update { .. }) {
        let service = crate::application::update::UpdateService::new(
            &base_url,
            runtime_cli_name(),
            env!("CARGO_PKG_VERSION"),
        );
        match service.auto_update().await {
            Ok(update) => match update.get("status").and_then(Value::as_str) {
                Some("staged") => eprintln!(
                    "info: a verified {} update was staged for the next launch",
                    runtime_cli_name()
                ),
                Some("pending") => eprintln!(
                    "info: a verified {} update is already staged for the next launch",
                    runtime_cli_name()
                ),
                _ => {}
            },
            Err(error) => eprintln!("warning: automatic CLI update skipped: {error}"),
        }
    }

    let mut failed = false;
    let mut exit_code = ExitCode::GeneralError;
    let mut explicit_exit_code = None;
    let mut human_capabilities_view = None;
    let mut value = match command {
        Command::Smoke {
            url,
            query,
            limit,
            max_pages,
            timeout_secs,
        } => {
            let effective_timeout = global_timeout_secs
                .or(timeout_secs)
                .unwrap_or(DEFAULT_SMOKE_TIMEOUT_SECS);
            let endpoint = url
                .as_deref()
                .map(Endpoint::from_mcp_url)
                .unwrap_or_else(|| Endpoint::new(&base_url));
            let mcp_url = url.as_ref().map(|raw| endpoint_for_mcp(raw));
            let client = DccMcpClient::with_gateway(
                endpoint,
                HttpGateway::with_timeout(Duration::from_secs(effective_timeout.max(1))),
            );
            let result = client
                .smoke(mcp_url, query, limit, usize::from(max_pages))
                .await;
            failed = !result.get("ok").and_then(Value::as_bool).unwrap_or(false);
            if failed {
                exit_code = ExitCode::Unavailable;
            }
            result
        }
        Command::Health => {
            let client = DccMcpClient::new(endpoint.clone());
            client.health().await?
        }
        Command::Stats {
            range,
            dcc_type,
            skill,
            tool,
            status,
            instance_id,
            session_id,
        } => {
            control
                .stats(StatsRequest {
                    range,
                    dcc_type,
                    skill,
                    tool,
                    status,
                    instance_id,
                    session_id,
                })
                .await?
        }
        Command::Feedback(args) => args.run(&control, &doctor).await?,
        Command::Doctor {
            registry_dir,
            gateway_host,
            gateway_port,
            adapter_python,
            adapter_catalog,
        } => {
            let overrides = parse_adapter_python(&adapter_python)?;
            let value = run_doctor(doctor.request_with_adapter_probes(
                registry_dir,
                Some(gateway_host),
                Some(gateway_port),
                overrides,
                adapter_catalog,
            ))
            .await?;
            // A broken adapter install is a real finding: surface it through the
            // exit code instead of reporting `ok` for an unusable host.
            failed =
                adapter_import::has_failures(value.get("adapter_imports").unwrap_or(&Value::Null));
            value
        }
        Command::List => control.list_instances().await?,
        Command::Capabilities {
            dcc_type,
            instance_id,
            keys,
            force_refresh,
            invalidate,
        }
        | Command::DescribeCapabilities {
            dcc_type,
            instance_id,
            keys,
            force_refresh,
            invalidate,
        } => {
            let effective_timeout = global_timeout_secs
                .unwrap_or(DEFAULT_CALL_TIMEOUT_SECS)
                .max(1);
            let meta = attach_agent_session_id(None, agent_session_id.as_deref())?;
            let result = CapabilitiesCommand::new(
                dcc_type,
                instance_id,
                keys,
                force_refresh,
                invalidate,
                meta,
            )
            .run(&control, Duration::from_secs(effective_timeout))
            .await?;
            failed = !capabilities_result::command_succeeded(&result);
            if failed {
                exit_code = ExitCode::Unavailable;
            }
            let result = capabilities_result::project_result(result);
            if writer.format() == OutputFormat::Human {
                human_capabilities_view = Some(capabilities_output::render_human(&result));
            }
            result
        }
        Command::DccTypes {
            catalog,
            dcc_type,
            offline,
            project,
        } => {
            dcc_types_output::run(
                catalog.as_deref(),
                dcc_type.as_deref(),
                offline,
                project.as_deref(),
            )
            .await?
        }
        Command::Discover { dcc_type, paths } => {
            to_json(discover_dcc_installations(dcc_type.as_deref(), &paths))?
        }
        Command::Search {
            query,
            query_terms,
            dcc_type,
            instance_id,
            limit,
            gateway_only,
        } => {
            let request = SearchRequest {
                query: resolve_query(query, query_terms),
                dcc_type,
                instance_id,
                limit,
                gateway_only,
            };
            control.search(request).await?
        }
        Command::Describe { tool_slug } => control.describe(tool_slug).await?,
        Command::LoadSkill {
            skill_name,
            dcc_type,
            dcc,
            instance_id,
            activate_groups,
            request_json,
        } => {
            let request = build_load_skill_request(
                skill_name,
                dcc_type,
                dcc,
                instance_id,
                activate_groups,
                request_json,
            )?;
            control.load_skill(request).await?
        }
        Command::Call {
            tool_slug,
            batch,
            steps,
            dcc_type,
            instance_id,
            arguments_json,
            json_file,
            meta_json,
            wait,
            wait_timeout_secs,
            timeout_secs,
        } => {
            let effective_timeout = global_timeout_secs
                .or(timeout_secs)
                .unwrap_or(DEFAULT_CALL_TIMEOUT_SECS);
            let mut result = if batch {
                let mut request =
                    read_batch_request(&arguments_json, steps.as_deref(), json_file.as_deref())?;
                attach_batch_agent_session_id(&mut request, agent_session_id.as_deref())?;
                control
                    .call_batch(request, Duration::from_secs(effective_timeout.max(1)))
                    .await?
            } else {
                let tool_slug = tool_slug
                    .filter(|slug| !slug.trim().is_empty())
                    .context("call requires TOOL_SLUG unless --batch is provided")?;
                let arguments = read_call_arguments(&arguments_json, json_file.as_deref())?;
                let meta = meta_json
                    .as_deref()
                    .map(|raw| parse_json_object(raw, "--meta-json"))
                    .transpose()?;
                let meta = attach_agent_session_id(meta, agent_session_id.as_deref())?;
                let request_timeout = Duration::from_secs(effective_timeout.max(1));
                if wait {
                    let mut progress = JobProgressReporter::default();
                    control
                        .call_and_wait_with_progress(
                            tool_slug,
                            dcc_type,
                            instance_id,
                            arguments,
                            meta,
                            request_timeout,
                            Duration::from_secs(wait_timeout_secs.max(1)),
                            |update| {
                                if let Some(line) = progress.next_line(update, Instant::now()) {
                                    let _ = writer.diagnostic(&line);
                                }
                            },
                        )
                        .await?
                } else {
                    control
                        .call(
                            tool_slug,
                            dcc_type,
                            instance_id,
                            arguments,
                            meta,
                            request_timeout,
                        )
                        .await?
                }
            };
            materialize_call_images(&mut result, &default_image_artifact_root());
            failed = !crate::application::local_control::call_result_succeeded(&result);
            if failed {
                exit_code = ExitCode::GeneralError;
            }
            result
        }
        Command::CallBatch {
            request_json,
            json_file,
            timeout_secs,
        } => {
            let effective_timeout = global_timeout_secs
                .or(timeout_secs)
                .unwrap_or(DEFAULT_CALL_TIMEOUT_SECS);
            let mut request = read_call_arguments(&request_json, json_file.as_deref())?;
            attach_batch_agent_session_id(&mut request, agent_session_id.as_deref())?;
            let mut result = control
                .call_batch(request, Duration::from_secs(effective_timeout.max(1)))
                .await?;
            materialize_call_images(&mut result, &default_image_artifact_root());
            failed = !crate::application::local_control::call_result_succeeded(&result);
            if failed {
                exit_code = ExitCode::GeneralError;
            }
            result
        }
        Command::UiControl { action } => {
            let effective_timeout = action.effective_timeout_secs(global_timeout_secs);
            let (tool_name, args) = action.into_call();
            let full_output = args.full_output;
            let arguments = read_call_arguments(&args.arguments_json, args.json_file.as_deref())?;
            let meta = args
                .meta_json
                .as_deref()
                .map(|raw| parse_json_object(raw, "--meta-json"))
                .transpose()?;
            let meta = attach_agent_session_id(meta, agent_session_id.as_deref())?;
            let mut result = control
                .call(
                    tool_name.to_string(),
                    args.dcc_type,
                    args.instance_id,
                    arguments,
                    meta,
                    Duration::from_secs(effective_timeout.max(1)),
                )
                .await?;
            materialize_call_images(&mut result, &default_image_artifact_root());
            failed = !crate::application::local_control::call_result_succeeded(&result);
            if failed {
                exit_code = ExitCode::GeneralError;
            }
            if full_output {
                result
            } else {
                compact_ui_control_result(tool_name, &result)
            }
        }
        Command::RecordReplay { action } => {
            let result =
                run_record_replay(action, agent_session_id.as_deref(), &endpoint, &control).await?;
            failed = result.failed;
            if failed {
                exit_code = ExitCode::GeneralError;
            }
            result.value
        }
        Command::WaitReady {
            dcc_type,
            instance_id,
            require,
            timeout_secs,
            interval_secs,
        } => {
            let effective_timeout = global_timeout_secs
                .or(timeout_secs)
                .unwrap_or(DEFAULT_WAIT_READY_TIMEOUT_SECS);
            let request = WaitReadyRequest {
                dcc_type,
                instance_id,
                required: require,
                timeout: Duration::from_secs(effective_timeout),
                interval: Duration::from_secs(interval_secs.max(1)),
            };
            let result = control.wait_ready(request).await?;
            failed = !result
                .get("ready")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if failed {
                exit_code = ExitCode::Timeout;
            }
            result
        }
        Command::ReloadSkills {
            dcc_type,
            instance_id,
        } => {
            let request = ReloadSkillsRequest {
                dcc_type,
                instance_id,
            };
            let result = control.reload_skills(request).await?;
            failed = !result.get("ok").and_then(Value::as_bool).unwrap_or(false);
            if failed {
                exit_code = ExitCode::Unavailable;
            }
            result
        }
        Command::StopInstance {
            dcc_type,
            instance_id,
            expected_owner,
            expected_session,
            operation_id,
        } => {
            // A stop scoped to a lifecycle operation may only touch the
            // instance that operation launched and owns. Any supplied
            // --operation-id must resolve to a locally owned operation before
            // it constrains the stop: the local path re-checks ownership in
            // stop_instance_local, but the remote path would otherwise forward
            // a caller-supplied operation id that nothing has validated.
            // Resolving first also means the routing pair comes from the
            // operation record rather than straight from the caller, so
            // supplying both --dcc-type and --instance-id can no longer
            // short-circuit the ownership check.
            let owned_operation = if let Some(operation_id) =
                operation_id.as_deref().filter(|id| !id.trim().is_empty())
            {
                crate::application::instance_launch::resolve_owned_operation(
                    &gateway_ensure::default_registry_dir()?,
                    dcc_type.as_deref(),
                    instance_id.as_deref(),
                    Some(operation_id),
                )?
            } else {
                None
            };
            let (resolved_dcc_type, resolved_instance_id) = match owned_operation {
                Some(operation) => {
                    let instance_id = operation.instance_id.clone().ok_or_else(|| {
                        anyhow!(
                            "--operation-id resolves to an operation without a registered instance"
                        )
                    })?;
                    (operation.dcc_type.clone(), instance_id)
                }
                None => match (dcc_type.as_deref(), instance_id.as_deref()) {
                    (Some(dcc_type), Some(instance_id)) => {
                        (dcc_type.to_string(), instance_id.to_string())
                    }
                    _ => anyhow::bail!(
                        "stop-instance requires both --dcc-type and --instance-id, or an --operation-id that owns a registered instance"
                    ),
                },
            };
            let request = StopInstanceRequest {
                dcc_type: resolved_dcc_type,
                instance_id: resolved_instance_id,
                expected_owner,
                expected_session,
                operation_id,
            };
            control.stop_instance(request).await?
        }
        Command::StartInstance {
            dcc_type,
            project,
            launch_plan,
            version,
            wait_ready,
            require,
            timeout_secs,
            interval_secs,
            dry_run,
            yes,
            instance_id,
        } => {
            let effective_timeout = global_timeout_secs
                .or(timeout_secs)
                .unwrap_or(DEFAULT_START_INSTANCE_TIMEOUT_SECS);
            let request = StartInstanceRequest {
                dcc_type,
                project,
                launch_plan,
                version,
                wait_ready,
                timeout: Duration::from_secs(effective_timeout),
                interval: Duration::from_secs(interval_secs.max(1)),
                required: require,
                authorized: yes,
                dry_run,
                registry_dir: gateway_ensure::default_registry_dir()?,
                instance_id,
            };
            let result = crate::application::instance_launch::start_instance(request).await?;
            if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                failed = true;
                exit_code = ExitCode::Unavailable;
            } else if wait_ready
                && !dry_run
                && !result
                    .get("ready")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                // A dry run never spawns a host, so it reports `ready: false` by
                // design. Waiting on it would turn a successful plan resolution
                // into a spurious timeout exit code.
                failed = true;
                exit_code = ExitCode::Timeout;
            }
            result
        }
        Command::Verify { .. } => anyhow::bail!("verify must run before control-plane setup"),
        Command::Install {
            offline,
            dcc_type,
            version,
            catalog,
            python,
            dcc_path,
            plugin_source,
            adobe_debug_root,
            execute,
            json,
        } => {
            let service = if catalog.is_some() {
                InstallService::bundled()
            } else {
                InstallService::refreshed(offline).await
            };
            let req = InstallRequest {
                dcc_type,
                version,
                catalog_path: catalog,
                python,
                dcc_path,
                plugin_source,
                adobe_debug_root,
            };
            if execute {
                // `--execute` is the mutation opt-in; JSON mode must never add
                // an interactive prompt to the machine-readable contract.
                let report = service.execute(req, non_interactive || json);
                explicit_exit_code = Some(report.exit_code);
                to_json(report)?
            } else {
                to_json(service.plan(req)?)?
            }
        }
        Command::Uninstall {
            offline,
            dcc_type,
            version,
            catalog,
            python,
            dcc_path,
            plugin_source,
            adobe_debug_root,
            yes,
            json,
        } => {
            let service = if catalog.is_some() {
                InstallService::bundled()
            } else {
                InstallService::refreshed(offline).await
            };
            let report = service.uninstall(
                InstallRequest {
                    dcc_type,
                    version,
                    catalog_path: catalog,
                    python,
                    dcc_path,
                    plugin_source,
                    adobe_debug_root,
                },
                non_interactive || json || yes,
            );
            explicit_exit_code = Some(report.exit_code);
            to_json(report)?
        }
        Command::Marketplace { action } => {
            let service = new_service()?;
            match action {
                MarketplaceAction::Add { source } => to_json(service.add_source(&source)?)?,
                MarketplaceAction::List => to_json(service.list_sources()?)?,
                MarketplaceAction::Search {
                    query,
                    query_terms,
                    dcc,
                    target,
                    sources,
                    limit,
                    skip_validation,
                } => {
                    let query = resolve_query(query, query_terms);
                    if let Some(target) = target {
                        let target = parse_marketplace_target(&target)?;
                        to_json(
                            service
                                .search_for_target(query, target, sources, limit, skip_validation)
                                .await?,
                        )?
                    } else {
                        to_json(
                            service
                                .search(query, dcc, sources, limit, skip_validation)
                                .await?,
                        )?
                    }
                }
                MarketplaceAction::Inspect {
                    name,
                    sources,
                    skip_validation,
                } => to_json(service.inspect(name, sources, skip_validation).await?)?,
                MarketplaceAction::Install {
                    name,
                    dcc,
                    target,
                    reload,
                    sources,
                    force,
                    skip_validation,
                } => {
                    let installed = if let Some(target) = target {
                        service
                            .install_for_target(
                                name,
                                parse_marketplace_target(&target)?,
                                sources,
                                force,
                                skip_validation,
                            )
                            .await?
                    } else {
                        service
                            .install(name, dcc, sources, force, skip_validation)
                            .await?
                    };
                    // A shared install is loaded by every host, so the reload
                    // targets all of them. The pseudo-host `any` would match no
                    // live instance: the selectors compare host names exactly.
                    let installed_dcc = (!dcc_mcp_marketplace::is_host_neutral_dcc(&installed.dcc))
                        .then(|| installed.dcc.clone());
                    let skill_reload = installed.activation
                        == dcc_mcp_marketplace::MarketplaceActivation::SkillReload;
                    if !installed.superseded.is_empty() {
                        eprintln!(
                            "note: {} installs into the shared directory; per-host copies still \
                             shadow it and still cost disk: {}",
                            installed.name,
                            installed.superseded.join(", ")
                        );
                        eprintln!(
                            "      remove them with: dcc-mcp-cli marketplace uninstall {} --dcc \
                             <host>",
                            installed.name
                        );
                    }
                    let mut value = to_json(installed)?;
                    if reload && skill_reload {
                        let (reloaded_value, reload_failed) =
                            reload_marketplace_value(&control, value, installed_dcc).await;
                        value = reloaded_value;
                        if reload_failed {
                            failed = true;
                            exit_code = ExitCode::Unavailable;
                        }
                    }
                    value
                }
                MarketplaceAction::Uninstall {
                    name,
                    dcc,
                    target,
                    reload,
                } => {
                    let requested_target = target
                        .as_deref()
                        .map(parse_marketplace_target)
                        .transpose()?;
                    let installed_target = if requested_target.is_some() || dcc.is_none() {
                        service.resolve_installed_target(&name, requested_target.as_ref())?
                    } else {
                        dcc_mcp_catalog::CatalogTarget {
                            kind: dcc_mcp_catalog::CatalogTargetKind::Dcc,
                            id: dcc.clone().unwrap_or_default(),
                        }
                    };
                    // Host-neutral packages are recorded under `any`, which no
                    // live instance is named; refresh every host instead.
                    let installed_dcc =
                        (!dcc_mcp_marketplace::is_host_neutral_dcc(&installed_target.id))
                            .then(|| installed_target.id.clone());
                    let result = service.uninstall_for_target(&name, &installed_target)?;
                    let skill_reload = result.activation
                        == dcc_mcp_marketplace::MarketplaceActivation::SkillReload;
                    let mut value = to_json(result)?;
                    if reload && skill_reload {
                        let (reloaded_value, reload_failed) =
                            reload_marketplace_value(&control, value, installed_dcc).await;
                        value = reloaded_value;
                        if reload_failed {
                            failed = true;
                            exit_code = ExitCode::Unavailable;
                        }
                    }
                    value
                }
                MarketplaceAction::ListInstalled { dcc, target } => {
                    if let Some(target) = target {
                        let target = parse_marketplace_target(&target)?;
                        to_json(service.list_installed_for_target(Some(&target))?)?
                    } else {
                        to_json(service.list_installed(dcc.as_deref())?)?
                    }
                }
                MarketplaceAction::Outdated { dcc, names } => {
                    to_json(service.outdated(dcc.as_deref(), names).await?)?
                }
                MarketplaceAction::Update { name, all, dcc } => {
                    to_json(service.update(name, all, dcc).await?)?
                }
                MarketplaceAction::AddRepo {
                    repo_ref,
                    commit,
                    dcc,
                    list,
                    force,
                } => {
                    if list {
                        to_json(service.list_repo_skills(&repo_ref)?)?
                    } else {
                        let commit = commit.expect("clap requires --commit unless --list is set");
                        to_json(service.add_repo_at_commit(
                            &repo_ref,
                            &commit,
                            dcc.as_deref(),
                            force,
                        )?)?
                    }
                }
                MarketplaceAction::Pack(args) => marketplace_cmd::run_pack(args)?,
                MarketplaceAction::Publish(args) => marketplace_cmd::run_publish(*args)?,
            }
        }
        Command::Host { action } => {
            let result = super::host_cmd::run(&action)?;
            failed = result.failed;
            if failed {
                exit_code = ExitCode::Unavailable;
            }
            result.value
        }
        Command::Update { action } => match action.unwrap_or(UpdateAction::Apply) {
            UpdateAction::Check {
                binary,
                current_version,
            } => {
                let binary_name = binary.unwrap_or_else(|| runtime_cli_name().to_string());
                let current_version =
                    current_version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
                let service = crate::application::update::UpdateService::new(
                    &base_url,
                    &binary_name,
                    &current_version,
                );
                let value = service.check_update().await?;
                if value.get("error").is_some() {
                    failed = true;
                }
                to_json(value)?
            }
            UpdateAction::Apply => {
                let service = crate::application::update::UpdateService::new(
                    &base_url,
                    runtime_cli_name(),
                    env!("CARGO_PKG_VERSION"),
                );
                to_json(service.apply_update().await?)?
            }
        },
        Command::Gateway { action, daemon } => {
            if let Some(action) = action {
                to_json(run_gateway_cmd(&base_url, action, &profile_path).await?)?
            } else {
                if daemon.restart {
                    dcc_mcp_sidecar::gateway_daemon::restart_gateway(&daemon).await?;
                } else {
                    dcc_mcp_sidecar::gateway_daemon::run(daemon).await?;
                }
                return Ok(());
            }
        }
        Command::Mcp { action } => run_mcp_cmd(action, &base_url).await?,
    };

    print_value(&value, output)?;
    if failed {
        std::process::exit(1);
    }
    Ok(())
}

async fn run_mcp_cmd(action: McpAction, default_base_url: &str) -> anyhow::Result<Value> {
    match action {
        McpAction::Initialize(args) => {
            let mut client = new_mcp_client(&args.connection, default_base_url)?
                .with_client_info(args.client_name, args.client_version);
            to_json(client.initialize().await?)
        }
        McpAction::Tools { action } => match action {
            McpToolsAction::List(args) => {
                let mut client = new_mcp_client(&args, default_base_url)?;
                let tools = client.tools_list_all().await?;
                Ok(json!({
                    "count": tools.len(),
                    "tools": tools,
                }))
            }
            McpToolsAction::Call(args) => {
                let arguments = parse_json_object(&args.arguments_json, "--json")?;
                let meta = args
                    .meta_json
                    .as_deref()
                    .map(|raw| parse_json_object(raw, "--meta"))
                    .transpose()?;
                let mut client = new_mcp_client(&args.connection, default_base_url)?;
                to_json(client.tools_call(args.name, arguments, meta).await?)
            }
        },
        McpAction::Resources { action } => match action {
            McpResourcesAction::List(args) => {
                let mut client = new_mcp_client(&args, default_base_url)?;
                let resources = client.resources_list_all().await?;
                Ok(json!({
                    "count": resources.len(),
                    "resources": resources,
                }))
            }
            McpResourcesAction::Read(args) => {
                let mut client = new_mcp_client(&args.connection, default_base_url)?;
                to_json(client.resources_read(args.uri).await?)
            }
        },
        McpAction::Prompts { action } => match action {
            McpPromptsAction::List(args) => {
                let mut client = new_mcp_client(&args, default_base_url)?;
                let prompts = client.prompts_list_all().await?;
                Ok(json!({
                    "count": prompts.len(),
                    "prompts": prompts,
                }))
            }
            McpPromptsAction::Get(args) => {
                let arguments = parse_json_object(&args.arguments_json, "--arguments-json")?
                    .as_object()
                    .cloned()
                    .context("--arguments-json must be a JSON object")?;
                let mut client = new_mcp_client(&args.connection, default_base_url)?;
                to_json(client.prompts_get(args.name, arguments).await?)
            }
        },
    }
}

fn new_mcp_client(
    connection: &McpConnectionArgs,
    default_base_url: &str,
) -> anyhow::Result<McpHttpClient> {
    let raw_url = connection.url.as_deref().unwrap_or(default_base_url);
    let mcp_url = endpoint_for_mcp(raw_url);
    Ok(McpHttpClient::with_timeout(
        mcp_url,
        Duration::from_secs(connection.timeout_secs.max(1)),
    )?)
}

struct LintCommandResult {
    value: Value,
    failed: bool,
}

fn collect_skill_dirs(
    root: &std::path::Path,
    out: &mut BTreeSet<PathBuf>,
    max_depth: usize,
) -> anyhow::Result<()> {
    collect_skill_dirs_at(root, out, max_depth, 0)
}

fn collect_skill_dirs_at(
    root: &std::path::Path,
    out: &mut BTreeSet<PathBuf>,
    max_depth: usize,
    depth: usize,
) -> anyhow::Result<()> {
    if root.join("SKILL.md").is_file() {
        out.insert(root.to_path_buf());
        return Ok(());
    }

    if !root.is_dir() {
        anyhow::bail!(
            "skill lint path does not exist or is not a directory: {}",
            root.display()
        );
    }
    if depth >= max_depth {
        return Ok(());
    }

    let entries = std::fs::read_dir(root).map_err(|err| {
        anyhow::anyhow!("cannot read skill lint path '{}': {err}", root.display())
    })?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        collect_skill_dirs_at(&path, out, max_depth, depth + 1)?;
    }
    Ok(())
}

fn issue_severity_label(severity: IssueSeverity) -> &'static str {
    match severity {
        IssueSeverity::Error => "error",
        IssueSeverity::Warning => "warning",
    }
}

fn lint_report_to_json(report: &SkillValidationReport) -> Value {
    let (errors, warnings) = report.counts();
    let issues: Vec<_> = report
        .issues
        .iter()
        .map(|issue| {
            serde_json::json!({
                "severity": issue_severity_label(issue.severity),
                "category": format!("{:?}", issue.category),
                "message": issue.message,
            })
        })
        .collect();
    serde_json::json!({
        "skill_dir": report.skill_dir.display().to_string(),
        "errors": errors,
        "warnings": warnings,
        "issues": issues,
    })
}

fn run_lint_cmd(args: &LintArgs) -> anyhow::Result<LintCommandResult> {
    let mut skill_dirs = BTreeSet::new();
    for root in &args.paths {
        collect_skill_dirs(root, &mut skill_dirs, args.max_depth)?;
    }

    let reports: Vec<_> = skill_dirs
        .iter()
        .map(|skill_dir| validate_skill_dir(skill_dir))
        .collect();
    let (errors, warnings) = reports.iter().fold((0, 0), |(e_acc, w_acc), report| {
        let (errors, warnings) = report.counts();
        (e_acc + errors, w_acc + warnings)
    });
    let failed = errors > 0 || (args.warnings_as_errors && warnings > 0);
    let reports_json: Vec<_> = reports.iter().map(lint_report_to_json).collect();
    let value = serde_json::json!({
        "checked": reports.len(),
        "errors": errors,
        "warnings": warnings,
        "failed": failed,
        "reports": reports_json,
    });

    Ok(LintCommandResult { value, failed })
}

async fn ensure_gateway_for_command(
    base_url: &str,
    command: &Command,
    gateway_target: &GatewayTarget,
    gateway_bin: Option<PathBuf>,
    wait_timeout_secs: u64,
) -> anyhow::Result<()> {
    let Some(endpoint) = gateway_endpoint_for_command(base_url, command, gateway_target) else {
        return Ok(());
    };
    let Some(result) =
        gateway_ctrl::ensure_local_gateway_for_endpoint(&endpoint, gateway_bin, wait_timeout_secs)
            .await?
    else {
        return Ok(());
    };

    if !result.already_running {
        if let Some(pid) = result.pid {
            eprintln!(
                "info: auto-started gateway at http://{}:{} (pid {pid})",
                result.host, result.port
            );
        } else {
            eprintln!(
                "info: auto-started gateway at http://{}:{}",
                result.host, result.port
            );
        }
    }
    Ok(())
}

fn gateway_endpoint_for_command(
    base_url: &str,
    command: &Command,
    _gateway_target: &GatewayTarget,
) -> Option<Endpoint> {
    match command {
        Command::Smoke { url: None, .. } => Some(Endpoint::new(base_url)),
        Command::Smoke { url: Some(_), .. } => None,
        Command::Health | Command::Stats { .. } | Command::Update { .. } => {
            Some(Endpoint::new(base_url))
        }
        Command::Doctor { .. } | Command::DccTypes { .. } | Command::Discover { .. } => None,
        Command::List
        | Command::Search { .. }
        | Command::Describe { .. }
        | Command::LoadSkill { .. }
        | Command::Call { .. }
        | Command::CallBatch { .. }
        | Command::UiControl { .. }
        | Command::WaitReady { .. }
        | Command::ReloadSkills { .. }
        | Command::StopInstance { .. } => Some(Endpoint::new(base_url)),
        Command::Mcp { action } if mcp_action_url(action).is_none() => {
            Some(Endpoint::new(base_url))
        }
        Command::Mcp { .. } => None,
        // Local mode still executes these commands through FileRegistry/direct
        // MCP where that is the richer path, but the CLI owns gateway
        // lifecycle by default so agents can rely on the admin/control plane.
        Command::Install { .. }
        | Command::Marketplace { .. }
        | Command::Lint(_)
        | Command::Gateway { .. } => None,
    }
}

fn mcp_action_url(action: &McpAction) -> Option<&str> {
    match action {
        McpAction::Initialize(args) => args.connection.url.as_deref(),
        McpAction::Tools { action } => match action {
            McpToolsAction::List(args) => args.url.as_deref(),
            McpToolsAction::Call(args) => args.connection.url.as_deref(),
        },
        McpAction::Resources { action } => match action {
            McpResourcesAction::List(args) => args.url.as_deref(),
            McpResourcesAction::Read(args) => args.connection.url.as_deref(),
        },
        McpAction::Prompts { action } => match action {
            McpPromptsAction::List(args) => args.url.as_deref(),
            McpPromptsAction::Get(args) => args.connection.url.as_deref(),
        },
    }
}

async fn run_gateway_cmd(
    _base_url: &str,
    action: GatewayAction,
    profile_path: &std::path::Path,
) -> anyhow::Result<Value> {
    match action {
        GatewayAction::Register { url, name } => {
            gateway_profile::register_profile(profile_path, name, url)
        }
        GatewayAction::List => gateway_profile::list_profiles(profile_path),
        GatewayAction::Set { name } => gateway_profile::set_current_profile(profile_path, name),
        GatewayAction::Daemon { action } => {
            gateway_ctrl::run_gateway_daemon(gateway_daemon_request(action)).await
        }
        GatewayAction::Ensure(args) => {
            let request = gateway_ctrl::GatewayDaemonStartRequest::from(args);
            let reg = request
                .registry_dir
                .clone()
                .unwrap_or_else(gateway_ensure::default_registry_dir);
            let args = gateway_ensure::EnsureGatewayArgs {
                host: request.host,
                port: request.port,
                name: request.name,
                registry_dir: reg,
                remote_host: request.remote_host,
                remote_port: request.remote_port,
                gateway_idle_timeout_secs: request.gateway_idle_timeout_secs,
                gateway_bin: request.gateway_bin,
                wait_timeout_secs: request.wait_timeout_secs,
                pidfile: None,
            };
            let result = gateway_ensure::ensure_gateway_running(&args).await?;
            Ok(serde_json::to_value(result)?)
        }
        GatewayAction::Start(args) => {
            gateway_ctrl::run_gateway_daemon(gateway_ctrl::GatewayDaemonRequest::Start(args.into()))
                .await
        }
        GatewayAction::Stop(args) => {
            gateway_ctrl::run_gateway_daemon(gateway_ctrl::GatewayDaemonRequest::Stop(args.into()))
                .await
        }
        GatewayAction::Status(args) => {
            gateway_ctrl::run_gateway_daemon(gateway_ctrl::GatewayDaemonRequest::Status(
                args.into(),
            ))
            .await
        }
    }
}

fn gateway_daemon_request(action: GatewayDaemonAction) -> gateway_ctrl::GatewayDaemonRequest {
    match action {
        GatewayDaemonAction::Start(args) => gateway_ctrl::GatewayDaemonRequest::Start(args.into()),
        GatewayDaemonAction::Restart(args) => gateway_ctrl::GatewayDaemonRequest::Restart {
            start: args.start.into(),
            stop_timeout_secs: args.stop_timeout_secs,
        },
        GatewayDaemonAction::Stop(args) => gateway_ctrl::GatewayDaemonRequest::Stop(args.into()),
        GatewayDaemonAction::Status(args) => {
            gateway_ctrl::GatewayDaemonRequest::Status(args.into())
        }
    }
}

impl From<GatewayStartArgs> for gateway_ctrl::GatewayDaemonStartRequest {
    fn from(args: GatewayStartArgs) -> Self {
        Self {
            host: args.host,
            port: args.port,
            name: args.name,
            registry_dir: args.registry_dir,
            remote_host: args.remote_host,
            remote_port: args.remote_port,
            gateway_idle_timeout_secs: args.gateway_idle_timeout_secs,
            gateway_bin: args.gateway_bin,
            wait_timeout_secs: args.wait_timeout_secs,
        }
    }
}

impl From<GatewayStopArgs> for gateway_ctrl::GatewayDaemonStopRequest {
    fn from(args: GatewayStopArgs) -> Self {
        Self {
            host: args.host,
            port: args.port,
            registry_dir: args.registry_dir,
            wait_timeout_secs: args.wait_timeout_secs,
        }
    }
}

impl From<GatewayStatusArgs> for gateway_ctrl::GatewayDaemonStatusRequest {
    fn from(args: GatewayStatusArgs) -> Self {
        Self {
            host: args.host,
            port: args.port,
            registry_dir: args.registry_dir,
        }
    }
}

fn parse_json_object(raw: &str, flag_name: &str) -> anyhow::Result<Value> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{flag_name} must be valid JSON"))?;
    if value.is_object() {
        Ok(value)
    } else {
        anyhow::bail!("{flag_name} must be a JSON object")
    }
}

fn read_call_arguments(raw: &str, json_file: Option<&std::path::Path>) -> anyhow::Result<Value> {
    let Some(path) = json_file else {
        return parse_json_object(raw, "--json");
    };
    let contents = if path == std::path::Path::new("-") {
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .context("failed to read --json-file - from stdin")?;
        input
    } else {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read --json-file {}", path.display()))?
    };
    parse_json_object(&contents, "--json-file")
}

fn read_batch_request(
    raw: &str,
    steps: Option<&str>,
    json_file: Option<&std::path::Path>,
) -> anyhow::Result<Value> {
    if let Some(raw_steps) = steps {
        let calls: Value =
            serde_json::from_str(raw_steps).context("--steps must be a valid JSON array")?;
        if !calls.is_array() {
            anyhow::bail!("--steps must be a JSON array");
        }
        return Ok(serde_json::json!({"calls": calls}));
    }

    let request = read_call_arguments(raw, json_file)?;
    if request.get("calls").and_then(Value::as_array).is_none() {
        anyhow::bail!(
            "call --batch requires --steps JSON_ARRAY or a --json/--json-file object containing calls"
        );
    }

    output_maintenance
        .run(|| {
            super::update_cmd::surface_cached_cli_update(
                &mut value,
                &writer,
                &base_url,
                !explicit_update_command,
            )
        })
        .transpose()?;

    let failure = failed.then(|| failure_envelope(&value, exit_code));
    if let Some(rendered) = human_capabilities_view {
        if let Some(envelope) = failure {
            writer.write_error(&envelope)?;
            std::process::exit(exit_code.as_i32());
        }
        return writer.write_human(&rendered);
    }
    if canonicalize_result {
        result_output::canonical_result(&mut value);
    }
    let value = result_output::select_fields(value, &select);
    writer.write_data(&value)?;
    if let Some(code) = explicit_exit_code
        && code != 0
    {
        std::process::exit(code);
    }
    if let Some(envelope) = failure {
        writer.write_error(&envelope)?;
        std::process::exit(exit_code.as_i32());
    }
    Ok(())
}

#[cfg(test)]
#[path = "cli/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cli/confirmation_timeout_tests.rs"]
mod confirmation_timeout_tests;

#[cfg(test)]
#[path = "cli/gateway_precedence_tests.rs"]
mod gateway_precedence_tests;

#[cfg(test)]
#[path = "cli/verified_component_tests.rs"]
mod verified_component_tests;
