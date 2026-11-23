use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::domain::install::{InstallPlan, InstallRequest, InstallStepAction};

use super::pip::{pip_show_args, pip_uninstall_args};
use super::policy::ask_consent;
use super::receipt::{
    lock_receipt_for_plan, read_receipt, receipt_for_uninstall, receipt_owned_paths,
    receipt_pip_target, receipt_uninstall_ownership_is_safe,
};
use super::report::{execution_report_for_plan, failed_report, safe_report_identifier};
use super::{InstallExecutionReport, InstallService, report};

impl InstallService {
    pub fn uninstall(
        &self,
        mut request: InstallRequest,
        skip_confirmation: bool,
    ) -> InstallExecutionReport {
        let requested_dcc = safe_report_identifier(&request.dcc_type, "unknown");
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
        if let Some((package, python)) = receipt_pip_target(&receipt) {
            let status = match Command::new(&python)
                .args(pip_uninstall_args(&package))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(status) => status,
                Err(error) => {
                    return failed_report(
                        report,
                        "uninstall",
                        30,
                        "UNINSTALL_STEP_FAILED",
                        Some(&error.to_string()),
                    );
                }
            };
            if !status.success() {
                return failed_report(report, "uninstall", 30, "UNINSTALL_STEP_FAILED", None);
            }
            let package_absent = match pip_package_is_absent(&python, &package) {
                Ok(absent) => absent,
                Err(message) => {
                    return failed_report(
                        report,
                        "uninstall",
                        30,
                        "UNINSTALL_STEP_FAILED",
                        Some(&message),
                    );
                }
            };
            if !package_absent {
                return failed_report(report, "uninstall", 30, "UNINSTALL_STEP_FAILED", None);
            }
        }
        for path in receipt_owned_paths(&receipt) {
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
            if let Err(error) = remove_owned_path(&path) {
                return failed_report(
                    report,
                    "uninstall",
                    30,
                    "UNINSTALL_STEP_FAILED",
                    Some(&error.to_string()),
                );
            }
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
fn remove_owned_path(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        #[cfg(windows)]
        if std::os::windows::fs::FileTypeExt::is_symlink_dir(&metadata.file_type()) {
            return fs::remove_dir(path);
        }
        return fs::remove_file(path);
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
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
