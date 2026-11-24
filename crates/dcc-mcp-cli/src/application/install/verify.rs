//! Standalone target-import verification never executes an installation plan.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::InstallService;
use super::report::{
    InstallExecutionReport, execution_report_for_plan, failed_report, plan_failure_report,
    safe_report_identifier, success_next_steps,
};
use crate::application::adapter_import::{
    AdapterImportTarget, core_compatibility_for_requirement, import_module, probe_target,
};
use crate::domain::install::{InstallRequest, InstallStepAction};

fn observed_version(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(|value| safe_report_identifier(value, "unknown"))
        .filter(|value| value != "unknown")
}

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
        report.verify.observed_adapter_version = observed_version(&probe["dist_version"])
            .or_else(|| observed_version(&probe["module_version"]));
        report.verify.expected_adapter_version = version
            .as_deref()
            .map(|value| safe_report_identifier(value, "unknown"))
            .filter(|value| value != "unknown");
        report.verify.observed_distribution_source = observed_version(&probe["dist_source"]);
        if probe
            .get("core_compatibility")
            .is_some_and(|value| !value.is_null())
        {
            report.verify.observed_core_compatibility = Some(probe["core_compatibility"].clone());
        }
        let catalog_core = super::target_core_requirement(&plan).map(|requirement| {
            let core = core_compatibility_for_requirement(
                probe
                    .as_object()
                    .expect("adapter import probe is a JSON object"),
                &requirement,
            );
            if probe.get("status").and_then(Value::as_str) != Some("incompatible_core") {
                report.verify.observed_core_compatibility = Some(json!({
                    "requirement": core.declaration,
                    "running": core.running,
                    "status": core.verdict.as_str(),
                    "declaration_valid": core.bound.is_ok(),
                }));
            }
            core
        });
        let pinned_version = version.as_deref();
        let code = if probe["status"] == "version_mismatch" {
            Some("ADAPTER_VERSION_MISMATCH")
        } else if probe["status"] == "incompatible_core" {
            Some("ADAPTER_CORE_INCOMPATIBLE")
        } else if probe["status"] == "unavailable" {
            Some("ADAPTER_IMPORT_UNAVAILABLE")
        } else if probe["status"] != "ok" {
            Some("ADAPTER_IMPORT_FAILED")
        } else if pinned_version.is_none_or(|expected| probe["dist_version"] != expected) {
            Some("ADAPTER_VERSION_MISMATCH")
        } else if catalog_core.is_some_and(|core| !core.is_ok()) {
            Some("ADAPTER_CORE_INCOMPATIBLE")
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
        super::report::apply_readiness(report, super::readiness::probe_ready(&plan.dcc_type), true)
    }
}
