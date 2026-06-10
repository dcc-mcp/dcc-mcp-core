use dcc_mcp_updater::Updater;
use serde_json::Value;

use crate::domain::rest::Endpoint;

/// Service for checking and applying binary updates through the gateway.
pub struct UpdateService {
    updater: Updater,
}

impl UpdateService {
    pub fn new(gateway_url: &str, binary_name: &str, current_version: &str) -> Self {
        Self {
            updater: Updater::new(gateway_url, binary_name, current_version),
        }
    }

    pub fn with_endpoint(endpoint: &Endpoint, binary_name: &str, current_version: &str) -> Self {
        Self::new(&endpoint.base_url, binary_name, current_version)
    }

    /// Check for available updates. Returns the result as a JSON Value.
    pub async fn check_update(&self) -> anyhow::Result<Value> {
        let info = self.updater.check_update().await?;
        Ok(serde_json::to_value(&info)?)
    }

    /// Check for and apply an update (download + stage for next launch).
    pub async fn apply_update(&self) -> anyhow::Result<Value> {
        let info = self.updater.check_update().await?;

        if !info.update_available {
            return Ok(serde_json::json!({
                "status": "up-to-date",
                "current_version": info.current_version,
                "latest_version": info.latest_version,
                "message": "Already running the latest version."
            }));
        }

        // Download the update archive
        let downloaded = self.updater.download_update(&info).await?;

        // Stage it for replacement on next launch
        Updater::stage_update(&downloaded, &self.updater.binary_name())?;

        Ok(serde_json::json!({
            "status": "staged",
            "current_version": info.current_version,
            "latest_version": info.latest_version,
            "staged_at": downloaded.to_string_lossy(),
            "message": "Update downloaded and staged. Restart the binary to apply.",
        }))
    }
}
