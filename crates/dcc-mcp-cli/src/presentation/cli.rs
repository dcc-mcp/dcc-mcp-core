use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use dcc_mcp_skills::validator::IssueSeverity;
use dcc_mcp_skills::{SkillValidationReport, validate_skill_dir};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::application::client::DccMcpClient;
use crate::application::control_plane::DccControlPlane;
use crate::application::doctor::{DoctorRequest, run_doctor};
use crate::application::gateway_ctrl;
use crate::application::gateway_ensure;
use crate::application::gateway_profile::{self, GatewayTarget};
use crate::application::install::InstallService;
use crate::application::marketplace::new_service;
use crate::domain::install::InstallRequest;
use crate::domain::rest::{
    Endpoint, LoadSkillRequest, ReloadSkillsRequest, SearchRequest, StopInstanceRequest,
    WaitReadyRequest,
};
use crate::infra::http::HttpGateway;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:9765";

#[derive(Debug, Parser)]
#[command(name = "dcc-mcp-cli", about, version)]
pub struct Args {
    #[arg(long, global = true, env = "DCC_MCP_BASE_URL")]
    base_url: Option<String>,
    /// Select a gateway profile. Use `local` for the local FileRegistry path.
    #[arg(long, global = true, env = "DCC_MCP_GATEWAY_PROFILE")]
    gateway: Option<String>,
    /// Disable the default local gateway auto-start before agent control commands.
    #[arg(long, env = "DCC_MCP_CLI_NO_AUTO_GATEWAY", default_value = "false")]
    no_auto_gateway: bool,
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
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    output: OutputFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Pretty,
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
    /// Invoke one tool slug.
    Call {
        tool_slug: String,
        /// DCC type for direct backend-tool calls without a dotted gateway slug.
        #[arg(long)]
        dcc_type: Option<String>,
        /// Full instance UUID or unique >=4-character prefix for direct calls.
        #[arg(long)]
        instance_id: Option<String>,
        #[arg(long = "json", default_value = "{}")]
        arguments_json: String,
        #[arg(long)]
        meta_json: Option<String>,
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
        action: UpdateAction,
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
    run_with_args(Args::parse()).await
}

async fn run_with_args(args: Args) -> anyhow::Result<()> {
    // Apply any staged binary update before running commands (CLI restart
    // is the user's next invocation after `update apply`).
    match dcc_mcp_updater::Updater::apply_staged_update(env!("CARGO_PKG_NAME")) {
        Ok(true) => eprintln!("info: staged binary update applied"),
        Ok(false) => { /* no update was staged */ }
        Err(e) => eprintln!("warning: failed to apply staged binary update: {e}"),
    }

    let Args {
        base_url,
        gateway,
        no_auto_gateway,
        auto_gateway_bin,
        auto_gateway_timeout_secs,
        output,
        command,
    } = args;

    let profile_path = gateway_profile::default_profile_path();
    let profile_store = gateway_profile::GatewayProfileStore::load(&profile_path)?;
    let gateway_target = profile_store.resolve(gateway.as_deref(), base_url.as_deref())?;
    let endpoint = gateway_target.endpoint_or_default(DEFAULT_BASE_URL);
    let base_url = endpoint.base_url.clone();
    let control = DccControlPlane::new(
        gateway_target.clone(),
        endpoint.clone(),
        gateway_ensure::default_registry_dir(),
    );

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

    let mut failed = false;
    let value = match command {
        Command::Smoke {
            url,
            query,
            limit,
            timeout_secs,
        } => {
            let endpoint = url
                .as_deref()
                .map(Endpoint::from_mcp_url)
                .unwrap_or_else(|| Endpoint::new(&base_url));
            let mcp_url = url.as_ref().map(|raw| endpoint_for_mcp(raw));
            let client = DccMcpClient::with_gateway(
                endpoint,
                HttpGateway::with_timeout(Duration::from_secs(timeout_secs.max(1))),
            );
            let result = client.smoke(mcp_url, query, limit).await;
            failed = !result.get("ok").and_then(Value::as_bool).unwrap_or(false);
            result
        }
        Command::Health => {
            let client = DccMcpClient::new(endpoint.clone());
            client.health().await?
        }
        Command::Doctor {
            registry_dir,
            gateway_host,
            gateway_port,
        } => {
            run_doctor(DoctorRequest {
                profile_path: profile_path.clone(),
                profile_store: profile_store.clone(),
                gateway_target: gateway_target.clone(),
                registry_dir,
                server_bin: auto_gateway_bin.clone(),
                auto_gateway_enabled: !no_auto_gateway,
                gateway_host,
                gateway_port,
            })
            .await?
        }
        Command::List => control.list_instances().await?,
        Command::Search {
            query,
            dcc_type,
            instance_id,
            limit,
        } => {
            let request = SearchRequest {
                query,
                dcc_type,
                instance_id,
                limit,
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
            dcc_type,
            instance_id,
            arguments_json,
            meta_json,
        } => {
            let arguments = parse_json_object(&arguments_json, "--json")?;
            let meta = meta_json
                .as_deref()
                .map(|raw| parse_json_object(raw, "--meta-json"))
                .transpose()?;
            control
                .call(tool_slug, dcc_type, instance_id, arguments, meta)
                .await?
        }
        Command::WaitReady {
            dcc_type,
            instance_id,
            require,
            timeout_secs,
            interval_secs,
        } => {
            let request = WaitReadyRequest {
                dcc_type,
                instance_id,
                required: require,
                timeout: Duration::from_secs(timeout_secs),
                interval: Duration::from_secs(interval_secs.max(1)),
            };
            let result = control.wait_ready(request).await?;
            failed = !result
                .get("ready")
                .and_then(Value::as_bool)
                .unwrap_or(false);
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
            control.reload_skills(request).await?
        }
        Command::StopInstance {
            dcc_type,
            instance_id,
            expected_owner,
            expected_session,
        } => {
            let request = StopInstanceRequest {
                dcc_type,
                instance_id,
                expected_owner,
                expected_session,
            };
            control.stop_instance(request).await?
        }
        Command::Install {
            dcc_type,
            version,
            catalog,
            python,
            execute,
        } => {
            let service = InstallService::new(PathBuf::from("dcc-mcp-catalog.yml"));
            let req = InstallRequest {
                dcc_type,
                version,
                catalog_path: catalog,
                python,
            };
            if execute {
                to_json(service.execute(req)?)?
            } else {
                to_json(service.plan(req)?)?
            }
        }
        Command::Marketplace { action } => {
            let service = new_service()?;
            match action {
                MarketplaceAction::Add { source } => to_json(service.add_source(&source)?)?,
                MarketplaceAction::List => to_json(service.list_sources()?)?,
                MarketplaceAction::Search {
                    query,
                    dcc,
                    sources,
                    limit,
                    skip_validation,
                } => to_json(
                    service
                        .search(query, dcc, sources, limit, skip_validation)
                        .await?,
                )?,
                MarketplaceAction::Inspect {
                    name,
                    sources,
                    skip_validation,
                } => to_json(service.inspect(name, sources, skip_validation).await?)?,
                MarketplaceAction::Install {
                    name,
                    dcc,
                    sources,
                    force,
                    skip_validation,
                } => to_json(
                    service
                        .install(name, dcc, sources, force, skip_validation)
                        .await?,
                )?,
                MarketplaceAction::Uninstall { name, dcc } => {
                    to_json(service.uninstall(&name, &dcc)?)?
                }
                MarketplaceAction::ListInstalled { dcc } => {
                    to_json(service.list_installed(dcc.as_deref())?)?
                }
                MarketplaceAction::Outdated { dcc, names } => {
                    to_json(service.outdated(dcc.as_deref(), names).await?)?
                }
                MarketplaceAction::Update { name, all, dcc } => {
                    to_json(service.update(name, all, dcc).await?)?
                }
                MarketplaceAction::AddRepo {
                    repo_ref,
                    dcc,
                    list,
                    force,
                } => {
                    if list {
                        to_json(service.list_repo_skills(&repo_ref)?)?
                    } else {
                        to_json(service.add_repo(&repo_ref, dcc.as_deref(), force)?)?
                    }
                }
            }
        }
        Command::Lint(lint_args) => {
            let result = run_lint_cmd(&lint_args)?;
            failed = result.failed;
            result.value
        }
        Command::Update { action } => match action {
            UpdateAction::Check {
                binary,
                current_version,
            } => {
                let binary_name = binary.unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string());
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
                    env!("CARGO_PKG_NAME"),
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
    };

    print_value(&value, output)?;
    if failed {
        std::process::exit(1);
    }
    Ok(())
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
        Command::Health | Command::Update { .. } => Some(Endpoint::new(base_url)),
        Command::Doctor { .. } => None,
        Command::List
        | Command::Search { .. }
        | Command::Describe { .. }
        | Command::LoadSkill { .. }
        | Command::Call { .. }
        | Command::WaitReady { .. }
        | Command::ReloadSkills { .. }
        | Command::StopInstance { .. } => Some(Endpoint::new(base_url)),
        // Local mode still executes these commands through FileRegistry/direct
        // MCP where that is the richer path, but the CLI owns gateway
        // lifecycle by default so agents can rely on the admin/control plane.
        Command::Install { .. }
        | Command::Marketplace { .. }
        | Command::Lint(_)
        | Command::Gateway { .. } => None,
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

fn build_load_skill_request(
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

fn endpoint_for_mcp(raw: &str) -> String {
    let trimmed = raw.trim_end_matches('/');
    if trimmed.ends_with("/mcp") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/mcp")
    }
}

fn to_json(value: impl Serialize) -> anyhow::Result<Value> {
    serde_json::to_value(value).context("failed to serialize command output")
}

fn print_value(value: &Value, output: OutputFormat) -> anyhow::Result<()> {
    match output {
        OutputFormat::Json => println!("{}", serde_json::to_string(value)?),
        OutputFormat::Pretty if is_list_payload(value) => print_list_pretty(value),
        OutputFormat::Pretty => println!("{}", serde_json::to_string_pretty(value)?),
    }
    Ok(())
}

fn is_list_payload(value: &Value) -> bool {
    value.get("instances").is_some() && value.get("gateway").is_some()
}

fn print_list_pretty(value: &Value) {
    let gateway = value.get("gateway").unwrap_or(&Value::Null);
    println!("Gateway");
    if let Some(current) = gateway.get("current").filter(|v| !v.is_null()) {
        println!(
            "  owner      {}",
            gateway_summary(
                current,
                current
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("active")
            )
        );
    } else if let Some(error) = gateway.get("error").and_then(Value::as_str) {
        println!("  owner      unknown ({error})");
    } else {
        println!("  owner      unknown");
    }

    let candidates = gateway
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if candidates.is_empty() {
        println!("  candidates none");
    } else {
        println!("  candidates");
        for candidate in candidates {
            println!("    {}", gateway_summary(&candidate, "challenger"));
        }
    }

    println!();
    println!("Instances");
    let instances = value
        .get("instances")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if instances.is_empty() {
        println!("  none");
        return;
    }
    for instance in instances {
        let dcc = instance
            .get("dcc_type")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let short = instance
            .get("instance_short")
            .or_else(|| instance.get("instance_id"))
            .and_then(Value::as_str)
            .unwrap_or("-");
        let name = instance
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let pid = instance
            .get("pid")
            .and_then(Value::as_u64)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        let status = instance
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("available");
        let mcp_url = instance
            .get("mcp_url")
            .and_then(Value::as_str)
            .unwrap_or("-");
        println!("  {dcc:<12} {short:<12} {status:<12} pid={pid:<8} name={name} mcp={mcp_url}");
    }
}

fn gateway_summary(value: &Value, fallback_role: &str) -> String {
    let name = value
        .get("name")
        .or_else(|| value.get("display_name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or(fallback_role);
    let pid = value
        .get("pid")
        .and_then(Value::as_u64)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());
    let dcc = value
        .get("adapter_dcc")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let version = value
        .get("adapter_version")
        .or_else(|| value.get("version"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    let host = value.get("host").and_then(Value::as_str).unwrap_or("-");
    let port = value
        .get("port")
        .and_then(Value::as_u64)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("{name} role={role} pid={pid} dcc={dcc} version={version} addr={host}:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_endpoint_for_command_ensures_gateway_for_agent_control_commands() {
        let local = GatewayTarget::Local;
        let remote = GatewayTarget::Remote {
            name: "pcA".to_string(),
            endpoint: Endpoint::new(DEFAULT_BASE_URL),
        };
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Smoke {
                    url: None,
                    query: "sphere".to_string(),
                    limit: 5,
                    timeout_secs: 5,
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Smoke {
                    url: Some("http://127.0.0.1:8765/mcp".to_string()),
                    query: "sphere".to_string(),
                    limit: 5,
                    timeout_secs: 5,
                },
                &local,
            )
            .is_none()
        );
        assert!(gateway_endpoint_for_command(DEFAULT_BASE_URL, &Command::Health, &local).is_some());
        assert!(gateway_endpoint_for_command(DEFAULT_BASE_URL, &Command::List, &local).is_some());
        assert!(gateway_endpoint_for_command(DEFAULT_BASE_URL, &Command::List, &remote).is_some());
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Search {
                    query: Some("sphere".to_string()),
                    dcc_type: None,
                    instance_id: None,
                    limit: None,
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Describe {
                    tool_slug: "maya.abc12345.create_sphere".to_string(),
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::LoadSkill {
                    skill_name: Some("maya-modeling".to_string()),
                    dcc_type: Some("maya".to_string()),
                    dcc: None,
                    instance_id: Some("abc12345".to_string()),
                    activate_groups: None,
                    request_json: None,
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Call {
                    tool_slug: "maya.abc12345.create_sphere".to_string(),
                    dcc_type: None,
                    instance_id: None,
                    arguments_json: "{}".to_string(),
                    meta_json: None,
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::WaitReady {
                    dcc_type: Some("maya".to_string()),
                    instance_id: Some("abc12345".to_string()),
                    require: vec!["process".to_string(), "dispatcher".to_string()],
                    timeout_secs: 30,
                    interval_secs: 1,
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::ReloadSkills {
                    dcc_type: Some("maya".to_string()),
                    instance_id: Some("abc12345".to_string()),
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::StopInstance {
                    dcc_type: "maya".to_string(),
                    instance_id: "abc12345".to_string(),
                    expected_owner: Some("release-smoke-test".to_string()),
                    expected_session: Some("test".to_string()),
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Search {
                    query: Some("sphere".to_string()),
                    dcc_type: None,
                    instance_id: None,
                    limit: None,
                },
                &remote,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::ReloadSkills {
                    dcc_type: Some("maya".to_string()),
                    instance_id: Some("abc12345".to_string()),
                },
                &remote,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Update {
                    action: UpdateAction::Check {
                        binary: Some("dcc-mcp-server".to_string()),
                        current_version: Some("0.0.0".to_string()),
                    },
                },
                &local,
            )
            .is_some()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Doctor {
                    registry_dir: None,
                    gateway_host: "127.0.0.1".to_string(),
                    gateway_port: 9765,
                },
                &local,
            )
            .is_none()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Marketplace {
                    action: MarketplaceAction::List,
                },
                &local,
            )
            .is_none()
        );
        assert!(
            gateway_endpoint_for_command(
                DEFAULT_BASE_URL,
                &Command::Gateway {
                    action: Some(GatewayAction::Status(GatewayStatusArgs {
                        host: "127.0.0.1".to_string(),
                        port: 9765,
                        registry_dir: None,
                    })),
                    daemon: default_gateway_daemon_args(),
                },
                &local,
            )
            .is_none()
        );
    }

    #[test]
    fn gateway_daemon_start_defaults_to_persistent_daemon() {
        let args = Args::parse_from(["dcc-mcp-cli", "gateway", "daemon", "start"]);

        let Command::Gateway {
            action:
                Some(GatewayAction::Daemon {
                    action: GatewayDaemonAction::Start(start),
                }),
            ..
        } = args.command
        else {
            panic!("expected gateway daemon start");
        };

        assert_eq!(start.gateway_idle_timeout_secs, 0);
    }

    #[test]
    fn gateway_daemon_restart_defaults_to_persistent_daemon() {
        let args = Args::parse_from(["dcc-mcp-cli", "gateway", "daemon", "restart"]);

        let Command::Gateway {
            action:
                Some(GatewayAction::Daemon {
                    action: GatewayDaemonAction::Restart(restart),
                }),
            ..
        } = args.command
        else {
            panic!("expected gateway daemon restart");
        };

        assert_eq!(restart.start.gateway_idle_timeout_secs, 0);
        assert_eq!(restart.stop_timeout_secs, 10);
    }

    fn default_gateway_daemon_args() -> dcc_mcp_sidecar::gateway_daemon::GatewayArgs {
        dcc_mcp_sidecar::gateway_daemon::GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: None,
            remote_host: "0.0.0.0".to_string(),
            remote_port: 59765,
            registry_dir: None,
            no_admin: false,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: 30,
            daemon: false,
            pidfile: None,
            restart: false,
        }
    }
}
