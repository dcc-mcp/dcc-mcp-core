use crate::cli::UpdateAction;

pub(crate) async fn run_update_cmd(gateway_port: u16, action: UpdateAction) -> anyhow::Result<()> {
    let gateway_url = format!("http://127.0.0.1:{gateway_port}");

    match action {
        UpdateAction::Check {
            binary,
            current_version,
        } => {
            let binary_name = binary.unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string());
            let current_version =
                current_version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
            let updater =
                dcc_mcp_updater::Updater::new(&gateway_url, &binary_name, &current_version);
            let info = updater.check_update().await?;
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        UpdateAction::Apply => {
            let updater = dcc_mcp_updater::Updater::new(
                &gateway_url,
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            );
            let info = updater.check_update().await?;
            if !info.update_available {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "up-to-date",
                        "current_version": info.current_version,
                        "latest_version": info.latest_version,
                        "message": "Already running the latest version."
                    }))?
                );
                return Ok(());
            }
            let downloaded = updater.download_update(&info).await?;
            dcc_mcp_updater::Updater::stage_update(&downloaded, updater.binary_name())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "staged",
                    "current_version": info.current_version,
                    "latest_version": info.latest_version,
                    "staged_at": downloaded.to_string_lossy(),
                    "message": "Update downloaded and staged. Restart the server to apply.",
                }))?
            );
        }
    }
    Ok(())
}

pub(crate) fn apply_staged_update() {
    match dcc_mcp_updater::Updater::apply_staged_update(env!("CARGO_PKG_NAME")) {
        Ok(true) => tracing::info!("staged binary update applied"),
        Ok(false) => {}
        Err(e) => tracing::warn!(error = %e, "failed to apply staged binary update"),
    }
}
