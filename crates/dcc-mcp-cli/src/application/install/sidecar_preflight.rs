//! Stop adapter-owned sidecars before replacing an installed pip package.
//!
//! The upgrade path must not mutate files under a running sidecar on Windows.
//! This preflight shells out to Core's import-light `install_lifecycle` module,
//! which only stops sidecars whose registry rows carry a `sidecar_pid` (never
//! the user's DCC host process) and classifies blocked/stop failures.

use std::process::{Command, Stdio};

use serde_json::Value;

/// Outcome of the upgrade preflight.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum UpgradePreflight {
    /// Core lifecycle metadata was unavailable; upgrade proceeds unchanged.
    NoLifecycleMetadata,
    /// Sidecars were stopped or none needed stopping; upgrade may proceed.
    Ready,
    /// A sidecar could not be stopped or needs a manual host restart.
    RequiresRestart { reason: String, detail: String },
}

/// Ask Core's install lifecycle to stop stale adapter-owned sidecars.
///
/// `python` is the target DCC interpreter and must be able to import
/// `dcc_mcp_core`. When it cannot (for example an editable adapter install),
/// the probe is treated as "no metadata" rather than blocking the upgrade.
/// `target_version` is the adapter version about to be installed, so sidecars
/// advertising an older adapter version select the `restart_sidecar` action.
pub(super) fn prepare_sidecar_update(
    python: &str,
    dcc_type: &str,
    target_version: Option<&str>,
) -> UpgradePreflight {
    let mut args = vec![
        "-m".to_string(),
        "dcc_mcp_core.install_lifecycle".to_string(),
        "prepare-update".to_string(),
        "--dcc-type".to_string(),
        dcc_type.to_string(),
    ];
    if let Some(version) = target_version.filter(|version| !version.trim().is_empty()) {
        args.push("--target-version".to_string());
        args.push(format!("adapter={version}"));
    }
    args.push("--timeout-secs".to_string());
    args.push("5".to_string());

    let output = match Command::new(python)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(_) => return UpgradePreflight::NoLifecycleMetadata,
    };
    let parsed: Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(_) => return UpgradePreflight::NoLifecycleMetadata,
    };
    classify_lifecycle_report(&parsed)
}

/// Classify the JSON emitted by `install_lifecycle prepare-update`.
fn classify_lifecycle_report(parsed: &Value) -> UpgradePreflight {
    if parsed.get("success").and_then(Value::as_bool) == Some(true) {
        return UpgradePreflight::Ready;
    }
    if matches!(
        parsed.get("reason").and_then(Value::as_str),
        Some("missing_target_versions" | "invalid_target_version")
    ) {
        return UpgradePreflight::NoLifecycleMetadata;
    }
    let reason = parsed
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let detail = parsed
        .get("blocked")
        .map(Value::to_string)
        .or_else(|| parsed.get("stop").map(Value::to_string))
        .unwrap_or_else(|| parsed.to_string());
    UpgradePreflight::RequiresRestart { reason, detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_python_module_is_treated_as_no_metadata() {
        // A path that cannot launch never resolves to a lifecycle report.
        assert_eq!(
            prepare_sidecar_update("__dcc_mcp_missing_python__", "maya", Some("1.0.0")),
            UpgradePreflight::NoLifecycleMetadata
        );
    }

    #[test]
    fn stopped_sidecars_allow_the_upgrade() {
        let report = serde_json::json!({
            "success": true,
            "status": "ready",
            "reason": "sidecars_stopped",
            "stop": {"success": true, "results": [{"status": "stopped"}]}
        });
        assert_eq!(classify_lifecycle_report(&report), UpgradePreflight::Ready);
    }

    #[test]
    fn failed_sidecar_stop_requires_restart_before_pip_runs() {
        let report = serde_json::json!({
            "success": false,
            "status": "failed",
            "reason": "sidecar_stop_failed",
            "stop": {"success": false, "results": [{"status": "still_running"}]}
        });
        assert_eq!(
            classify_lifecycle_report(&report),
            UpgradePreflight::RequiresRestart {
                reason: "sidecar_stop_failed".into(),
                detail: serde_json::json!({"success": false, "results": [{"status": "still_running"}]}).to_string(),
            }
        );
    }

    #[test]
    fn host_managed_runtime_blocks_without_stopping_it() {
        let report = serde_json::json!({
            "success": false,
            "status": "blocked",
            "reason": "manual_runtime_action_required",
            "blocked": [{"action": "manual_restart_required"}]
        });
        assert!(matches!(
            classify_lifecycle_report(&report),
            UpgradePreflight::RequiresRestart { reason, .. } if reason == "manual_runtime_action_required"
        ));
    }
}
