use clap::Subcommand;
use serde_json::Value;

use super::output::{ExitCode, OutputWriter};
use crate::application::update::{
    CLI_BINARY_NAME, CLI_VERSION, UpdateService, cached_cli_update, is_background_refresh,
    just_applied_status, record_cli_update, spawn_cli_update_refresh,
};

#[derive(Debug, Subcommand)]
pub(crate) enum UpdateAction {
    /// Check whether a newer version is available.
    Check {
        #[arg(long)]
        binary: Option<String>,
        #[arg(long)]
        current_version: Option<String>,
    },
    /// Download the latest CLI version and stage it for the next launch.
    Apply {
        /// Confirm that the user approved downloading and staging the update.
        #[arg(long)]
        yes: bool,
    },
}

pub(crate) struct UpdateCommandResult {
    pub value: Value,
    pub failed: bool,
    pub exit_code: ExitCode,
}

pub(crate) async fn run(
    base_url: &str,
    action: UpdateAction,
) -> anyhow::Result<UpdateCommandResult> {
    let (value, exit_code) = match action {
        UpdateAction::Check {
            binary,
            current_version,
        } => {
            let binary_name = binary.unwrap_or_else(|| CLI_BINARY_NAME.to_string());
            let current_version = current_version.unwrap_or_else(|| CLI_VERSION.to_string());
            let cache_default_cli =
                binary_name == CLI_BINARY_NAME && current_version == CLI_VERSION;
            let service = UpdateService::new(base_url, &binary_name, &current_version);
            let value = service.check_update().await?;
            if cache_default_cli {
                record_cli_update(&value);
            }
            let exit_code = if value.get("error").is_some() {
                ExitCode::Unavailable
            } else {
                ExitCode::Success
            };
            (value, exit_code)
        }
        UpdateAction::Apply { yes } => {
            let service = UpdateService::new(base_url, CLI_BINARY_NAME, CLI_VERSION);
            let value = service.apply_update(yes).await?;
            let exit_code = match value["error"].as_str() {
                None => ExitCode::Success,
                Some("confirmation_required") => ExitCode::InvalidInput,
                Some(_) => ExitCode::Unavailable,
            };
            (value, exit_code)
        }
    };
    Ok(UpdateCommandResult {
        failed: exit_code != ExitCode::Success,
        value,
        exit_code,
    })
}

pub(crate) fn surface_cached_cli_update(
    value: &mut Value,
    writer: &OutputWriter,
    base_url: &str,
    enabled_for_command: bool,
) -> anyhow::Result<()> {
    if !enabled_for_command || is_background_refresh() {
        return Ok(());
    }

    let cached = cached_cli_update();
    let status = just_applied_status().or(cached.notification);
    if let Some(status) = status {
        if let Some(object) = value.as_object_mut() {
            object.insert("cli_update".into(), status.clone());
        }
        match status["version_status"].as_str() {
            Some("update_available") => writer.diagnostic(&format!(
                "info: dcc-mcp-cli {} -> {} is available. Ask the user before updating; after confirmation run `dcc-mcp-cli update apply --yes`. The update activates on the next CLI launch and does not restart running servers.",
                status["current_version"].as_str().unwrap_or("unknown"),
                status["latest_version"].as_str().unwrap_or("unknown"),
            ))?,
            Some("applied") => writer.diagnostic(
                "info: the staged dcc-mcp-cli update was applied on this launch; running servers were not restarted",
            )?,
            _ => {}
        }
    }
    if cached.refresh_due {
        let _ = spawn_cli_update_refresh(base_url);
    }
    Ok(())
}
