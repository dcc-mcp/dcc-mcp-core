use serde::Serialize;

use crate::domain::install::{InstallPlan, InstallStepAction, normalized_dcc_key};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallExecutionReport {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallStepReport {
    pub id: String,
    pub status: String,
    pub rollback: InstallStepRollbackReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallStepRollbackReport {
    pub attempted: bool,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallRollbackReport {
    pub attempted: bool,
    pub status: String,
    pub failure_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallReportNextStep {
    pub id: String,
    pub description: String,
    pub why: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallVerifyReport {
    pub directly_usable: bool,
    pub failure_stage: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
        },
        error: None,
    }
}

pub(super) fn empty_execution_report(dcc_type: String) -> InstallExecutionReport {
    InstallExecutionReport {
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
        InstallStepAction::RegisterDcc { .. } => "register-dcc",
        InstallStepAction::Verify => "verify",
    }
    .into()
}

pub(super) fn action_failure(action: &InstallStepAction) -> (&'static str, i32, &'static str) {
    match action {
        InstallStepAction::GitClone { .. } | InstallStepAction::ZipExtract { .. } => {
            ("acquire", 20, "ACQUIRE_STEP_FAILED")
        }
        InstallStepAction::Verify => ("verify", 40, "VERIFY_FAILED"),
        InstallStepAction::PipInstall { .. }
        | InstallStepAction::PathCopy { .. }
        | InstallStepAction::RegisterDcc { .. } => ("install", 30, "INSTALL_STEP_FAILED"),
    }
}

pub(super) fn success_next_steps(dcc_type: &str) -> Vec<InstallReportNextStep> {
    vec![
        InstallReportNextStep {
            id: "inspect-runtime".into(),
            description: "Inspect safe local runtime diagnostics.".into(),
            why: "Diagnostics confirm the installed CLI and local gateway prerequisites.".into(),
            command: vec!["dcc-mcp-cli".into(), "doctor".into()],
        },
        InstallReportNextStep {
            id: "confirm-readiness".into(),
            description: format!("Confirm that a live {dcc_type} adapter becomes ready."),
            why: "Host readiness is separate from local installation success.".into(),
            command: vec![
                "dcc-mcp-cli".into(),
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
            command: vec!["dcc-mcp-cli".into(), "doctor".into()],
        },
        InstallReportNextStep {
            id: "review-plan".into(),
            description: format!("Review a fresh non-mutating install plan for {dcc_type}."),
            why: "The plan can be reviewed safely before another authorized execution.".into(),
            command: vec![
                "dcc-mcp-cli".into(),
                "install".into(),
                "--dcc-type".into(),
                dcc_type.into(),
                "--json".into(),
            ],
        },
    ]
}
