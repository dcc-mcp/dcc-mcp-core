//! CLI surface declaration and argument parsing for `dcc-mcp-cli`.
//!
//! This module owns the clap command tree and the pure helpers that turn raw
//! flag values into domain requests. Command execution stays in `cli`.

use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, anyhow};
use clap::Subcommand;
use serde_json::{Map, Value};

use crate::domain::rest::LoadSkillRequest;

use super::record_replay::RecordReplayAction;
use crate::presentation::feedback_cmd::FeedbackArgs;
use crate::presentation::marketplace_cmd::MarketplaceAction;
use crate::presentation::output::OutputFormat;

pub(crate) fn parse_output_format(s: &str) -> Result<OutputFormat, String> {
    OutputFormat::from_flag(s)
}

// clap keeps flattened command arguments by value; this parser enum is short-lived.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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
        #[arg(long)]
        timeout_secs: Option<u64>,
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
    /// File structured feedback at the gateway, including after a DCC instance exits.
    Feedback(FeedbackArgs),
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
        /// Use a still-valid signed cache or the bundled catalog without checking online.
        #[arg(long, env = "DCC_MCP_INSTALL_OFFLINE")]
        offline: bool,
        /// Read a custom adapter catalog instead of the release catalog.
        #[arg(long, env = "DCC_MCP_CATALOG_PATH")]
        catalog: Option<PathBuf>,
        /// Resolve one DCC's catalog and live-instance states independently.
        #[arg(long)]
        dcc_type: Option<String>,
        /// Absolute project root. Lets a zero-instance decision recommend
        /// `start-instance` when the adapter published a launch plan.
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
    },
    /// Search callable tools, or list the complete loaded inventory when no query is provided.
    Search {
        /// Query text. Positional words are also accepted, for example `search create sphere`.
        #[arg(short, long, conflicts_with = "query_terms")]
        query: Option<String>,
        /// Unquoted positional query words joined with spaces.
        #[arg(value_name = "QUERY", num_args = 1.., conflicts_with = "query")]
        query_terms: Vec<String>,
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
            conflicts_with_all = ["tool_slug", "dcc_type", "instance_id", "meta_json", "wait"]
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
        /// Poll an asynchronous job through the same DCC route until it reaches a terminal state.
        #[arg(long, conflicts_with = "batch")]
        wait: bool,
        /// Maximum total time to wait for an asynchronous job.
        #[arg(
            long,
            default_value = "600",
            requires = "wait",
            conflicts_with = "batch"
        )]
        wait_timeout_secs: u64,
        /// Per-request timeout for the tool call. Increase for renders and other long-running sync tools.
        #[arg(long, env = "DCC_MCP_CLI_CALL_TIMEOUT_SECS")]
        timeout_secs: Option<u64>,
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
        #[arg(long, env = "DCC_MCP_CLI_CALL_TIMEOUT_SECS")]
        timeout_secs: Option<u64>,
    },
    /// Run the scoped DCC UI Control fallback through stable ui-control tools.
    UiControl {
        #[command(subcommand)]
        action: UiControlAction,
    },
    /// Record, review, compile, and explicitly replay a demonstrated workflow.
    RecordReplay {
        #[command(subcommand)]
        action: RecordReplayAction,
    },
    /// Wait until a local or gateway-managed instance reports readiness bits.
    WaitReady {
        #[arg(long)]
        dcc_type: Option<String>,
        #[arg(long)]
        instance_id: Option<String>,
        #[arg(long, value_delimiter = ',')]
        require: Vec<String>,
        #[arg(long)]
        timeout_secs: Option<u64>,
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
        /// DCC type. Optional when `--operation-id` identifies the owner.
        #[arg(long, required_unless_present = "operation_id")]
        dcc_type: Option<String>,
        /// Full instance UUID or unique >=4-character prefix. Optional when
        /// `--operation-id` identifies the owner.
        #[arg(long, required_unless_present = "operation_id")]
        instance_id: Option<String>,
        #[arg(long)]
        expected_owner: Option<String>,
        #[arg(long)]
        expected_session: Option<String>,
        /// Restrict the stop to the instance launched and owned by this
        /// `start-instance` operation.
        #[arg(long, value_name = "OPERATION_ID")]
        operation_id: Option<String>,
    },
    /// Launch a project-bound DCC host and wait for readiness (zero-instance path).
    StartInstance {
        #[arg(long)]
        dcc_type: String,
        /// Absolute project root the launched host must bind to.
        #[arg(long, value_name = "PATH")]
        project: PathBuf,
        /// Explicit launch plan document. Defaults to the project receipt.
        #[arg(long, value_name = "PATH")]
        launch_plan: Option<PathBuf>,
        /// Require the launch plan to declare this exact version.
        #[arg(long)]
        version: Option<String>,
        /// Wait for terminal readiness instead of registration only.
        #[arg(long)]
        wait_ready: bool,
        /// Readiness bits required for terminal success.
        #[arg(long, value_delimiter = ',')]
        require: Vec<String>,
        #[arg(long)]
        timeout_secs: Option<u64>,
        #[arg(long, default_value = "1")]
        interval_secs: u64,
        /// Resolve and report the launch plan without spawning a process.
        #[arg(long)]
        dry_run: bool,
        /// Operator authorization to launch a GUI process.
        #[arg(long)]
        yes: bool,
    },
    /// Build an auditable DCC adapter installation plan.
    Install {
        /// Use a still-valid signed cache or the bundled catalog without checking online.
        #[arg(long, env = "DCC_MCP_INSTALL_OFFLINE")]
        offline: bool,
        #[arg(long)]
        dcc_type: String,
        /// Exact adapter package version; must match the catalog-pinned artifact.
        #[arg(long)]
        version: Option<String>,
        #[arg(long, env = "DCC_MCP_CATALOG_PATH")]
        catalog: Option<PathBuf>,
        /// Python interpreter used for pip-based adapter package installs.
        #[arg(long, env = "DCC_MCP_INSTALL_PYTHON")]
        python: Option<String>,
        /// Absolute DCC executable or application path for non-standard installs.
        #[arg(long, env = "DCC_MCP_DCC_PATH")]
        dcc_path: Option<PathBuf>,
        /// Source checkout or internal package root containing an Adobe plugin.
        #[arg(long, env = "DCC_MCP_PLUGIN_SOURCE")]
        plugin_source: Option<PathBuf>,
        /// Adobe UXP/CEP debug root. Use a studio-owned path in Internal deployments.
        #[arg(long, env = "DCC_MCP_ADOBE_DEBUG_ROOT")]
        adobe_debug_root: Option<PathBuf>,
        /// Execute the install plan with consent gating.
        #[arg(long, short = 'x')]
        execute: bool,
        /// Emit the plan or execution report as one compact JSON document.
        #[arg(long)]
        json: bool,
    },
    /// Search and manage DCC-MCP marketplace sources.
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceAction,
    },
    /// Validate local SKILL.md packages before loading them at runtime.
    Lint(LintArgs),
    Components {
        #[command(subcommand)]
        action: crate::presentation::components_cmd::ComponentsAction,
    },
    /// Check for and apply gateway-controlled binary updates.
    Update {
        #[command(subcommand)]
        action: crate::presentation::update_cmd::UpdateAction,
    },
    /// Gateway lifecycle management.
    Gateway {
        #[command(subcommand)]
        action: Option<GatewayAction>,
        #[command(flatten)]
        daemon: dcc_mcp_sidecar::gateway_daemon::GatewayArgs,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum UiControlAction {
    /// Capture the exact scoped DCC window and start its visible control session.
    Snapshot(UiControlArgs),
    /// Resolve a semantic control from the latest scoped snapshot.
    Find(UiControlArgs),
    /// Perform one scoped semantic, pointer, or keyboard action.
    Act(UiControlArgs),
    /// Start trajectory recording for the exact scoped window.
    RecordingStart(UiControlArgs),
    /// Read trajectory recording state for the exact scoped window.
    RecordingState(UiControlArgs),
    /// Finalize trajectory recording for the exact scoped window.
    RecordingStop(UiControlArgs),
    /// Wait for one semantic UI condition inside the scoped DCC window.
    Wait(UiControlArgs),
    /// Stop the scoped session and release its visible effects and input owner.
    Stop(UiControlArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct UiControlArgs {
    /// DCC type when more than one ready instance may expose UI Control.
    #[arg(long)]
    pub(crate) dcc_type: Option<String>,
    /// Full instance UUID or unique >=4-character prefix.
    #[arg(long)]
    pub(crate) instance_id: Option<String>,
    /// Operation arguments using the underlying ui-control tool schema.
    #[arg(long = "json", default_value = "{}")]
    pub(crate) arguments_json: String,
    /// Read operation arguments from a UTF-8 JSON file, or '-' for stdin.
    #[arg(long, value_name = "PATH", conflicts_with = "arguments_json")]
    pub(crate) json_file: Option<PathBuf>,
    /// Optional tool-call metadata such as agent context or lease owner.
    #[arg(long)]
    pub(crate) meta_json: Option<String>,
    /// Per-request timeout for the UI operation.
    #[arg(long, env = "DCC_MCP_CLI_CALL_TIMEOUT_SECS")]
    pub(crate) timeout_secs: Option<u64>,
    /// Print the complete underlying MCP response, including the bounded UI tree.
    #[arg(long, default_value_t = false)]
    pub(crate) full_output: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct LintArgs {
    /// Skill directory or directory tree to scan.
    #[arg(value_name = "PATH", required = true)]
    pub(crate) paths: Vec<PathBuf>,

    /// Maximum recursion depth below each PATH.
    #[arg(long, default_value = "2")]
    pub(crate) max_depth: usize,

    /// Exit non-zero when warnings are present.
    #[arg(long, default_value = "false")]
    pub(crate) warnings_as_errors: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct GatewayStartArgs {
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: String,
    #[arg(long, default_value = "9765")]
    pub(crate) port: u16,
    #[arg(long)]
    pub(crate) name: Option<String>,
    #[arg(long)]
    pub(crate) registry_dir: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) remote_host: String,
    #[arg(long, default_value = "59765")]
    pub(crate) remote_port: u16,
    #[arg(long, default_value = "0")]
    pub(crate) gateway_idle_timeout_secs: u64,
    #[arg(long)]
    pub(crate) gateway_bin: Option<PathBuf>,
    #[arg(long, default_value = "30")]
    pub(crate) wait_timeout_secs: u64,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct GatewayStopArgs {
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: String,
    #[arg(long, default_value = "9765")]
    pub(crate) port: u16,
    #[arg(long)]
    pub(crate) registry_dir: Option<PathBuf>,
    #[arg(long, default_value = "10")]
    pub(crate) wait_timeout_secs: u64,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct GatewayStatusArgs {
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: String,
    #[arg(long, default_value = "9765")]
    pub(crate) port: u16,
    #[arg(long)]
    pub(crate) registry_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct GatewayRestartArgs {
    #[command(flatten)]
    pub(crate) start: GatewayStartArgs,
    #[arg(long, default_value = "10")]
    pub(crate) stop_timeout_secs: u64,
}

#[derive(Debug, Subcommand)]
pub(crate) enum GatewayAction {
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
pub(crate) enum GatewayDaemonAction {
    /// Start the gateway daemon.
    Start(GatewayStartArgs),
    /// Restart the gateway daemon using pidfile-based stop/start.
    Restart(GatewayRestartArgs),
    /// Stop the gateway daemon.
    Stop(GatewayStopArgs),
    /// Query gateway daemon health and PID status.
    Status(GatewayStatusArgs),
}

pub(crate) fn resolve_query(query: Option<String>, query_terms: Vec<String>) -> Option<String> {
    query.or_else(|| {
        let joined = query_terms.join(" ");
        (!joined.is_empty()).then_some(joined)
    })
}

pub(crate) fn parse_marketplace_target(
    value: &str,
) -> anyhow::Result<dcc_mcp_catalog::CatalogTarget> {
    dcc_mcp_marketplace::parse_target(value).map_err(|_| {
        anyhow!("invalid marketplace target '{value}'; expected dcc|application|game|web:ID")
    })
}

pub(crate) fn parse_json_object(raw: &str, flag_name: &str) -> anyhow::Result<Value> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{flag_name} must be valid JSON"))?;
    if value.is_object() {
        Ok(value)
    } else {
        anyhow::bail!("{flag_name} must be a JSON object")
    }
}

pub(crate) fn read_call_arguments(
    raw: &str,
    json_file: Option<&std::path::Path>,
) -> anyhow::Result<Value> {
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

pub(crate) fn read_batch_request(
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
    Ok(request)
}

pub(crate) fn build_load_skill_request(
    skill_name: Option<String>,
    dcc_type: Option<String>,
    dcc: Option<String>,
    instance_id: Option<String>,
    activate_groups: Option<bool>,
    request_json: Option<String>,
) -> anyhow::Result<LoadSkillRequest> {
    if let Some(raw) = request_json {
        if skill_name.is_some()
            || dcc_type.is_some()
            || dcc.is_some()
            || instance_id.is_some()
            || activate_groups.is_some()
        {
            anyhow::bail!("load-skill --json cannot be combined with positional or routing flags");
        }
        return Ok(LoadSkillRequest {
            body: parse_json_object(&raw, "--json")?,
        });
    }

    let skill_name = skill_name
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("load-skill requires SKILL_NAME unless --json is provided")
        })?;

    let mut body = Map::new();
    body.insert("skill_name".to_string(), Value::String(skill_name));
    if let Some(dcc_type) = dcc_type {
        body.insert("dcc_type".to_string(), Value::String(dcc_type));
    }
    if let Some(dcc) = dcc {
        body.insert("dcc".to_string(), Value::String(dcc));
    }
    if let Some(instance_id) = instance_id {
        body.insert("instance_id".to_string(), Value::String(instance_id));
    }
    if let Some(activate_groups) = activate_groups {
        body.insert("activate_groups".to_string(), Value::Bool(activate_groups));
    }
    Ok(LoadSkillRequest {
        body: Value::Object(body),
    })
}

pub(crate) fn endpoint_for_mcp(raw: &str) -> String {
    let trimmed = raw.trim_end_matches('/');
    if trimmed.ends_with("/mcp") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/mcp")
    }
}

pub(crate) fn command_has_distinct_per_timeout(
    command: &Command,
    global_timeout_secs: Option<u64>,
) -> bool {
    let per_command_timeout = match command {
        Command::Smoke { timeout_secs, .. }
        | Command::Call { timeout_secs, .. }
        | Command::CallBatch { timeout_secs, .. }
        | Command::WaitReady { timeout_secs, .. }
        | Command::StartInstance { timeout_secs, .. } => *timeout_secs,
        Command::UiControl { action } => action.timeout_secs(),
        _ => None,
    };

    matches!(
        (global_timeout_secs, per_command_timeout),
        (Some(global), Some(per_command)) if global != per_command
    )
}

pub(crate) fn command_has_configured_timeout_env(command: &Command) -> bool {
    matches!(
        command,
        Command::Call { .. } | Command::CallBatch { .. } | Command::UiControl { .. }
    ) && std::env::var_os("DCC_MCP_CLI_CALL_TIMEOUT_SECS").is_some()
}
