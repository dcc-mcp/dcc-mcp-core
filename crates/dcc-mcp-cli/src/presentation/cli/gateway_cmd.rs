//! Gateway lifecycle commands for `dcc-mcp-cli`.
//!
//! Owns gateway auto-start for a command, the gateway endpoint decision, and
//! the `gateway` subcommand tree plus its request conversions.

use std::path::PathBuf;

use serde_json::Value;

use crate::application::gateway_ctrl;
use crate::application::gateway_ensure;
use crate::application::gateway_profile::{self, GatewayTarget};
use crate::domain::rest::Endpoint;

use super::cli_args::{
    Command, GatewayAction, GatewayDaemonAction, GatewayStartArgs, GatewayStatusArgs,
    GatewayStopArgs,
};

use crate::presentation::marketplace_cmd::MarketplaceAction;

pub(crate) async fn ensure_gateway_for_command(
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

pub(crate) fn gateway_endpoint_for_command(
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
        Command::Feedback(args) => args.requires_gateway().then_some(Endpoint::new(base_url)),
        Command::Doctor { .. } | Command::DccTypes { .. } => None,
        Command::List
        | Command::Search { .. }
        | Command::Describe { .. }
        | Command::LoadSkill { .. }
        | Command::Call { .. }
        | Command::CallBatch { .. }
        | Command::UiControl { .. }
        | Command::RecordReplay { .. }
        | Command::WaitReady { .. }
        | Command::ReloadSkills { .. }
        | Command::StopInstance { .. } => Some(Endpoint::new(base_url)),
        Command::Marketplace {
            action: MarketplaceAction::Install { reload: true, .. },
        }
        | Command::Marketplace {
            action: MarketplaceAction::Uninstall { reload: true, .. },
        } => Some(Endpoint::new(base_url)),
        // Local mode still executes these commands through FileRegistry/direct
        // MCP where that is the richer path, but the CLI owns gateway
        // lifecycle by default so agents can rely on the admin/control plane.
        Command::Install { .. }
        | Command::Marketplace { .. }
        | Command::Lint(_)
        | Command::Components { .. }
        | Command::Gateway { .. }
        // `start-instance` owns the local lifecycle: it resolves a launch plan
        // and talks to the FileRegistry directly, never through the gateway.
        | Command::StartInstance { .. } => None,
    }
}

pub(crate) async fn run_gateway_cmd(
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

pub(crate) fn gateway_daemon_request(
    action: GatewayDaemonAction,
) -> gateway_ctrl::GatewayDaemonRequest {
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
