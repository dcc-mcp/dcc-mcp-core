use dcc_mcp_updater::Updater;
use serde_json::Value;

use crate::domain::rest::Endpoint;

mod cache;

pub use cache::{
    cached_cli_update, is_background_refresh, just_applied_status, record_cli_update,
    spawn_cli_update_refresh,
};

pub const CLI_BINARY_NAME: &str = env!("CARGO_PKG_NAME");
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

const SUCCESS_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const FAILURE_RETRY_INTERVAL_SECS: u64 = 60 * 60;

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

    /// Check for available updates and return the stable agent-facing contract.
    pub async fn check_update(&self) -> anyhow::Result<Value> {
        let (payload, success) = match self.updater.check_update_json().await {
            Ok(payload) => {
                let success = payload.success && payload.body.get("error").is_none();
                (payload.body, success)
            }
            Err(error) => (
                serde_json::json!({
                    "status": "check_failed",
                    "error": "update_check_failed",
                    "message": error.to_string(),
                    "update_available": false,
                    "binary_name": self.updater.binary_name(),
                }),
                false,
            ),
        };
        Ok(enrich_check_payload(
            payload,
            success,
            "live",
            unix_timestamp(),
        ))
    }

    /// Check for and apply an update (download + stage for next launch).
    pub async fn apply_update(&self, confirmed: bool) -> anyhow::Result<Value> {
        let info = match self.updater.check_update().await {
            Ok(info) => info,
            Err(error) => {
                return Ok(enrich_check_payload(
                    serde_json::json!({
                        "status": "check_failed",
                        "error": "update_check_failed",
                        "message": error.to_string(),
                        "update_available": false,
                        "binary_name": self.updater.binary_name(),
                    }),
                    false,
                    "live",
                    unix_timestamp(),
                ));
            }
        };

        if !info.update_available {
            return Ok(enrich_check_payload(
                serde_json::json!({
                    "status": "up-to-date",
                    "current_version": info.current_version,
                    "latest_version": info.latest_version,
                    "update_available": false,
                    "binary_name": self.updater.binary_name(),
                    "message": "Already running the latest version."
                }),
                true,
                "live",
                unix_timestamp(),
            ));
        }

        if !confirmed {
            return Ok(enrich_check_payload(
                serde_json::json!({
                    "status": "confirmation_required",
                    "error": "confirmation_required",
                    "message": "User confirmation is required before downloading an update.",
                    "current_version": info.current_version,
                    "latest_version": info.latest_version,
                    "update_available": true,
                    "binary_name": self.updater.binary_name(),
                    "release_notes": info.release_notes,
                }),
                true,
                "live",
                unix_timestamp(),
            ));
        }

        // Download and verify the update binary.
        let downloaded = self.updater.download_verified_update(&info).await?;

        // Stage it for replacement on next launch
        Updater::stage_verified_update(
            downloaded.path(),
            self.updater.binary_name(),
            downloaded.sha256(),
        )?;

        let mut payload = enrich_check_payload(
            serde_json::json!({
                "status": "staged",
                "current_version": info.current_version,
                "latest_version": info.latest_version,
                "update_available": true,
                "binary_name": self.updater.binary_name(),
                "staged_at": downloaded.path().to_string_lossy(),
                "message": "Update downloaded and staged. It will be applied on the next CLI launch.",
            }),
            true,
            "live",
            unix_timestamp(),
        );
        payload["version_status"] = Value::String("staged".into());
        Ok(payload)
    }
}

fn enrich_check_payload(
    mut payload: Value,
    success: bool,
    source: &str,
    checked_at_unix_secs: u64,
) -> Value {
    let Some(object) = payload.as_object_mut() else {
        return payload;
    };
    let update_available = object
        .get("update_available")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let binary_name = object
        .get("binary_name")
        .and_then(Value::as_str)
        .unwrap_or(CLI_BINARY_NAME)
        .to_string();
    let version_status = if !success {
        "check_failed"
    } else if update_available {
        "update_available"
    } else {
        "up_to_date"
    };
    object.insert(
        "version_status".into(),
        Value::String(version_status.into()),
    );
    object.insert(
        "check".into(),
        serde_json::json!({
            "source": source,
            "checked_at_unix_secs": checked_at_unix_secs,
            "cache_age_secs": 0,
            "next_check_after_secs": if success {
                SUCCESS_CHECK_INTERVAL_SECS
            } else {
                FAILURE_RETRY_INTERVAL_SECS
            },
        }),
    );
    object.insert(
        "compatibility".into(),
        serde_json::json!({
            "status": if !success {
                "unknown"
            } else if update_available {
                "eligible"
            } else {
                "current"
            },
            "target": platform_target(),
            "basis": if update_available {
                "gateway_manifest_asset"
            } else {
                "version_check"
            },
        }),
    );
    object.insert("update_policy".into(), update_policy(&binary_name));
    payload
}

pub(super) fn mark_cached(mut payload: Value, cache_age_secs: u64) -> Value {
    if let Some(check) = payload.get_mut("check").and_then(Value::as_object_mut) {
        check.insert("source".into(), Value::String("cache".into()));
        check.insert("cache_age_secs".into(), cache_age_secs.into());
    }
    payload
}

fn update_policy(binary_name: &str) -> Value {
    let is_cli = binary_name == CLI_BINARY_NAME;
    serde_json::json!({
        "requires_user_confirmation": true,
        "apply_supported_by_this_command": is_cli,
        "apply_command": is_cli.then_some("dcc-mcp-cli update apply --yes"),
        "staged_for_next_launch": true,
        "running_server_restarted": false,
    })
}

fn platform_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_checks_expose_agent_update_contract() {
        let payload = enrich_check_payload(
            serde_json::json!({
                "update_available": true,
                "current_version": "0.20.22",
                "latest_version": "0.20.23",
                "binary_name": CLI_BINARY_NAME,
                "download_url": "https://example.invalid/dcc-mcp-cli",
                "sha256": "a".repeat(64),
            }),
            true,
            "live",
            42,
        );

        assert_eq!(payload["version_status"], "update_available");
        assert_eq!(payload["check"]["source"], "live");
        assert_eq!(payload["check"]["checked_at_unix_secs"], 42);
        assert_eq!(payload["compatibility"]["status"], "eligible");
        assert_eq!(payload["update_policy"]["requires_user_confirmation"], true);
        assert_eq!(
            payload["update_policy"]["apply_command"],
            "dcc-mcp-cli update apply --yes"
        );
        assert_eq!(payload["update_policy"]["running_server_restarted"], false);
    }

    #[test]
    fn failed_checks_are_structured_and_retry_sooner() {
        let payload = enrich_check_payload(
            serde_json::json!({
                "status": "manifest_error",
                "error": "failed_to_fetch_update_manifest",
                "message": "offline",
                "update_available": false,
            }),
            false,
            "live",
            42,
        );

        assert_eq!(payload["version_status"], "check_failed");
        assert_eq!(
            payload["check"]["next_check_after_secs"],
            FAILURE_RETRY_INTERVAL_SECS
        );
        assert_eq!(payload["compatibility"]["status"], "unknown");
    }
}
