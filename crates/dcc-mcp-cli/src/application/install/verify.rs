//! Standalone target-import verification never executes an installation plan.

use std::path::PathBuf;

use super::InstallService;
use super::report::{
    InstallExecutionReport, execution_report_for_plan, failed_report, plan_failure_report,
    safe_report_identifier, success_next_steps,
};
use crate::application::adapter_import::{AdapterImportTarget, import_module, probe_target};
use crate::domain::install::{InstallRequest, InstallStepAction};

impl InstallService {
    /// Verify pinned package metadata and imports in the explicitly selected interpreter.
    /// A successful import does not prove that the DCC host is ready.
    pub fn verify(&self, request: InstallRequest) -> InstallExecutionReport {
        let dcc = safe_report_identifier(&request.dcc_type, "unknown");
        let plan = match self.plan(request) {
            Ok(plan) => plan,
            Err(error) => return plan_failure_report(dcc, &error),
        };
        let mut report = execution_report_for_plan(&plan);
        report.steps.retain(|step| step.id == "verify");
        let Some((package, version, python)) =
            plan.steps
                .iter()
                .find_map(|step| match step.action.as_ref()? {
                    InstallStepAction::PipInstall {
                        package,
                        version,
                        python,
                        ..
                    } => Some((package, version, python)),
                    _ => None,
                })
        else {
            return failed_report(
                report,
                "verify",
                40,
                "VERIFY_UNSUPPORTED_INSTALLATION",
                None,
            );
        };
        let Some(python) = python.as_deref().filter(|python| !python.is_empty()) else {
            return failed_report(report, "verify", 40, "TARGET_PYTHON_REQUIRED", None);
        };
        let target = AdapterImportTarget {
            dcc_type: plan.dcc_type.clone(),
            module: import_module(&plan.adapter, package),
            distribution: package.clone(),
            python: Some(PathBuf::from(python)),
            python_source: Some("install_plan".into()),
        };
        let probe = probe_target(&target);
        let code = if probe["status"] == "version_mismatch" {
            Some("ADAPTER_VERSION_MISMATCH")
        } else if probe["status"] == "unavailable" {
            Some("ADAPTER_IMPORT_UNAVAILABLE")
        } else if probe["status"] != "ok" {
            Some("ADAPTER_IMPORT_FAILED")
        } else if version
            .as_deref()
            .is_none_or(|expected| probe["dist_version"] != expected)
        {
            Some("ADAPTER_VERSION_MISMATCH")
        } else {
            None
        };
        if let Some(step) = report.steps.first_mut() {
            step.status = if code.is_some() { "failed" } else { "ok" }.into();
        }
        if let Some(code) = code {
            return failed_report(report, "verify", 40, code, None);
        }
        report.status = "partial".into();
        report.stage = "verify".into();
        report.exit_code = 0;
        report.next_steps = success_next_steps(&report.dcc_type);
        report.verify.failure_stage = Some("host-readiness".into());
        report.verify.failure_reason = Some("LIVE_DCC_VERIFICATION_REQUIRED".into());
        report
    }
}
