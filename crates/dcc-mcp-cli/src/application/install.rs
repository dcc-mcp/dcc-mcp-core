use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::domain::install_catalog::CatalogSource;
use dcc_mcp_catalog::{CatalogEntry, CoreRuntimeReport, derive_requirement};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::application::adapter_import::{
    AdapterImportTarget, core_compatibility_for_requirement, import_module, probe_target,
};
use crate::domain::install::{
    InstallPlan, InstallPlanError, InstallPlanner, InstallPolicy, InstallRequest,
    InstallStepAction, normalized_dcc_key,
};

const BUNDLED_CATALOG: &str = include_str!("../../../../dcc-mcp-catalog.yml");

mod acquire;
mod adobe;
#[cfg(test)]
mod archive_http_tests;
mod catalog;
#[cfg(test)]
mod catalog_contract_tests;
mod discovery;
mod file_ops;
mod lifecycle;
mod pip;
mod policy;
mod readiness;
mod receipt;
mod report;
mod sidecar_preflight;
mod uninstall;
mod verify;
#[cfg(test)]
use uninstall::install_state_is_absent;

use acquire::{execute_git_clone, execute_path_copy, execute_zip_extract};
pub(crate) use catalog::{is_implicit_cwd_catalog, warn_if_implicit_cwd_catalog};
pub use discovery::{DccAdapterSummary, DccTypeSummary, DccTypesCatalog};
use pip::{
    PipArtifact, pip_install_args, pip_restore_args_for_artifact, pip_show_args, pip_show_version,
    pip_uninstall_args,
};
use policy::{AutoInstallPolicy, ask_consent, render_install_policy_prompt};
use receipt::{
    commit_install_receipt, commit_install_receipt_with_lock, lock_receipt_for_plan,
    receipt_match_details, receipt_path_for_plan,
};
pub use report::{
    InstallExecutionError, InstallExecutionReport, InstallReportNextStep, InstallRollbackReport,
    InstallStepReport, InstallStepRollbackReport, InstallVerifyReport,
};
use report::{
    action_failure, apply_readiness, execution_report_for_plan, failed_report,
    safe_report_identifier, stable_step_id, success_next_steps, upgrade_requires_restart_report,
};

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("{0}")]
    CatalogRefresh(String),
    #[error(transparent)]
    Catalog(#[from] dcc_mcp_catalog::CatalogError),
    #[error(transparent)]
    Plan(#[from] InstallPlanError),
    #[error("consent denied by user")]
    ConsentDenied,
    #[error("step '{step}' failed: {message}")]
    StepFailed { step: String, message: String },
    #[error("target Core version '{running}' does not satisfy '{requirement}': {reason}")]
    CoreRequirementFailed {
        requirement: String,
        running: String,
        verdict: String,
        declaration_valid: bool,
        reason: String,
    },
    #[error("sidecar blocks upgrade: {reason}")]
    UpgradeRequiresRestart { reason: String, detail: String },
    #[error("rollback of step '{step}' failed: {message}")]
    RollbackFailed { step: String, message: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Describes how to undo a completed step.
#[derive(Debug)]
enum StepRollback {
    /// Remove a file or directory that was created.
    RemovePath(PathBuf),
    /// Run a shell command to revert.
    Command { program: String, args: Vec<String> },
    /// Restore a pinned wheel and remove its retained staging file afterward.
    PipCommand {
        program: String,
        args: Vec<String>,
        artifact: Box<PipArtifact>,
    },
}

#[derive(Debug)]
enum StepExecution {
    Completed(Option<StepRollback>),
    Deferred,
}

pub struct InstallService {
    default_catalog_path: Option<PathBuf>,
    auto_install_policy: AutoInstallPolicy,
    catalog_snapshot: Option<catalog::ResolvedCatalog>,
    catalog_error: Option<String>,
    require_fresh_catalog: bool,
}

impl InstallService {
    /// Adapter catalog entries from the bundled or explicitly supplied catalog.
    ///
    /// Read-only view for sibling application modules (e.g. the `doctor`
    /// adapter-import probe) that need adapter package/interpreter metadata.
    pub(crate) fn catalog_entries(
        &self,
        requested_path: Option<&Path>,
    ) -> Result<Vec<CatalogEntry>, InstallError> {
        self.load_entries(requested_path)
    }

    #[must_use]
    pub fn new(default_catalog_path: PathBuf) -> Self {
        Self {
            default_catalog_path: Some(default_catalog_path),
            auto_install_policy: AutoInstallPolicy::from_env(),
            catalog_snapshot: None,
            catalog_error: None,
            require_fresh_catalog: false,
        }
    }

    /// Build a service whose implicit catalog is the release catalog embedded
    /// in this binary. Custom catalogs must be passed explicitly in a request.
    #[must_use]
    pub fn bundled() -> Self {
        Self {
            default_catalog_path: None,
            auto_install_policy: AutoInstallPolicy::from_env(),
            catalog_snapshot: None,
            catalog_error: None,
            require_fresh_catalog: false,
        }
    }

    #[cfg(test)]
    fn with_auto_install_policy(
        default_catalog_path: PathBuf,
        auto_install_policy: AutoInstallPolicy,
    ) -> Self {
        Self {
            default_catalog_path: Some(default_catalog_path),
            auto_install_policy,
            catalog_snapshot: None,
            catalog_error: None,
            require_fresh_catalog: false,
        }
    }

    /// Generate an install plan (display-only, no execution).
    pub fn plan(&self, request: InstallRequest) -> Result<InstallPlan, InstallError> {
        let catalog = self.load_catalog(request.catalog_path.as_deref())?;
        let install_env = crate::domain::install::python::InstallEnv::from_process();
        let host_candidates = Self::host_candidates(&request);
        let mut plan = InstallPlanner::plan_with_candidates(
            &catalog.entries,
            request,
            &install_env,
            &host_candidates,
        )?;
        plan.catalog = Some(catalog.provenance);
        Ok(self.apply_auto_install_policy(plan))
    }

    fn host_candidates(
        request: &InstallRequest,
    ) -> Vec<crate::domain::install::python::HostCandidate> {
        let mut candidates = Vec::new();
        if let Some(path) = request.dcc_path.as_deref() {
            candidates.push(crate::domain::install::python::HostCandidate {
                path: path.to_path_buf(),
                explicit: true,
            });
        }

        let dcc_key = normalized_dcc_key(&request.dcc_type);
        if let Some(def) = crate::application::host::manifest::bundled().find(&dcc_key) {
            let env = crate::application::host::detect::HostEnv::from_process();
            for candidate in crate::application::host::detect::install_candidates(
                def,
                &env,
                request.dcc_path.as_deref(),
            ) {
                if !candidates
                    .iter()
                    .any(|existing| existing.path == candidate.path)
                {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    }

    /// Generate and execute an install plan with user consent.
    ///
    /// Execution always returns one machine-readable Install SOP v1 report.
    /// Internal errors are reduced to stable public codes before serialization.
    pub fn execute(
        &self,
        request: InstallRequest,
        skip_confirmation: bool,
    ) -> InstallExecutionReport {
        let requested_dcc = safe_report_identifier(&request.dcc_type, "unknown");
        match self.plan(request) {
            Ok(plan) => self.execute_plan(&plan, skip_confirmation),
            Err(error) => report::plan_failure_report(requested_dcc, &error),
        }
    }

    fn apply_auto_install_policy(&self, mut plan: InstallPlan) -> InstallPlan {
        if self.auto_install_policy.enabled {
            plan.install_policy = InstallPolicy::enabled();
            return plan;
        }

        let prompt = render_install_policy_prompt(&self.auto_install_policy.prompt_template, &plan);
        plan.install_policy = InstallPolicy::disabled(prompt);
        plan
    }

    fn execute_plan(&self, plan: &InstallPlan, skip_confirmation: bool) -> InstallExecutionReport {
        let skipped_actions = RefCell::new(Vec::new());
        let acquired_actions = RefCell::new(Vec::new());
        let receipt_lock = match lock_receipt_for_plan(plan) {
            Ok(lock) => lock,
            Err(message) => {
                return failed_report(
                    execution_report_for_plan(plan),
                    "preflight",
                    30,
                    "INSTALL_RECEIPT_LOCK_FAILED",
                    Some(&message),
                );
            }
        };
        self.execute_plan_with_commit(
            plan,
            skip_confirmation,
            // A failed preflight must never fail open: without the sidecar
            // stop, a Windows file lock could break the upgrade mid-transaction.
            |plan| match prepare_sidecar_upgrade(plan) {
                Ok(()) => Ok(()),
                Err(InstallError::UpgradeRequiresRestart { reason, detail }) => {
                    Err(InstallError::UpgradeRequiresRestart { reason, detail })
                }
                Err(error) => Err(InstallError::StepFailed {
                    step: "sidecar-preflight".into(),
                    message: error.to_string(),
                }),
            },
            |action, plan| {
                let result = if matches!(action, InstallStepAction::Verify) {
                    execute_verify(plan).map(StepExecution::Completed)
                } else if is_receipt_action(action) {
                    let (matches_receipt, validated_digest) = receipt_match_details(plan, action)
                        .map_err(|message| {
                        InstallError::StepFailed {
                            step: "install-receipt".into(),
                            message,
                        }
                    })?;
                    if matches_receipt {
                        skipped_actions
                            .borrow_mut()
                            .push((action.clone(), validated_digest));
                        Ok(StepExecution::Completed(None))
                    } else {
                        execute_action(action)
                    }
                } else {
                    execute_action(action)
                };
                if matches!(&result, Ok(StepExecution::Completed(Some(_)))) {
                    acquired_actions.borrow_mut().push(action.clone());
                }
                result
            },
            execute_rollback,
            |plan| {
                let commit = match receipt_lock.as_ref() {
                    Some(lock) => commit_install_receipt_with_lock(
                        plan,
                        &skipped_actions.borrow(),
                        &acquired_actions.borrow(),
                        lock,
                    ),
                    None => commit_install_receipt(
                        plan,
                        &skipped_actions.borrow(),
                        &acquired_actions.borrow(),
                    ),
                };
                commit
                    .map(|written| written.then(|| receipt_path_for_plan(plan)))
                    .map_err(|message| InstallError::StepFailed {
                        step: "install-receipt".into(),
                        message,
                    })
            },
        )
    }

    #[cfg(test)]
    fn execute_plan_with<E, R>(
        &self,
        plan: &InstallPlan,
        skip_confirmation: bool,
        execute_step: E,
        rollback_step: R,
    ) -> InstallExecutionReport
    where
        E: FnMut(&InstallStepAction, &InstallPlan) -> Result<StepExecution, InstallError>,
        R: FnMut(&StepRollback) -> Result<(), InstallError>,
    {
        self.execute_plan_with_commit(
            plan,
            skip_confirmation,
            |_| Ok(()),
            execute_step,
            rollback_step,
            |_| Ok(None),
        )
    }

    fn execute_plan_with_commit<P, E, R, C>(
        &self,
        plan: &InstallPlan,
        skip_confirmation: bool,
        mut preflight: P,
        mut execute_step: E,
        mut rollback_step: R,
        mut commit: C,
    ) -> InstallExecutionReport
    where
        P: FnMut(&InstallPlan) -> Result<(), InstallError>,
        E: FnMut(&InstallStepAction, &InstallPlan) -> Result<StepExecution, InstallError>,
        R: FnMut(&StepRollback) -> Result<(), InstallError>,
        C: FnMut(&InstallPlan) -> Result<Option<PathBuf>, InstallError>,
    {
        let mut report = execution_report_for_plan(plan);

        if self.require_fresh_catalog
            && plan.catalog.as_ref().is_some_and(|catalog| {
                !catalog.latest_checked && catalog.source != CatalogSource::Explicit
            })
        {
            eprintln!(
                "Latest installation catalog could not be checked. Reconnect or explicitly use --offline."
            );
            return failed_report(report, "preflight", 10, "INSTALL_CATALOG_UNAVAILABLE", None);
        }

        if !plan.install_policy.auto_install_enabled {
            eprintln!("Automatic installation is disabled by policy.");
            return failed_report(report, "preflight", 10, "AUTO_INSTALL_DISABLED", None);
        }

        let executable_steps = plan
            .steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| step.action.as_ref().map(|action| (index, action)))
            .collect::<Vec<_>>();

        if executable_steps.is_empty() {
            eprintln!("No executable steps in the install plan.");
            return failed_report(report, "preflight", 10, "NO_EXECUTABLE_STEPS", None);
        }

        eprintln!(
            "Install execution requires {} step(s).",
            executable_steps.len()
        );
        if !skip_confirmation {
            match ask_consent("Proceed with installation? [Y/n]") {
                Ok(true) => {}
                Ok(false) => {
                    return failed_report(report, "preflight", 10, "CONSENT_DENIED", None);
                }
                Err(_) => {
                    return failed_report(report, "preflight", 10, "CONSENT_INPUT_FAILED", None);
                }
            }
        }

        if plan
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.expires_at)
            .is_some_and(|expires| catalog::now_seconds().map_or(true, |now| expires <= now))
        {
            return failed_report(report, "preflight", 10, "INSTALL_CATALOG_EXPIRED", None);
        }

        match preflight(plan) {
            Ok(()) => {}
            Err(InstallError::UpgradeRequiresRestart { reason, detail }) => {
                return upgrade_requires_restart_report(
                    report,
                    plan.retry_request.as_ref(),
                    &reason,
                    &detail,
                );
            }
            Err(error) => {
                return failed_report(
                    report,
                    "preflight",
                    10,
                    "SIDECAR_PREFLIGHT_FAILED",
                    Some(&error.to_string()),
                );
            }
        }

        let mut completed: Vec<(usize, Option<StepRollback>)> = Vec::new();
        let mut has_deferred_steps = false;

        for (ordinal, (step_index, action)) in executable_steps.iter().enumerate() {
            let step_id = stable_step_id(action);
            eprint!(
                "  [{}/{}] {step_id} ... ",
                ordinal + 1,
                executable_steps.len()
            );
            match execute_step(action, plan) {
                Ok(StepExecution::Completed(rollback)) => {
                    eprintln!("OK");
                    report.steps[*step_index].status = "ok".into();
                    report.steps[*step_index].rollback.status = if rollback.is_some() {
                        "available".into()
                    } else {
                        "not_available".into()
                    };
                    completed.push((*step_index, rollback));
                }
                Ok(StepExecution::Deferred) => {
                    eprintln!("DEFERRED");
                    report.steps[*step_index].status = "deferred".into();
                    report.steps[*step_index].rollback.status = "not_available".into();
                    has_deferred_steps = true;
                }
                Err(error) => {
                    eprintln!("FAILED");
                    report.steps[*step_index].status = "failed".into();
                    report.steps[*step_index].rollback.status = "not_available".into();
                    if let InstallError::CoreRequirementFailed {
                        requirement,
                        running,
                        verdict,
                        declaration_valid,
                        ..
                    } = &error
                    {
                        report.verify.observed_core_compatibility = Some(serde_json::json!({
                            "requirement": requirement,
                            "running": running,
                            "status": verdict,
                            "declaration_valid": declaration_valid,
                        }));
                    }
                    let (failure_stage, failure_exit, failure_code) =
                        action_failure(action, &error);

                    let mut rollback_attempted = false;
                    let mut rollback_failures = 0;
                    for (completed_index, rollback) in completed.iter().rev() {
                        let Some(rollback) = rollback else {
                            continue;
                        };
                        rollback_attempted = true;
                        let rollback_report = &mut report.steps[*completed_index].rollback;
                        rollback_report.attempted = true;
                        if rollback_step(rollback).is_ok() {
                            rollback_report.status = "ok".into();
                        } else {
                            rollback_report.status = "failed".into();
                            rollback_failures += 1;
                        }
                    }
                    report.rollback = InstallRollbackReport {
                        attempted: rollback_attempted,
                        status: if rollback_failures > 0 {
                            "failed".into()
                        } else if rollback_attempted {
                            "ok".into()
                        } else {
                            "not_attempted".into()
                        },
                        failure_count: rollback_failures,
                    };

                    if rollback_failures > 0 {
                        return failed_report(
                            report,
                            "rollback",
                            30,
                            "ROLLBACK_FAILED",
                            Some(failure_code),
                        );
                    }
                    return failed_report(report, failure_stage, failure_exit, failure_code, None);
                }
            }
        }

        let receipt_path = match commit(plan) {
            Ok(path) => path,
            Err(_error) => {
                let mut rollback_attempted = false;
                let mut rollback_failures = 0;
                for (completed_index, rollback) in completed.iter().rev() {
                    let Some(rollback) = rollback else {
                        continue;
                    };
                    rollback_attempted = true;
                    let rollback_report = &mut report.steps[*completed_index].rollback;
                    rollback_report.attempted = true;
                    if rollback_step(rollback).is_ok() {
                        rollback_report.status = "ok".into();
                    } else {
                        rollback_report.status = "failed".into();
                        rollback_failures += 1;
                    }
                }
                report.rollback = InstallRollbackReport {
                    attempted: rollback_attempted,
                    status: if rollback_failures > 0 {
                        "failed".into()
                    } else if rollback_attempted {
                        "ok".into()
                    } else {
                        "not_attempted".into()
                    },
                    failure_count: rollback_failures,
                };
                if rollback_failures > 0 {
                    return failed_report(
                        report,
                        "rollback",
                        30,
                        "ROLLBACK_FAILED",
                        Some("INSTALL_STEP_FAILED"),
                    );
                }
                return failed_report(report, "install", 30, "INSTALL_STEP_FAILED", None);
            }
        };

        if has_deferred_steps {
            eprintln!("Installation execution completed with deferred manual steps.");
            report.status = "partial".into();
        } else {
            eprintln!("Installation execution completed.");
            report.status = "ok".into();
        }
        report.stage = "complete".into();
        report.exit_code = 0;
        report.next_steps = success_next_steps(&report.dcc_type);
        report.receipt_path = receipt_path.map(|path| path.display().to_string());
        report.verify.failure_stage = Some("host-readiness".into());
        report.verify.failure_reason = Some("LIVE_DCC_VERIFICATION_REQUIRED".into());
        apply_readiness(report, readiness::probe_ready(&plan.dcc_type), false)
    }
}

fn is_receipt_action(action: &InstallStepAction) -> bool {
    matches!(
        action,
        InstallStepAction::PipInstall { .. }
            | InstallStepAction::GitClone { .. }
            | InstallStepAction::ZipExtract { .. }
            | InstallStepAction::PathCopy { .. }
            | InstallStepAction::AdobePluginLink { .. }
    )
}

/// Stop adapter-owned sidecars before replacing an installed pip package.
///
/// Runs once before the step loop so a blocked preflight leaves the system
/// untouched (there is nothing to roll back). Fresh installs and receipt
/// matches never enter this path.
fn prepare_sidecar_upgrade(plan: &InstallPlan) -> Result<(), InstallError> {
    let Some((action, package, python)) =
        plan.steps
            .iter()
            .find_map(|step| match step.action.as_ref()? {
                InstallStepAction::PipInstall {
                    package, python, ..
                } => Some((step.action.as_ref()?, package.as_str(), python.as_deref()?)),
                _ => None,
            })
    else {
        return Ok(());
    };
    // A receipt match means no pip mutation will happen; do not stop a sidecar
    // (or block) on a converged re-run.
    if receipt_match_details(plan, action).is_ok_and(|(matches, _)| matches) {
        return Ok(());
    }
    if discover_pip_artifact(python, package)?.is_none() {
        return Ok(());
    }
    let version = plan
        .steps
        .iter()
        .find_map(|step| match step.action.as_ref()? {
            InstallStepAction::PipInstall { version, .. } => version.as_deref(),
            _ => None,
        });
    match sidecar_preflight::prepare_sidecar_update(python, &plan.dcc_type, version) {
        sidecar_preflight::UpgradePreflight::RequiresRestart { reason, detail } => {
            Err(InstallError::UpgradeRequiresRestart { reason, detail })
        }
        sidecar_preflight::UpgradePreflight::ExecutionFailed { detail } => {
            Err(InstallError::StepFailed {
                step: "sidecar-preflight".into(),
                message: detail,
            })
        }
        _ => Ok(()),
    }
}

// ── step executors ───────────────────────────────────────────────────────────────

/// Execute a single install action, distinguishing completed work from manual deferral.
fn execute_action(action: &InstallStepAction) -> Result<StepExecution, InstallError> {
    match action {
        InstallStepAction::PipInstall {
            package,
            version,
            extras,
            python,
            artifact_url,
            sha256,
        } => execute_pip_install(
            package,
            version.as_deref(),
            extras.as_deref(),
            python.as_deref(),
            artifact_url.as_deref(),
            sha256.as_deref(),
        )
        .map(StepExecution::Completed),
        InstallStepAction::GitClone { url, ref_, dest } => {
            execute_git_clone(url, ref_.as_deref(), dest).map(StepExecution::Completed)
        }
        InstallStepAction::ZipExtract { url, sha256, dest } => {
            execute_zip_extract(url, sha256.as_deref(), dest).map(StepExecution::Completed)
        }
        InstallStepAction::PathCopy { source, dest } => {
            execute_path_copy(source, dest).map(StepExecution::Completed)
        }
        InstallStepAction::AdobePluginLink {
            source,
            dest,
            product,
            extension_type,
            ..
        } => adobe::execute_plugin_link(source, dest, product, extension_type)
            .map(StepExecution::Completed),
        InstallStepAction::RegisterDcc {
            dcc_type,
            entry_point,
            dcc_path,
            ..
        } => Ok(execute_register_dcc(
            dcc_type,
            entry_point.as_deref(),
            dcc_path.as_deref(),
        )),
        InstallStepAction::Verify => Err(InstallError::StepFailed {
            step: "verify".into(),
            message: "verify requires the full install plan".into(),
        }),
    }
}

fn execute_pip_install(
    package: &str,
    version: Option<&str>,
    extras: Option<&[String]>,
    python: Option<&str>,
    artifact_url: Option<&str>,
    sha256: Option<&str>,
) -> Result<Option<StepRollback>, InstallError> {
    let Some(python_cmd) = python.filter(|value| !value.trim().is_empty()) else {
        return Err(InstallError::StepFailed {
            step: format!("pip-install-{package}"),
            message:
                "no safe Python interpreter was selected; pass --python or DCC_MCP_INSTALL_PYTHON"
                    .into(),
        });
    };
    let package_spec = verified_pip_artifact_spec(package, version, extras, artifact_url, sha256)?;
    let previous_artifact = discover_pip_artifact(python_cmd, package)?;
    let mut cmd = Command::new(python_cmd);
    cmd.args(pip_install_args(&package_spec))
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let status = cmd.status().map_err(|e| InstallError::StepFailed {
        step: format!("pip-install-{package}"),
        message: format!("failed to launch {python_cmd}: {e}"),
    })?;

    if !status.success() {
        return Err(InstallError::StepFailed {
            step: format!("pip-install-{package}"),
            message: format!("{python_cmd} exited with {status}"),
        });
    }

    Ok(Some(pip_rollback(package, python_cmd, previous_artifact)))
}

fn pip_rollback(
    package: &str,
    python_cmd: &str,
    previous_artifact: Option<PipArtifact>,
) -> StepRollback {
    let Some(previous) = previous_artifact else {
        return StepRollback::Command {
            program: python_cmd.to_string(),
            args: pip_uninstall_args(package),
        };
    };
    let args = pip_restore_args_for_artifact(package, &previous);
    StepRollback::PipCommand {
        program: python_cmd.to_string(),
        args,
        artifact: Box::new(previous),
    }
}

fn discover_pip_artifact(
    python_cmd: &str,
    package: &str,
) -> Result<Option<PipArtifact>, InstallError> {
    let output = Command::new(python_cmd)
        .args(pip_show_args(package))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let Ok(output) = output else {
        return Err(pip_rollback_preflight_error(
            package,
            "failed to inspect the installed package",
        ));
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Package(s) not found") {
            return Ok(None);
        }
        return Err(pip_rollback_preflight_error(
            package,
            "pip show failed while inspecting the installed package",
        ));
    }

    let metadata = String::from_utf8_lossy(&output.stdout);
    let version = pip_show_version(&output.stdout).ok_or_else(|| {
        pip_rollback_preflight_error(
            package,
            "pip show did not report a usable installed version",
        )
    })?;
    let location = metadata
        .lines()
        .find_map(|line| line.strip_prefix("Location:").map(str::trim))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            pip_rollback_preflight_error(package, "pip show did not report a location")
        })?;
    let (metadata_path, direct_url) = find_direct_url_metadata(Path::new(location), package)
        .ok_or_else(|| {
            pip_rollback_preflight_error(
                package,
                "the installed package has no trusted direct URL metadata",
            )
        })?;
    let document: serde_json::Value = serde_json::from_str(&direct_url).map_err(|_| {
        pip_rollback_preflight_error(package, "the installed direct URL metadata is invalid")
    })?;
    let url = document
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.starts_with("https://"))
        .map(str::to_owned)
        .ok_or_else(|| {
            pip_rollback_preflight_error(package, "the prior artifact URL is not HTTPS")
        })?;
    let sha256 = document
        .get("archive_info")
        .and_then(|archive| archive.get("hashes"))
        .and_then(|hashes| hashes.get("sha256"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            document
                .get("archive_info")
                .and_then(|archive| archive.get("hash"))
                .and_then(serde_json::Value::as_str)
                .and_then(|value| value.strip_prefix("sha256="))
        })
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or_else(|| {
            pip_rollback_preflight_error(package, "the prior artifact has no SHA-256")
        })?;
    let normalized_package = package
        .chars()
        .map(|character| match character {
            '-' | '.' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect::<String>();
    let filename = url.rsplit('/').next().unwrap_or_default();
    let expected_prefix = format!("{normalized_package}-{version}-");
    if !filename
        .to_ascii_lowercase()
        .starts_with(&expected_prefix.to_ascii_lowercase())
        || !filename.to_ascii_lowercase().ends_with(".whl")
    {
        return Err(pip_rollback_preflight_error(
            package,
            "the prior artifact URL does not bind the installed package and version",
        ));
    }

    let artifact = PipArtifact {
        version,
        url,
        sha256,
        path: PathBuf::new(),
        metadata_path,
        metadata: direct_url,
    };
    Ok(Some(prepare_pip_artifact(python_cmd, package, artifact)?))
}

fn prepare_pip_artifact(
    python_cmd: &str,
    package: &str,
    artifact: PipArtifact,
) -> Result<PipArtifact, InstallError> {
    let staging = tempfile::tempdir().map_err(|_| {
        pip_rollback_preflight_error(package, "failed to create rollback staging directory")
    })?;
    let spec = format!("{package} @ {}#sha256={}", artifact.url, artifact.sha256);
    let status = Command::new(python_cmd)
        .args([
            "-m",
            "pip",
            "download",
            "--no-deps",
            "--only-binary=:all:",
            "--dest",
            staging.path().to_str().unwrap_or_default(),
            &spec,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| {
            pip_rollback_preflight_error(package, "failed to prepare the rollback wheel")
        })?;
    if !status.success() {
        return Err(pip_rollback_preflight_error(
            package,
            "the prior rollback wheel could not be downloaded",
        ));
    }
    let wheel = std::fs::read_dir(staging.path())
        .ok()
        .and_then(|entries| {
            entries.flatten().map(|entry| entry.path()).find(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("whl"))
            })
        })
        .ok_or_else(|| {
            pip_rollback_preflight_error(package, "pip did not produce a rollback wheel")
        })?;
    let bytes = std::fs::read(&wheel)
        .map_err(|_| pip_rollback_preflight_error(package, "failed to read the rollback wheel"))?;
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if digest != artifact.sha256 {
        return Err(pip_rollback_preflight_error(
            package,
            "the rollback wheel SHA-256 does not match its recorded digest",
        ));
    }
    let retained = staging.keep();
    let retained_wheel = retained.join(wheel.file_name().unwrap_or_default());
    let mut artifact = artifact;
    artifact.path = retained_wheel;
    Ok(artifact)
}

fn pip_rollback_preflight_error(package: &str, message: &str) -> InstallError {
    InstallError::StepFailed {
        step: format!("pip-rollback-{package}"),
        message: format!("cannot prepare a verified rollback: {message}"),
    }
}

fn find_direct_url_metadata(location: &Path, package: &str) -> Option<(PathBuf, String)> {
    let normalized_package = package
        .chars()
        .map(|character| match character {
            '-' | '.' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect::<String>();
    for entry in std::fs::read_dir(location).ok()?.flatten() {
        let file_type = entry.file_type().ok()?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !name.starts_with(&format!("{normalized_package}-")) || !name.ends_with(".dist-info") {
            continue;
        }
        let path = entry.path().join("direct_url.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            return Some((path, content));
        }
    }
    None
}

fn verified_pip_artifact_spec(
    package: &str,
    version: Option<&str>,
    extras: Option<&[String]>,
    artifact_url: Option<&str>,
    sha256: Option<&str>,
) -> Result<String, InstallError> {
    let version = version.unwrap_or_default().trim();
    let artifact_url = artifact_url.unwrap_or_default().trim();
    let value = sha256.unwrap_or_default().trim();
    let checksum = value.strip_prefix("sha256:").unwrap_or(value);
    let valid_package = !package.is_empty()
        && package
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && package
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && package
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    let valid_extras = extras.is_none_or(|values| {
        values.iter().all(|value| {
            !value.is_empty()
                && value
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && value
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    });
    let normalized_package = package
        .chars()
        .map(|character| match character {
            '-' | '.' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect::<String>();
    let filename = artifact_url.rsplit('/').next().unwrap_or_default();
    let expected_prefix = format!("{normalized_package}-{version}-");
    let valid_artifact = artifact_url.starts_with("https://")
        && !artifact_url.contains('#')
        && !artifact_url.contains('?')
        && filename
            .to_ascii_lowercase()
            .starts_with(&expected_prefix.to_ascii_lowercase())
        && filename.ends_with("-py3-none-any.whl");
    if !valid_package
        || !valid_extras
        || version.is_empty()
        || !valid_artifact
        || checksum.len() != 64
        || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(InstallError::StepFailed {
            step: "pip-integrity".into(),
            message: "catalog-pinned wheel URL, version, and SHA-256 are required".into(),
        });
    }

    let package_with_extras = if extras.is_some_and(|values| !values.is_empty()) {
        format!("{}[{}]", package, extras.unwrap().join(","))
    } else {
        package.to_string()
    };
    Ok(format!(
        "{package_with_extras} @ {artifact_url}#sha256={}",
        checksum.to_ascii_lowercase()
    ))
}

pub(super) fn resolve_install_source_path(source: &Path) -> PathBuf {
    if source.exists() {
        return std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    }
    if source.is_absolute() {
        source.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(source))
            .unwrap_or_else(|_| source.to_path_buf())
    }
}

fn execute_register_dcc(
    _dcc_type: &str,
    _entry_point: Option<&str>,
    _dcc_path: Option<&Path>,
) -> StepExecution {
    // Registration is owned by the DCC plugin's sidecar. The CLI can install
    // packages, but it must not pretend a host process is registered until the
    // plugin starts, stays alive, and advertises itself in the registry.
    StepExecution::Deferred
}

fn execute_verify(plan: &InstallPlan) -> Result<Option<StepRollback>, InstallError> {
    if let Some(path) = &plan.dcc_path
        && !path.exists()
    {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!("DCC path does not exist: {}", path.display()),
        });
    }
    let mut pip_target = None;
    for step in &plan.steps {
        let Some(action) = &step.action else {
            continue;
        };
        match action {
            InstallStepAction::GitClone { dest, .. }
            | InstallStepAction::ZipExtract { dest, .. }
            | InstallStepAction::PathCopy { dest, .. } => verify_installed_path(dest)?,
            InstallStepAction::PipInstall {
                package,
                version,
                python,
                ..
            } => {
                verify_pip_package(package, version.as_deref(), python.as_deref())?;
                if target_core_requirement(plan).is_none() {
                    verify_target_adapter_import(plan, package, python.as_deref())?;
                }
                pip_target = Some((package.clone(), python.clone()));
            }
            InstallStepAction::AdobePluginLink { dest, manifest, .. } => {
                adobe::verify_linked_plugin(dest, manifest.as_deref())?
            }
            InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => {}
        }
    }
    if let Some(requirement) = target_core_requirement(plan) {
        let Some((package, python)) = pip_target else {
            return Err(InstallError::StepFailed {
                step: "verify".into(),
                message: format!(
                    "cannot verify Core requirement '{requirement}' without a target Python package"
                ),
            });
        };
        verify_target_core_requirement(plan, &requirement, &package, python.as_deref())?;
    }
    Ok(None)
}

pub(super) fn target_core_requirement(plan: &InstallPlan) -> Option<String> {
    plan.adapter.core_requirement.clone().or_else(|| {
        plan.adapter
            .min_core_version
            .as_deref()
            .and_then(derive_requirement)
    })
}

fn verify_target_adapter_import(
    plan: &InstallPlan,
    package: &str,
    python: Option<&str>,
) -> Result<(), InstallError> {
    let Some(python) = python.filter(|value| !value.trim().is_empty()) else {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!(
                "no target Python interpreter is available to verify adapter import '{package}'"
            ),
        });
    };
    let target = AdapterImportTarget {
        dcc_type: plan.dcc_type.clone(),
        module: import_module(&plan.adapter, package),
        distribution: package.to_string(),
        python: Some(PathBuf::from(python)),
        python_source: Some("install_plan".into()),
    };
    let probe = probe_target(&target);
    if probe
        .get("imported")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        && probe.get("status").and_then(serde_json::Value::as_str) == Some("ok")
    {
        return Ok(());
    }
    let status = probe
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unavailable");
    Err(InstallError::StepFailed {
        step: "verify".into(),
        message: format!("target adapter import is not healthy for '{package}' ({status})"),
    })
}

fn verify_target_core_requirement(
    plan: &InstallPlan,
    requirement: &str,
    package: &str,
    python: Option<&str>,
) -> Result<(), InstallError> {
    let Some(python) = python.filter(|value| !value.trim().is_empty()) else {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!(
                "no target Python interpreter is available to verify Core requirement '{requirement}'"
            ),
        });
    };
    let target = AdapterImportTarget {
        dcc_type: plan.dcc_type.clone(),
        module: import_module(&plan.adapter, package),
        distribution: package.to_string(),
        python: Some(PathBuf::from(python)),
        python_source: Some("install_plan".into()),
    };
    let probe = probe_target(&target);
    if !probe
        .get("imported")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        let status = probe
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unavailable");
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!(
                "target adapter import failed while verifying Core requirement '{requirement}' ({status})"
            ),
        });
    }
    let report = probe
        .as_object()
        .expect("adapter import probe is a JSON object");
    let probe_status = report.get("status").and_then(serde_json::Value::as_str);
    if probe_status == Some("incompatible_core") {
        let declared_requirement = report
            .get("core_requirement")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(requirement);
        let core = core_compatibility_for_requirement(report, declared_requirement);
        return Err(core_requirement_failure(&core));
    }
    if probe_status != Some("ok") {
        let status = report
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unavailable");
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!(
                "target adapter/Core probe is not healthy while verifying Core requirement '{requirement}' ({status})"
            ),
        });
    }
    let core = core_compatibility_for_requirement(report, requirement);
    if !core.is_ok() {
        return Err(core_requirement_failure(&core));
    }
    Ok(())
}

fn core_requirement_failure(core: &CoreRuntimeReport) -> InstallError {
    InstallError::CoreRequirementFailed {
        requirement: core.declaration.clone(),
        running: core.running.as_deref().unwrap_or("unknown").into(),
        verdict: core.verdict.as_str().into(),
        declaration_valid: core.bound.is_ok(),
        reason: core.bound.message(),
    }
}

fn verify_installed_path(path: &Path) -> Result<(), InstallError> {
    if !path.exists() {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!("installed path does not exist: {}", path.display()),
        });
    }
    if path.is_dir() && fs::read_dir(path)?.next().is_none() {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!("installed directory is empty: {}", path.display()),
        });
    }
    Ok(())
}

fn verify_pip_package(
    package: &str,
    expected_version: Option<&str>,
    python: Option<&str>,
) -> Result<(), InstallError> {
    let Some(python_cmd) = python.filter(|value| !value.trim().is_empty()) else {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message:
                "no safe Python interpreter was selected; pass --python or DCC_MCP_INSTALL_PYTHON"
                    .into(),
        });
    };
    let output = Command::new(python_cmd)
        .args(pip_show_args(package))
        .output()
        .map_err(|e| InstallError::StepFailed {
            step: "verify".into(),
            message: format!("failed to launch {python_cmd}: {e}"),
        })?;
    if !output.status.success() {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!("{python_cmd} could not verify installed pip package '{package}'"),
        });
    }
    if let Some(expected) = expected_version {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let actual = stdout
            .lines()
            .find_map(|line| line.strip_prefix("Version:"))
            .map(str::trim)
            .unwrap_or_default();
        if actual != expected {
            return Err(InstallError::StepFailed {
                step: "verify".into(),
                message: format!(
                    "installed {package} version mismatch: expected {expected}, got {actual}"
                ),
            });
        }
    }
    Ok(())
}

// ── rollback ─────────────────────────────────────────────────────────────────────

fn execute_rollback(rollback: &StepRollback) -> Result<(), InstallError> {
    match rollback {
        StepRollback::RemovePath(path) => {
            if path.exists() {
                if path.is_dir() {
                    std::fs::remove_dir_all(path)?;
                } else {
                    std::fs::remove_file(path)?;
                }
            }
            Ok(())
        }
        StepRollback::Command { program, args } => {
            let status = Command::new(program)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| InstallError::RollbackFailed {
                    step: program.clone(),
                    message: format!("failed to launch {program}: {e}"),
                })?;
            if !status.success() {
                return Err(InstallError::RollbackFailed {
                    step: program.clone(),
                    message: format!("{program} exited with {status}"),
                });
            }
            Ok(())
        }
        StepRollback::PipCommand {
            program,
            args,
            artifact,
        } => {
            let status = Command::new(program)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| InstallError::RollbackFailed {
                    step: program.clone(),
                    message: format!("failed to launch {program}: {e}"),
                })?;
            if !status.success() {
                return Err(InstallError::RollbackFailed {
                    step: program.clone(),
                    message: format!("{program} exited with {status}"),
                });
            }
            std::fs::write(&artifact.metadata_path, &artifact.metadata).map_err(|e| {
                InstallError::RollbackFailed {
                    step: program.clone(),
                    message: format!("failed to restore direct URL metadata: {e}"),
                }
            })?;
            std::fs::remove_file(&artifact.path).map_err(|e| InstallError::RollbackFailed {
                step: program.clone(),
                message: format!("failed to remove retained rollback wheel: {e}"),
            })?;
            Ok(())
        }
    }
}

// ── tests ────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "install/recovery_tests.rs"]
mod recovery_tests;

#[cfg(test)]
mod tests;
