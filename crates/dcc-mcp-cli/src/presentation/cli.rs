use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use clap::Parser;
use serde_json::Value;

use super::output::{ExitCode, OutputFormat, OutputWriter, failure_envelope, to_json};

use crate::application::adapter_import;
use crate::application::call_attribution::{
    attach_agent_session_id, attach_batch_agent_session_id,
};
use crate::application::client::DccMcpClient;
use crate::application::control_plane::DccControlPlane;
use crate::application::doctor::{DoctorContext, run_doctor};
use crate::application::gateway_ensure;
use crate::application::gateway_profile;
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

mod cli_args;
mod dcc_types_output;
mod gateway_cmd;
mod image_artifacts;
mod job_progress;
mod lint;
mod marketplace_output;
mod record_replay;
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
    /// Require local agent-control calls to pass through the gateway for audit and stats.
    #[arg(
        long,
        global = true,
        env = "DCC_MCP_CLI_REQUIRE_GATEWAY",
        default_value = "false"
    )]
    require_gateway: bool,
    /// Task-scoped stats identifier written to _meta.agent_context.session_id on calls.
    #[arg(long, global = true, env = "DCC_MCP_AGENT_SESSION_ID")]
    agent_session_id: Option<String>,
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
    /// Non-interactive mode: zero prompts, missing input fails immediately (exit code 2).
    #[arg(long, global = true, env = "DCC_MCP_NON_INTERACTIVE")]
    non_interactive: bool,
    /// Global timeout in seconds for all operations.
    #[arg(long, global = true, env = "DCC_MCP_TIMEOUT_SECS")]
    timeout_secs: Option<u64>,
    #[command(flatten)]
    transport: TransportArgs,
    #[command(subcommand)]
    command: Command,
}

pub async fn run() -> anyhow::Result<()> {
    if apply_staged_update() {
        return restart_after_update();
    }
    run_with_args(Args::parse()).await
}

fn apply_staged_update() -> bool {
    // A package manager owns this binary (see application::package_manager):
    // replacing it would undo the version the manager installed, so a staged
    // update is left in place and never applied.
    if crate::application::package_manager::detect().is_some() {
        return false;
    }

    // Apply any staged binary update before running commands (CLI restart
    // is the user's next invocation after `update apply`).
    match dcc_mcp_updater::Updater::apply_staged_update(env!("CARGO_PKG_NAME")) {
        Ok(true) => {
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

/// Parse repeatable `--adapter-python <dcc_type>=<python>` values.
fn parse_adapter_python(raw: &[String]) -> anyhow::Result<BTreeMap<String, PathBuf>> {
    let mut parsed = BTreeMap::new();
    for entry in raw {
        let (dcc_type, python) = entry.split_once('=').context(
            "--adapter-python expects <dcc_type>=<python>, e.g. maya=/usr/autodesk/maya2026/bin/mayapy",
        )?;
        let dcc_type = dcc_type.trim();
        let python = python.trim();
        if dcc_type.is_empty() || python.is_empty() {
            anyhow::bail!(
                "--adapter-python expects <dcc_type>=<python> with both sides non-empty, got '{entry}'"
            );
        }
        parsed.insert(dcc_type.to_ascii_lowercase(), PathBuf::from(python));
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

async fn run_with_args(args: Args) -> anyhow::Result<()> {
    let Args {
        base_url,
        gateway,
        no_auto_gateway,
        require_gateway,
        agent_session_id,
        auto_gateway_bin,
        auto_gateway_timeout_secs,
        output,
        non_interactive,
        timeout_secs: global_timeout_secs,
        transport: transport_args,
        command,
    } = args;

    let transport = transport_args.transport;
    let output = match &command {
        Command::Feedback(args) => args.resolve_output(output),
        Command::Install { json: true, .. } => Ok(OutputFormat::Json),
        _ => Ok(output.unwrap_or_else(OutputFormat::auto_detect)),
    };
    let writer = OutputWriter::new(output.map_err(anyhow::Error::msg)?);
    let explicit_update_command = matches!(&command, Command::Update { .. });
    let marketplace_update_check = (!crate::application::update::is_background_refresh())
        .then(|| tokio::spawn(check_marketplace_updates()));

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
        gateway_ensure::default_registry_dir(),
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

    let mut failed = false;
    let mut exit_code = ExitCode::GeneralError;
    let mut explicit_exit_code = None;
    let mut value = match command {
        Command::Smoke {
            url,
            query,
            limit,
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
            let result = client.smoke(mcp_url, query, limit).await;
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
        Command::Search {
            query,
            query_terms,
            dcc_type,
            instance_id,
            limit,
        } => {
            let request = SearchRequest {
                query: resolve_query(query, query_terms),
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
            let owned_operation = crate::application::instance_launch::resolve_owned_operation(
                &gateway_ensure::default_registry_dir(),
                dcc_type.as_deref(),
                instance_id.as_deref(),
                operation_id.as_deref(),
            )?;
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
                registry_dir: gateway_ensure::default_registry_dir(),
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
                    let installed_dcc = installed.dcc.clone();
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
                    let installed_dcc = installed_target.id.clone();
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
        Command::Lint(lint_args) => {
            let result = lint::run_lint_cmd(&lint_args).await?;
            failed = result.failed;
            if failed {
                exit_code = ExitCode::InvalidInput;
            }
            result.value
        }
        Command::Components { action } => super::components_cmd::run(action).await?,
        Command::Update { action } => {
            let result = super::update_cmd::run(&base_url, action).await?;
            failed = result.failed;
            exit_code = result.exit_code;
            result.value
        }
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

    if let Some(marketplace_update_check) = marketplace_update_check
        && let Ok(Ok(Some(updates))) =
            tokio::time::timeout(Duration::from_millis(750), marketplace_update_check).await
    {
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "marketplace_updates".into(),
                serde_json::json!(updates.clone()),
            );
        }
        eprintln!(
            "info: marketplace updates available for {}. Review and run `dcc-mcp-cli marketplace update` after confirmation.",
            updates.join(", ")
        );
    }

    super::update_cmd::surface_cached_cli_update(
        &mut value,
        &writer,
        &base_url,
        !explicit_update_command,
    )?;

    writer.write_data(&value)?;
    if let Some(code) = explicit_exit_code
        && code != 0
    {
        std::process::exit(code);
    }
    if failed {
        let envelope = failure_envelope(&value, exit_code);
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
