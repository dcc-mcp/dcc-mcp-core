use serde::{Deserialize, Serialize};

use crate::domain::install::InstallRequest;
use crate::domain::install::{InstallPlan, InstallStepAction, normalized_dcc_key};
use crate::domain::install_catalog::{CatalogProvenance, CatalogSource};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InstallExecutionReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<crate::domain::install_catalog::CatalogProvenance>,
    pub schema_version: u8,
    pub status: String,
    pub dcc_type: String,
    pub adapter_version: String,
    pub core_version: String,
    pub stage: String,
    pub exit_code: i32,
    pub steps: Vec<InstallStepReport>,
    pub rollback: InstallRollbackReport,
    pub next_steps: Vec<InstallReportNextStep>,
    pub receipt_path: Option<String>,
    pub verify: InstallVerifyReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<InstallExecutionError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InstallStepReport {
    pub id: String,
    pub status: String,
    pub rollback: InstallStepRollbackReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InstallStepRollbackReport {
    pub attempted: bool,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InstallRollbackReport {
    pub attempted: bool,
    pub status: String,
    pub failure_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InstallReportNextStep {
    pub id: String,
    pub description: String,
    pub why: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InstallVerifyReport {
    pub directly_usable: bool,
    pub failure_stage: Option<String>,
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_adapter_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_adapter_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_distribution_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_core_compatibility: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InstallExecutionError {
    pub code: String,
    pub stage: String,
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_code: Option<String>,
}

pub(super) fn execution_report_for_plan(plan: &InstallPlan) -> InstallExecutionReport {
    let dcc_type = safe_plan_dcc_type(plan);
    let steps = plan
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| InstallStepReport {
            id: step
                .action
                .as_ref()
                .map(stable_step_id)
                .unwrap_or_else(|| format!("informational-{}", index + 1)),
            status: "not_run".into(),
            rollback: InstallStepRollbackReport {
                attempted: false,
                status: "not_attempted".into(),
            },
        })
        .collect();
    InstallExecutionReport {
        catalog: plan.catalog.clone(),
        schema_version: 1,
        status: "running".into(),
        dcc_type: dcc_type.clone(),
        adapter_version: plan
            .version
            .as_deref()
            .or(plan.adapter.version.as_deref())
            .map(|value| safe_report_identifier(value, "unknown"))
            .unwrap_or_else(|| "unknown".into()),
        core_version: env!("CARGO_PKG_VERSION").into(),
        stage: "preflight".into(),
        exit_code: 0,
        steps,
        rollback: InstallRollbackReport {
            attempted: false,
            status: "not_attempted".into(),
            failure_count: 0,
        },
        next_steps: failure_next_steps(&dcc_type),
        receipt_path: None,
        verify: InstallVerifyReport {
            directly_usable: false,
            failure_stage: None,
            failure_reason: None,
            expected_adapter_version: None,
            observed_adapter_version: None,
            observed_distribution_source: None,
            observed_core_compatibility: None,
        },
        error: None,
    }
}

pub(super) fn empty_execution_report(dcc_type: String) -> InstallExecutionReport {
    InstallExecutionReport {
        catalog: None,
        schema_version: 1,
        status: "running".into(),
        dcc_type: dcc_type.clone(),
        adapter_version: "unknown".into(),
        core_version: env!("CARGO_PKG_VERSION").into(),
        stage: "preflight".into(),
        exit_code: 0,
        steps: Vec::new(),
        rollback: InstallRollbackReport {
            attempted: false,
            status: "not_attempted".into(),
            failure_count: 0,
        },
        next_steps: failure_next_steps(&dcc_type),
        receipt_path: None,
        verify: InstallVerifyReport {
            directly_usable: false,
            failure_stage: None,
            failure_reason: None,
            expected_adapter_version: None,
            observed_adapter_version: None,
            observed_distribution_source: None,
            observed_core_compatibility: None,
        },
        error: None,
    }
}

pub(super) fn failed_report(
    mut report: InstallExecutionReport,
    stage: &str,
    exit_code: i32,
    code: &str,
    primary_code: Option<&str>,
) -> InstallExecutionReport {
    report.status = if code == "ROLLBACK_FAILED" {
        "partial".into()
    } else {
        "failed".into()
    };
    report.stage = stage.into();
    report.exit_code = exit_code;
    report.verify.directly_usable = false;
    report.verify.failure_stage = Some(stage.into());
    report.verify.failure_reason = Some(code.into());
    report.error = Some(InstallExecutionError {
        code: code.into(),
        stage: stage.into(),
        exit_code,
        primary_code: primary_code.map(Into::into),
    });
    report
}

/// Promote a successful static verification to a live-usable verdict only
/// when a registered instance actually answers a complete readiness probe.
/// Any missing evidence keeps the existing `LIVE_DCC_VERIFICATION_REQUIRED`
/// boundary, so the probe can never turn an unverified install green.
pub(super) fn apply_readiness(
    mut report: InstallExecutionReport,
    evidence: Option<super::readiness::ReadinessEvidence>,
    promote_status: bool,
) -> InstallExecutionReport {
    if evidence.is_some_and(|evidence| evidence.missing.is_empty()) {
        report.verify.directly_usable = true;
        report.verify.failure_stage = None;
        report.verify.failure_reason = None;
        if promote_status && report.status == "partial" {
            report.status = "ok".into();
        }
    }
    report
}

/// Report a blocked upgrade before any file was changed.
///
/// The owning DCC host must be restarted so its sidecar releases the adapter
/// payload; retrying the install then converges without a rollback.
pub(super) fn upgrade_requires_restart_report(
    mut report: InstallExecutionReport,
    retry_request: Option<&InstallRequest>,
    reason: &str,
    detail: &str,
) -> InstallExecutionReport {
    report.status = "requires_restart".into();
    report.stage = "preflight".into();
    report.exit_code = 50;
    report.verify.directly_usable = false;
    report.verify.failure_stage = Some("preflight".into());
    report.verify.failure_reason = Some("UPGRADE_REQUIRES_RESTART".into());
    report.error = Some(InstallExecutionError {
        code: "UPGRADE_REQUIRES_RESTART".into(),
        stage: "preflight".into(),
        exit_code: 50,
        primary_code: Some(reason.into()),
    });
    report.next_steps = vec![InstallReportNextStep {
        id: "retry-after-restart".into(),
        description: format!(
            "Restart the {} host so its sidecar releases the adapter, then retry the install.",
            report.dcc_type
        ),
        why: format!("Adapter-owned sidecar blocks the upgrade ({reason}): {detail}"),
        command: retry_command(&report.dcc_type, retry_request),
    }];
    report
}

fn retry_command(dcc_type: &str, request: Option<&InstallRequest>) -> Vec<String> {
    let mut command = vec![
        "dccmcp".into(),
        "install".into(),
        "--dcc-type".into(),
        dcc_type.into(),
    ];
    if let Some(request) = request {
        if let Some(version) = request.version.as_deref() {
            command.extend(["--version".into(), version.into()]);
        }
        if let Some(catalog_path) = request.catalog_path.as_ref() {
            command.extend(["--catalog".into(), catalog_path.display().to_string()]);
        }
        if let Some(python) = request.python.as_deref() {
            command.extend(["--python".into(), python.into()]);
        }
        if let Some(dcc_path) = request.dcc_path.as_ref() {
            command.extend(["--dcc-path".into(), dcc_path.display().to_string()]);
        }
        if let Some(plugin_source) = request.plugin_source.as_ref() {
            command.extend([
                "--plugin-source".into(),
                plugin_source.display().to_string(),
            ]);
        }
        if let Some(adobe_debug_root) = request.adobe_debug_root.as_ref() {
            command.extend([
                "--adobe-debug-root".into(),
                adobe_debug_root.display().to_string(),
            ]);
        }
    }
    command.extend(["--execute".into(), "--yes".into()]);
    command
}

pub(super) fn requires_restart_report(
    mut report: InstallExecutionReport,
    receipt_path: &std::path::Path,
    locked_path: &std::path::Path,
    message: &str,
    catalog_path: Option<&std::path::Path>,
    adobe_debug_root: Option<&std::path::Path>,
) -> InstallExecutionReport {
    report.status = "requires_restart".into();
    report.stage = "uninstall".into();
    report.exit_code = 50;
    report.receipt_path = Some(receipt_path.display().to_string());
    report.verify.directly_usable = false;
    report.verify.failure_stage = Some("uninstall".into());
    report.verify.failure_reason = Some("UNINSTALL_REQUIRES_RESTART".into());
    report.error = Some(InstallExecutionError {
        code: "UNINSTALL_REQUIRES_RESTART".into(),
        stage: "uninstall".into(),
        exit_code: 50,
        primary_code: Some(message.into()),
    });
    let mut command = vec![
        "dccmcp".into(),
        "uninstall".into(),
        "--dcc-type".into(),
        report.dcc_type.clone(),
        "--yes".into(),
    ];
    if let Some(catalog_path) = catalog_path {
        command.push("--catalog".into());
        command.push(catalog_path.display().to_string());
    }
    if let Some(adobe_debug_root) = adobe_debug_root {
        command.push("--adobe-debug-root".into());
        command.push(adobe_debug_root.display().to_string());
    }
    report.next_steps = vec![InstallReportNextStep {
        id: "retry-after-restart".into(),
        description: "Restart the owning DCC host, then retry receipt-driven uninstall.".into(),
        why: format!(
            "The adapter-owned path remains locked: {}",
            locked_path.display()
        ),
        command,
    }];
    report
}

pub(super) fn plan_failure_report(
    dcc_type: String,
    error: &super::InstallError,
) -> InstallExecutionReport {
    let mut report = empty_execution_report(dcc_type);
    let code = if matches!(error, super::InstallError::CatalogRefresh(_)) {
        report.catalog = Some(CatalogProvenance::local(CatalogSource::Unavailable));
        "INSTALL_CATALOG_UNAVAILABLE"
    } else {
        "INSTALL_PLAN_FAILED"
    };
    failed_report(report, "preflight", 10, code, None)
}

pub(super) fn safe_report_identifier(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    let valid = !trimmed.is_empty()
        && trimmed.chars().count() <= 64
        && trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '-' | '_' | '.' | ' ' | '+' | '(' | ')')
        });
    if valid {
        trimmed.into()
    } else {
        fallback.into()
    }
}

pub(super) fn stable_step_id(action: &InstallStepAction) -> String {
    match action {
        InstallStepAction::PipInstall { .. } => "install-pip",
        InstallStepAction::GitClone { .. } => "install-git",
        InstallStepAction::ZipExtract { .. } => "install-zip",
        InstallStepAction::PathCopy { .. } => "install-path",
        InstallStepAction::AdobePluginLink { .. } => "install-adobe-debug-link",
        InstallStepAction::RegisterDcc { .. } => "register-dcc",
        InstallStepAction::Verify => "verify",
    }
    .into()
}

pub(super) fn action_failure(
    action: &InstallStepAction,
    error: &super::InstallError,
) -> (&'static str, i32, &'static str) {
    if matches!(error, super::InstallError::CoreRequirementFailed { .. }) {
        return ("verify", 40, "ADAPTER_CORE_INCOMPATIBLE");
    }
    match action {
        InstallStepAction::GitClone { .. } | InstallStepAction::ZipExtract { .. } => {
            ("acquire", 20, "ACQUIRE_STEP_FAILED")
        }
        InstallStepAction::Verify => ("verify", 40, "VERIFY_FAILED"),
        InstallStepAction::PipInstall { .. }
        | InstallStepAction::PathCopy { .. }
        | InstallStepAction::AdobePluginLink { .. }
        | InstallStepAction::RegisterDcc { .. } => ("install", 30, "INSTALL_STEP_FAILED"),
    }
}

pub(super) fn success_next_steps(dcc_type: &str) -> Vec<InstallReportNextStep> {
    vec![
        InstallReportNextStep {
            id: "inspect-runtime".into(),
            description: "Inspect safe local runtime diagnostics.".into(),
            why: "Diagnostics confirm the installed CLI and local gateway prerequisites.".into(),
            command: vec!["dccmcp".into(), "doctor".into()],
        },
        InstallReportNextStep {
            id: "confirm-readiness".into(),
            description: format!("Confirm that a live {dcc_type} adapter becomes ready."),
            why: "Host readiness is separate from local installation success.".into(),
            command: vec![
                "dccmcp".into(),
                "wait-ready".into(),
                "--dcc-type".into(),
                dcc_type.into(),
            ],
        },
    ]
}

fn safe_plan_dcc_type(plan: &InstallPlan) -> String {
    let normalized = normalized_dcc_key(&plan.dcc_type);
    plan.adapter
        .dcc
        .iter()
        .find(|candidate| normalized_dcc_key(candidate) == normalized)
        .map(|candidate| safe_report_identifier(candidate, "unknown"))
        .unwrap_or_else(|| safe_report_identifier(&plan.dcc_type, "unknown"))
}

fn failure_next_steps(dcc_type: &str) -> Vec<InstallReportNextStep> {
    vec![
        InstallReportNextStep {
            id: "inspect-runtime".into(),
            description: "Inspect safe local runtime diagnostics.".into(),
            why: "Diagnostics can identify a safe remediation before retrying installation.".into(),
            command: vec!["dccmcp".into(), "doctor".into()],
        },
        InstallReportNextStep {
            id: "review-plan".into(),
            description: format!("Review a fresh non-mutating install plan for {dcc_type}."),
            why: "The plan can be reviewed safely before another authorized execution.".into(),
            command: vec![
                "dccmcp".into(),
                "install".into(),
                "--dcc-type".into(),
                dcc_type.into(),
                "--json".into(),
            ],
        },
    ]
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    use crate::application::install::readiness::ReadinessEvidence;

    fn partial_report() -> InstallExecutionReport {
        let mut report = empty_execution_report("maya".into());
        report.status = "partial".into();
        report.verify.failure_stage = Some("host-readiness".into());
        report.verify.failure_reason = Some("LIVE_DCC_VERIFICATION_REQUIRED".into());
        report
    }

    fn evidence(missing: &[&str]) -> ReadinessEvidence {
        ReadinessEvidence {
            missing: missing.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn complete_readiness_promotes_a_partial_verify_to_usable() {
        let report = apply_readiness(partial_report(), Some(evidence(&[])), true);
        assert!(report.verify.directly_usable);
        assert_eq!(report.verify.failure_stage, None);
        assert_eq!(report.verify.failure_reason, None);
        assert_eq!(report.status, "ok");
    }

    #[test]
    fn incomplete_readiness_keeps_the_unverified_boundary() {
        let report = apply_readiness(partial_report(), Some(evidence(&["dispatcher"])), true);
        assert!(!report.verify.directly_usable);
        assert_eq!(
            report.verify.failure_reason.as_deref(),
            Some("LIVE_DCC_VERIFICATION_REQUIRED")
        );
        assert_eq!(report.status, "partial");
    }

    #[test]
    fn no_live_instance_keeps_the_unverified_boundary() {
        let report = apply_readiness(partial_report(), None, true);
        assert!(!report.verify.directly_usable);
        assert_eq!(report.status, "partial");
    }
}
