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
    /// Lifecycle execution failed before a trustworthy report was produced.
    ExecutionFailed { detail: String },
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
        Err(error) => {
            return UpgradePreflight::ExecutionFailed {
                detail: format!("failed to execute lifecycle preflight: {error}"),
            };
        }
    };
    classify_lifecycle_output(&output.stdout, &output.stderr, output.status.success())
}

fn classify_lifecycle_output(
    stdout: &[u8],
    stderr: &[u8],
    process_succeeded: bool,
) -> UpgradePreflight {
    let parsed: Value = match serde_json::from_slice(stdout) {
        Ok(value) => value,
        Err(error) => {
            let stderr = String::from_utf8_lossy(stderr).trim().to_string();
            if stderr.contains("No module named 'dcc_mcp_core'")
                || stderr.contains("No module named \"dcc_mcp_core\"")
            {
                return UpgradePreflight::NoLifecycleMetadata;
            }
            return UpgradePreflight::ExecutionFailed {
                detail: if stderr.is_empty() {
                    format!("lifecycle preflight returned invalid JSON: {error}")
                } else {
                    format!("lifecycle preflight returned invalid JSON: {error}: {stderr}")
                },
            };
        }
    };
    if !process_succeeded && parsed.get("success").and_then(Value::as_bool) == Some(true) {
        return UpgradePreflight::ExecutionFailed {
            detail: "lifecycle preflight exited unsuccessfully after reporting success".into(),
        };
    }
    classify_lifecycle_report(&parsed)
}

/// Classify the JSON emitted by `install_lifecycle prepare-update`.
fn classify_lifecycle_report(parsed: &Value) -> UpgradePreflight {
    if parsed.get("success").and_then(Value::as_bool) == Some(true) {
        return UpgradePreflight::Ready;
    }
    // `install_lifecycle_cli.py prepare-update` emits the singular form when
    // no `--target-version` reaches it; the library emits the plural form.
    // Both mean "no lifecycle metadata available", not "host needs restart".
    if matches!(
        parsed.get("reason").and_then(Value::as_str),
        Some(
            "missing_target_version"
                | "missing_target_versions"
                | "invalid_target_version"
                | "invalid_target_versions"
        )
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
    fn missing_python_executable_aborts_the_preflight() {
        // A path that cannot launch is an execution failure, not proof that
        // the lifecycle module is absent from a working interpreter.
        assert!(matches!(
            prepare_sidecar_update("__dcc_mcp_missing_python__", "maya", Some("1.0.0")),
            UpgradePreflight::ExecutionFailed { .. }
        ));
    }

    /// Exact JSON emitted by `python -m dcc_mcp_core.install_lifecycle
    /// prepare-update` when no `--target-version` reaches the CLI. The real
    /// reason is singular; it must not become a false exit-50 requires-restart.
    #[test]
    fn real_cli_missing_target_version_does_not_block_upgrade() {
        let report = serde_json::json!({
            "message": "At least one --target-version KEY=VERSION is required.",
            "path": null,
            "reason": "missing_target_version",
            "requires_restart": false,
            "status": "failed",
            "success": false
        });
        assert_eq!(
            classify_lifecycle_report(&report),
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
                detail:
                    serde_json::json!({"success": false, "results": [{"status": "still_running"}]})
                        .to_string(),
            }
        );
    }

    #[test]
    fn invalid_lifecycle_output_aborts_instead_of_claiming_missing_metadata() {
        assert!(matches!(
            classify_lifecycle_output(b"not-json", b"unexpected traceback", false),
            UpgradePreflight::ExecutionFailed { .. }
        ));
    }

    #[test]
    fn missing_core_module_is_the_only_invalid_output_fallback() {
        assert_eq!(
            classify_lifecycle_output(
                b"",
                b"ModuleNotFoundError: No module named 'dcc_mcp_core'",
                false
            ),
            UpgradePreflight::NoLifecycleMetadata
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
