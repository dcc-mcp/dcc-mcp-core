use std::path::PathBuf;

use crate::cli::UpdateAction;

const SERVER_BINARY_NAME: &str = env!("CARGO_PKG_NAME");

pub(crate) async fn run_update_cmd(gateway_port: u16, action: UpdateAction) -> anyhow::Result<()> {
    let gateway_url = format!("http://127.0.0.1:{gateway_port}");

    match action {
        UpdateAction::Check {
            binary,
            current_version,
        } => {
            let binary_name = binary.unwrap_or_else(|| SERVER_BINARY_NAME.to_string());
            let current_version =
                current_version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
            let updater =
                dcc_mcp_updater::Updater::new(&gateway_url, &binary_name, &current_version);
            let info = updater.check_update().await?;
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        UpdateAction::Apply => stage_server_update(&gateway_url).await?,
    }
    Ok(())
}

async fn stage_server_update(gateway_url: &str) -> anyhow::Result<()> {
    let updater =
        dcc_mcp_updater::Updater::new(gateway_url, SERVER_BINARY_NAME, env!("CARGO_PKG_VERSION"));
    let info = updater.check_update().await?;
    if !info.update_available {
        print_up_to_date(&info)?;
        return Ok(());
    }
    let downloaded = updater.download_update(&info).await?;
    dcc_mcp_updater::Updater::stage_update(&downloaded, updater.binary_name())?;
    print_staged(&info, &[downloaded])?;
    Ok(())
}

fn print_up_to_date(info: &dcc_mcp_updater::UpdateInfo) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "up-to-date",
            "current_version": info.current_version,
            "latest_version": info.latest_version,
            "message": "Already running the latest version."
        }))?
    );
    Ok(())
}

fn print_staged(info: &dcc_mcp_updater::UpdateInfo, downloaded: &[PathBuf]) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "staged",
            "current_version": info.current_version,
            "latest_version": info.latest_version,
            "staged_at": downloaded.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
            "message": "Update downloaded and staged. Restart the server to apply it.",
        }))?
    );
    Ok(())
}

/// Apply a staged update and report whether the process must re-exec.
pub(crate) fn apply_staged_update() -> anyhow::Result<bool> {
    match dcc_mcp_updater::Updater::apply_staged_update(SERVER_BINARY_NAME) {
        Ok(true) => {
            tracing::info!("staged server update applied; restarting into the new executable");
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Spawn the just-installed executable with the exact current arguments.
pub(crate) fn restart_after_update() -> anyhow::Result<()> {
    let executable = std::env::current_exe()?;
    let mut command = std::process::Command::new(&executable);
    command.args(std::env::args_os().skip(1));
    command.spawn()?;
    tracing::info!(path = %executable.display(), "spawned updated server executable");
    Ok(())
}
