use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::domain::install::{InstallPlan, InstallRequest, InstallStepAction};

use super::lifecycle::{RecoveryEntry, RemovalError, RemovalTransaction, resume_pending};
use super::pip::{pip_show_args, pip_uninstall_args};
use super::policy::ask_consent;
use super::receipt::{
    lock_receipt_for_plan, read_receipt, receipt_for_uninstall, receipt_owned_paths,
    receipt_pip_target, receipt_uninstall_ownership_is_safe,
};
use super::report::{
    execution_report_for_plan, failed_report, requires_restart_report, safe_report_identifier,
};
use super::{InstallExecutionReport, InstallService, report};

#[derive(Debug, Deserialize, Serialize)]
struct PendingRemoval {
    schema_version: u8,
    operation: String,
    entries: Vec<RecoveryEntry>,
}

fn pending_marker_path(receipt_path: &Path) -> PathBuf {
    let file_name = receipt_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("install-receipt.json");
    receipt_path.with_file_name(format!("{file_name}.uninstall-pending.json"))
}

fn read_pending_marker(receipt_path: &Path) -> Result<Option<PendingRemoval>, String> {
    let path = pending_marker_path(receipt_path);
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content)
            .map(Some)
            .map_err(|error| format!("invalid pending uninstall marker: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to read pending uninstall marker: {error}")),
    }
}

fn write_pending_marker(
    receipt_path: &Path,
    operation: &str,
    entries: &[RecoveryEntry],
) -> Result<PathBuf, String> {
    let path = pending_marker_path(receipt_path);
    let marker = PendingRemoval {
        schema_version: 1,
        operation: operation.into(),
        entries: entries.to_vec(),
    };
    let content = serde_json::to_vec_pretty(&marker)
        .map_err(|error| format!("failed to encode pending uninstall marker: {error}"))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".dcc-mcp-uninstall-pending-")
        .tempfile_in(parent)
        .map_err(|error| format!("failed to create pending uninstall marker: {error}"))?;
    temporary
        .write_all(&content)
        .map_err(|error| format!("failed to write pending uninstall marker: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("failed to flush pending uninstall marker: {error}"))?;
    temporary
        .persist(&path)
        .map_err(|error| format!("failed to commit pending uninstall marker: {}", error.error))?;
    Ok(path)
}

fn remove_pending_marker(receipt_path: &Path) -> Result<(), String> {
    let path = pending_marker_path(receipt_path);
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to remove pending uninstall marker: {error}"
        )),
    }
}

fn recovery_message(
    receipt_path: &Path,
    operation: &str,
    entries: &[RecoveryEntry],
    message: &str,
) -> String {
    if entries.is_empty() {
        return message.into();
    }
    match write_pending_marker(receipt_path, operation, entries) {
        Ok(path) => format!("{message}; recovery marker: {}", path.display()),
        Err(error) => format!(
            "{message}; {error}; pending recovery ({operation}): {}",
            format_recovery_entries(entries)
        ),
    }
}

fn existing_recovery_message(
    receipt_path: &Path,
    operation: &str,
    entries: &[RecoveryEntry],
    message: &str,
) -> String {
    let marker = pending_marker_path(receipt_path);
    if marker.exists() {
        format!("{message}; recovery marker: {}", marker.display())
    } else {
        format!(
            "{message}; pending recovery ({operation}): {}",
            format_recovery_entries(entries)
        )
    }
}

fn format_recovery_entries(entries: &[RecoveryEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "{} -> {}",
                entry.original.display(),
                entry.recovery_dir.display()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl InstallService {
    pub fn uninstall(
        &self,
        mut request: InstallRequest,
        skip_confirmation: bool,
    ) -> InstallExecutionReport {
        let requested_dcc = safe_report_identifier(&request.dcc_type, "unknown");
        let catalog_path = request.catalog_path.clone();
        let adobe_debug_root = request.adobe_debug_root.clone();
        request.version = None;
        let plan = match self.plan(request) {
            Ok(plan) => plan,
            Err(error) => return report::plan_failure_report(requested_dcc, &error),
        };
        let mut report = execution_report_for_plan(&plan);
        let Some((receipt_path, _initial_receipt)) = (match receipt_for_uninstall(&plan) {
            Ok(receipt) => receipt,
            Err(message) => {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_RECEIPT_INVALID",
                    Some(&message),
                );
            }
        }) else {
            if install_state_is_absent(&plan) {
                return successful_uninstall_report(report);
            }
            return failed_report(report, "uninstall", 30, "UNINSTALL_RECEIPT_REQUIRED", None);
        };
        let _receipt_lock = match lock_receipt_for_plan(&plan) {
            Ok(lock) => lock,
            Err(message) => {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_OWNERSHIP_CHECK_FAILED",
                    Some(&message),
                );
            }
        };
        let receipt = match read_receipt(&receipt_path) {
            Ok(Some(receipt)) => receipt,
            Ok(None) => {
                return failed_report(report, "uninstall", 30, "UNINSTALL_RECEIPT_REQUIRED", None);
            }
            Err(message) => {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_RECEIPT_INVALID",
                    Some(&message),
                );
            }
        };
        report.adapter_version = safe_report_identifier(&receipt.adapter_version, "unknown");
        if let Some(pending) = match read_pending_marker(&receipt_path) {
            Ok(pending) => pending,
            Err(message) => {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_STEP_FAILED",
                    Some(&message),
                );
            }
        } {
            if pending.schema_version != 1 {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_STEP_FAILED",
                    Some("unsupported pending uninstall marker schema"),
                );
            }
            if let Err(error) = resume_pending(&pending.operation, &pending.entries) {
                return match error {
                    RemovalError::RequiresRestart { path, message, .. } => {
                        let message = existing_recovery_message(
                            &receipt_path,
                            &pending.operation,
                            &pending.entries,
                            &message,
                        );
                        requires_restart_report(
                            report,
                            &receipt_path,
                            &path,
                            &message,
                            catalog_path.as_deref(),
                            adobe_debug_root.as_deref(),
                        )
                    }
                    RemovalError::Failed { path, message, .. } => failed_report(
                        report,
                        "uninstall",
                        30,
                        "UNINSTALL_STEP_FAILED",
                        Some(&format!("{}: {message}", path.display())),
                    ),
                };
            }
            if let Err(message) = remove_pending_marker(&receipt_path) {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_STEP_FAILED",
                    Some(&message),
                );
            }
        }
        let ownership_safe = match receipt_uninstall_ownership_is_safe(&plan, &receipt) {
            Ok(safe) => safe,
            Err(message) => {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_OWNERSHIP_CHECK_FAILED",
                    Some(&message),
                );
            }
        };
        if !ownership_safe {
            return failed_report(
                report,
                "uninstall",
                30,
                "UNINSTALL_OWNERSHIP_MISMATCH",
                None,
            );
        }
        if !skip_confirmation {
            match ask_consent("Remove the receipted adapter state? [y/N]") {
                Ok(true) => {}
                Ok(false) => return failed_report(report, "preflight", 10, "CONSENT_DENIED", None),
                Err(_) => {
                    return failed_report(report, "preflight", 10, "CONSENT_INPUT_FAILED", None);
                }
            }
        }
        match receipt_uninstall_ownership_is_safe(&plan, &receipt) {
            Ok(true) => {}
            Ok(false) => {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_OWNERSHIP_MISMATCH",
                    None,
                );
            }
            Err(message) => {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_OWNERSHIP_CHECK_FAILED",
                    Some(&message),
                );
            }
        }
        let owned_paths = receipt_owned_paths(&receipt);
        let removal = match RemovalTransaction::stage(owned_paths) {
            Ok(transaction) => transaction,
            Err(RemovalError::RequiresRestart {
                path,
                message,
                recovery,
            }) => {
                let message = recovery_message(&receipt_path, "restore", &recovery, &message);
                return requires_restart_report(
                    report,
                    &receipt_path,
                    &path,
                    &message,
                    catalog_path.as_deref(),
                    adobe_debug_root.as_deref(),
                );
            }
            Err(RemovalError::Failed {
                path,
                message,
                recovery,
            }) => {
                let message = recovery_message(&receipt_path, "restore", &recovery, &message);
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_STEP_FAILED",
                    Some(&format!("{}: {message}", path.display())),
                );
            }
        };
        let mut removal = Some(removal);
        if let Some((package, python)) = receipt_pip_target(&receipt) {
            let status = match Command::new(&python)
                .args(pip_uninstall_args(&package))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(status) => status,
                Err(error) => {
                    return restore_after_failure(
                        report,
                        &receipt_path,
                        &mut removal,
                        "UNINSTALL_STEP_FAILED",
                        Some(error.to_string()),
                    );
                }
            };
            if !status.success() {
                return restore_after_failure(
                    report,
                    &receipt_path,
                    &mut removal,
                    "UNINSTALL_STEP_FAILED",
                    None,
                );
            }
            let package_absent = match pip_package_is_absent(&python, &package) {
                Ok(absent) => absent,
                Err(message) => {
                    return restore_after_failure(
                        report,
                        &receipt_path,
                        &mut removal,
                        "UNINSTALL_STEP_FAILED",
                        Some(message),
                    );
                }
            };
            if !package_absent {
                return restore_after_failure(
                    report,
                    &receipt_path,
                    &mut removal,
                    "UNINSTALL_STEP_FAILED",
                    None,
                );
            }
        }
        if let Some(transaction) = removal.take()
            && let Err(error) = transaction.commit()
        {
            return match error {
                RemovalError::RequiresRestart {
                    path,
                    message,
                    recovery,
                } => {
                    let message = recovery_message(&receipt_path, "cleanup", &recovery, &message);
                    requires_restart_report(
                        report,
                        &receipt_path,
                        &path,
                        &message,
                        catalog_path.as_deref(),
                        adobe_debug_root.as_deref(),
                    )
                }
                RemovalError::Failed {
                    path,
                    message,
                    recovery,
                } => {
                    let message = recovery_message(&receipt_path, "cleanup", &recovery, &message);
                    failed_report(
                        report,
                        "uninstall",
                        30,
                        "UNINSTALL_STEP_FAILED",
                        Some(&format!("{}: {message}", path.display())),
                    )
                }
            };
        }
        if let Err(message) = remove_pending_marker(&receipt_path) {
            return failed_report(
                report,
                "uninstall",
                30,
                "UNINSTALL_STEP_FAILED",
                Some(&message),
            );
        }
        if let Err(error) = fs::remove_file(&receipt_path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return failed_report(
                report,
                "uninstall",
                30,
                "UNINSTALL_STEP_FAILED",
                Some(&error.to_string()),
            );
        }
        for step in &mut report.steps {
            if step.id != "verify" && step.id != "register-dcc" {
                step.status = "ok".into();
            }
        }
        let mut report = successful_uninstall_report(report);
        report.receipt_path = Some(receipt_path.display().to_string());
        report
    }
}

fn restore_after_failure(
    report: InstallExecutionReport,
    receipt_path: &Path,
    removal: &mut Option<RemovalTransaction>,
    code: &str,
    message: Option<String>,
) -> InstallExecutionReport {
    if let Some(transaction) = removal.as_mut()
        && let Err(error) = transaction.restore()
    {
        let recovery = transaction.retain_recovery();
        let primary = message.unwrap_or_else(|| code.into());
        let detail = recovery_message(
            receipt_path,
            "restore",
            &recovery,
            &format!("{primary}; rollback failed: {error}"),
        );
        return failed_report(report, "rollback", 30, "ROLLBACK_FAILED", Some(&detail));
    }
    failed_report(report, "uninstall", 30, code, message.as_deref())
}

fn successful_uninstall_report(mut report: InstallExecutionReport) -> InstallExecutionReport {
    report.status = "ok".into();
    report.stage = "complete".into();
    report.exit_code = 0;
    report.next_steps.clear();
    report.receipt_path = None;
    report.verify.failure_stage = None;
    report.verify.failure_reason = None;
    report
}

pub(super) fn install_state_is_absent(plan: &InstallPlan) -> bool {
    for step in &plan.steps {
        let Some(action) = step.action.as_ref() else {
            continue;
        };
        match action {
            InstallStepAction::GitClone { dest, .. }
            | InstallStepAction::ZipExtract { dest, .. }
            | InstallStepAction::PathCopy { dest, .. }
            | InstallStepAction::AdobePluginLink { dest, .. } => match fs::symlink_metadata(dest) {
                Ok(_) => return false,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return false,
            },
            InstallStepAction::PipInstall {
                package, python, ..
            } => {
                let Some(python) = python.as_deref().filter(|value| !value.trim().is_empty())
                else {
                    return false;
                };
                let output = match Command::new(python)
                    .args(pip_show_args(package))
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .output()
                {
                    Ok(output) => output,
                    Err(_) => return false,
                };
                if output.status.success() {
                    return false;
                }
                if !String::from_utf8_lossy(&output.stderr)
                    .to_ascii_lowercase()
                    .contains("package(s) not found")
                {
                    return false;
                }
            }
            InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => {}
        }
    }
    true
}

fn pip_package_is_absent(python: &str, package: &str) -> Result<bool, String> {
    let output = Command::new(python)
        .args(pip_show_args(package))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("failed to verify pip removal: {error}"))?;
    if output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stderr)
        .to_ascii_lowercase()
        .contains("package(s) not found"))
}
