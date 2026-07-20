#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;

use crate::cli::UpdateAction;

const SERVER_BINARY_NAME: &str = env!("CARGO_PKG_NAME");
#[cfg(windows)]
const UI_CONTROL_HOST_BINARY_NAME: &str = "dcc-mcp-ui-control-host";
#[cfg(windows)]
const UI_CONTROL_HOST_FILE_NAME: &str = "dcc-mcp-ui-control-host.exe";

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
        UpdateAction::Apply => {
            #[cfg(windows)]
            stage_windows_server_update(&gateway_url).await?;
            #[cfg(not(windows))]
            stage_single_server_update(&gateway_url).await?;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
async fn stage_single_server_update(gateway_url: &str) -> anyhow::Result<()> {
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

#[cfg(windows)]
async fn stage_windows_server_update(gateway_url: &str) -> anyhow::Result<()> {
    use dcc_mcp_updater::{UpdateSetSource, UpdateTarget};

    let current_version = env!("CARGO_PKG_VERSION");
    let server_updater =
        dcc_mcp_updater::Updater::new(gateway_url, SERVER_BINARY_NAME, current_version);
    let host_updater =
        dcc_mcp_updater::Updater::new(gateway_url, UI_CONTROL_HOST_BINARY_NAME, current_version);
    let (server_info, host_info) =
        tokio::try_join!(server_updater.check_update(), host_updater.check_update())?;
    validate_paired_versions(&server_info, &host_info)?;

    let server_sha = required_sha(&server_info, SERVER_BINARY_NAME)?;
    let host_sha = required_sha(&host_info, UI_CONTROL_HOST_BINARY_NAME)?;
    let host_path = sibling_path(UI_CONTROL_HOST_FILE_NAME)?;
    let host_needs_repair = local_file_differs(&host_path, host_sha)?;
    if !server_info.update_available && !host_info.update_available && !host_needs_repair {
        print_up_to_date(&server_info)?;
        return Ok(());
    }

    // Both assets must download and pass their independent checksums before a
    // single update-set marker becomes visible to the next server launch.
    let (server_download, host_download) = tokio::try_join!(
        server_updater.download_update(&server_info),
        host_updater.download_update(&host_info)
    )?;
    let current_target = UpdateTarget::CurrentExecutable;
    let host_target = UpdateTarget::Sibling {
        file_name: UI_CONTROL_HOST_FILE_NAME.to_owned(),
    };
    dcc_mcp_updater::stage_update_set(
        SERVER_BINARY_NAME,
        &[
            UpdateSetSource {
                downloaded: &server_download,
                target: &current_target,
                expected_sha256: server_sha,
            },
            UpdateSetSource {
                downloaded: &host_download,
                target: &host_target,
                expected_sha256: host_sha,
            },
        ],
    )?;
    print_staged(&server_info, &[server_download, host_download])?;
    Ok(())
}

#[cfg(windows)]
pub(crate) async fn reconcile_ui_control_host(gateway_port: u16) -> anyhow::Result<bool> {
    let current_version = env!("CARGO_PKG_VERSION");
    let gateway_url = format!("http://127.0.0.1:{gateway_port}");
    let updater =
        dcc_mcp_updater::Updater::new(&gateway_url, UI_CONTROL_HOST_BINARY_NAME, current_version);
    let info = updater.check_update().await?;
    if info.latest_version != current_version {
        anyhow::bail!(
            "UI Control host manifest version {} does not match server {}; run dcc-mcp-server update apply",
            info.latest_version,
            current_version
        );
    }
    let expected_sha = required_sha(&info, UI_CONTROL_HOST_BINARY_NAME)?;
    let target = sibling_path(UI_CONTROL_HOST_FILE_NAME)?;
    if !local_file_differs(&target, expected_sha)? {
        return Ok(false);
    }

    // This repairs the one unavoidable bootstrap hop from a release whose old
    // updater knew only the raw server asset. Protocol v2 uses a new pipe and
    // singleton, so an old v1 process cannot impersonate the repaired host.
    let downloaded = updater.download_update(&info).await?;
    let current_exe = std::env::current_exe()?;
    dcc_mcp_updater::install_verified_sibling(
        SERVER_BINARY_NAME,
        &current_exe,
        &downloaded,
        UI_CONTROL_HOST_FILE_NAME,
        expected_sha,
    )?;
    tracing::info!(path = %target.display(), "reconciled version-matched UI Control host");
    Ok(true)
}

#[cfg(windows)]
fn validate_paired_versions(
    server: &dcc_mcp_updater::UpdateInfo,
    host: &dcc_mcp_updater::UpdateInfo,
) -> anyhow::Result<()> {
    if server.latest_version != host.latest_version {
        anyhow::bail!(
            "server and UI Control host manifest versions differ: server={}, host={}",
            server.latest_version,
            host.latest_version
        );
    }
    Ok(())
}

#[cfg(windows)]
fn required_sha<'a>(
    info: &'a dcc_mcp_updater::UpdateInfo,
    binary_name: &str,
) -> anyhow::Result<&'a str> {
    info.sha256
        .as_deref()
        .filter(|sha| sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow::anyhow!("manifest entry {binary_name} requires a valid SHA-256"))
}

#[cfg(windows)]
fn sibling_path(file_name: &str) -> anyhow::Result<PathBuf> {
    let current_exe = std::env::current_exe()?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("current server executable has no parent directory"))?;
    Ok(parent.join(file_name))
}

#[cfg(windows)]
fn local_file_differs(path: &Path, expected_sha: &str) -> anyhow::Result<bool> {
    if !path.is_file() {
        return Ok(true);
    }
    Ok(!dcc_mcp_updater::sha256_file(path)?.eq_ignore_ascii_case(expected_sha))
}

fn print_up_to_date(info: &dcc_mcp_updater::UpdateInfo) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "up-to-date",
            "current_version": info.current_version,
            "latest_version": info.latest_version,
            "message": "Already running the latest version with its required runtime components."
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
            "message": "Update set downloaded and staged. Restart the server to apply it as one recoverable transaction.",
        }))?
    );
    Ok(())
}

/// Apply a staged update and report whether the process must re-exec.
pub(crate) fn apply_staged_update() -> anyhow::Result<bool> {
    #[cfg(windows)]
    let result = dcc_mcp_updater::apply_staged_update_set(SERVER_BINARY_NAME);
    #[cfg(not(windows))]
    let result = dcc_mcp_updater::Updater::apply_staged_update(SERVER_BINARY_NAME);

    match result {
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn local_host_hash_detects_missing_stale_and_current_files() {
        let temp = tempfile::tempdir().unwrap();
        let host = temp.path().join(UI_CONTROL_HOST_FILE_NAME);
        let expected = dcc_mcp_updater::sha256_file({
            std::fs::write(&host, b"current-host").unwrap();
            &host
        })
        .unwrap();
        assert!(!local_file_differs(&host, &expected).unwrap());
        std::fs::write(&host, b"stale-host").unwrap();
        assert!(local_file_differs(&host, &expected).unwrap());
        std::fs::remove_file(&host).unwrap();
        assert!(local_file_differs(&host, &expected).unwrap());
    }
}
