use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::domain::install_catalog::CatalogSource;
use dcc_mcp_catalog::CatalogEntry;
use thiserror::Error;

use crate::application::adapter_import::{
    AdapterImportTarget, core_compatibility_for_requirement, import_module, probe_target,
};
use crate::domain::install::{
    InstallPlan, InstallPlanError, InstallPlanner, InstallPolicy, InstallRequest,
    InstallStepAction, normalized_dcc_key,
};

const BUNDLED_CATALOG: &str = include_str!("../../../../dcc-mcp-catalog.yml");

mod adobe;
#[cfg(test)]
mod archive_http_tests;
mod catalog;
#[cfg(test)]
mod catalog_contract_tests;
mod discovery;
mod pip;
mod policy;
mod report;
mod verify;

pub use discovery::{DccAdapterSummary, DccTypeSummary, DccTypesCatalog};
use pip::{pip_install_args, pip_show_args, pip_uninstall_args};
use policy::{AutoInstallPolicy, ask_consent, render_install_policy_prompt};
pub use report::{
    InstallExecutionError, InstallExecutionReport, InstallReportNextStep, InstallRollbackReport,
    InstallStepReport, InstallStepRollbackReport, InstallVerifyReport,
};
use report::{
    action_failure, execution_report_for_plan, failed_report, safe_report_identifier,
    stable_step_id, success_next_steps,
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
        self.execute_plan_with(
            plan,
            skip_confirmation,
            |action, plan| match action {
                InstallStepAction::Verify => execute_verify(plan).map(StepExecution::Completed),
                _ => execute_action(action),
            },
            execute_rollback,
        )
    }

    fn execute_plan_with<E, R>(
        &self,
        plan: &InstallPlan,
        skip_confirmation: bool,
        mut execute_step: E,
        mut rollback_step: R,
    ) -> InstallExecutionReport
    where
        E: FnMut(&InstallStepAction, &InstallPlan) -> Result<StepExecution, InstallError>,
        R: FnMut(&StepRollback) -> Result<(), InstallError>,
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
        report.verify.failure_stage = Some("host-readiness".into());
        report.verify.failure_reason = Some("LIVE_DCC_VERIFICATION_REQUIRED".into());
        report
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

    // Rollback: pip uninstall
    Ok(Some(StepRollback::Command {
        program: python_cmd.to_string(),
        args: pip_uninstall_args(package),
    }))
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

fn execute_git_clone(
    url: &str,
    ref_: Option<&str>,
    dest: &Path,
) -> Result<Option<StepRollback>, InstallError> {
    let commit = required_git_commit(ref_)?;
    if dest.exists() {
        return Err(InstallError::StepFailed {
            step: "git-clone".into(),
            message: format!("destination already exists: {}", dest.display()),
        });
    }

    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    run_git(
        dest,
        "init",
        &["init", "--quiet", dest.to_string_lossy().as_ref()],
    )?;
    run_git(dest, "remote-add", &["remote", "add", "origin", url])?;
    run_git(dest, "fetch", &["fetch", "--depth", "1", "origin", &commit])?;
    run_git(
        dest,
        "checkout",
        &["checkout", "--detach", "--quiet", "FETCH_HEAD"],
    )?;

    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dest)
        .output()
        .map_err(|error| InstallError::StepFailed {
            step: "git-verify".into(),
            message: format!("failed to launch git: {error}"),
        })?;
    if !output.status.success() {
        return Err(InstallError::StepFailed {
            step: "git-verify".into(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    let actual = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    if actual != commit {
        return Err(InstallError::StepFailed {
            step: "git-verify".into(),
            message: format!("commit mismatch: expected {commit}, got {actual}"),
        });
    }

    Ok(Some(StepRollback::RemovePath(dest.to_path_buf())))
}

fn required_git_commit(ref_: Option<&str>) -> Result<String, InstallError> {
    let reference = ref_.unwrap_or_default().trim();
    if reference.len() != 40 || !reference.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InstallError::StepFailed {
            step: "git-integrity".into(),
            message: "a full 40-character commit object ID is required".into(),
        });
    }
    Ok(reference.to_ascii_lowercase())
}

fn run_git(dest: &Path, step: &str, args: &[&str]) -> Result<(), InstallError> {
    let mut command = Command::new("git");
    command.args(args);
    if step != "init" {
        command.current_dir(dest);
    }
    let output = command.output().map_err(|error| InstallError::StepFailed {
        step: format!("git-{step}"),
        message: format!("failed to launch git: {error}"),
    })?;
    if !output.status.success() {
        return Err(InstallError::StepFailed {
            step: format!("git-{step}"),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

fn execute_zip_extract(
    url: &str,
    sha256: Option<&str>,
    dest: &Path,
) -> Result<Option<StepRollback>, InstallError> {
    let expected = required_archive_sha256(sha256)?;
    if dest.exists() {
        return Err(InstallError::StepFailed {
            step: "zip-extract".into(),
            message: format!("destination already exists: {}", dest.display()),
        });
    }

    // Download the archive
    let response = reqwest::blocking::get(url).map_err(|e| InstallError::StepFailed {
        step: "zip-download".into(),
        message: format!("failed to download {url}: {e}"),
    })?;

    if !response.status().is_success() {
        return Err(InstallError::StepFailed {
            step: "zip-download".into(),
            message: format!("archive download returned HTTP {}", response.status()),
        });
    }

    let bytes = response.bytes().map_err(|e| InstallError::StepFailed {
        step: "zip-download".into(),
        message: format!("failed to read response from {url}: {e}"),
    })?;

    use sha2::Digest;
    let actual = sha2::Sha256::digest(&bytes);
    let actual_hex = actual
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual_hex.eq_ignore_ascii_case(&expected) {
        return Err(InstallError::StepFailed {
            step: "zip-checksum".into(),
            message: format!("SHA-256 mismatch: expected {expected}, got {actual_hex}"),
        });
    }

    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Extract the archive
    let reader = std::io::Cursor::new(&bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| InstallError::StepFailed {
        step: "zip-extract".into(),
        message: format!("failed to open zip archive: {e}"),
    })?;

    archive
        .extract(dest)
        .map_err(|e| InstallError::StepFailed {
            step: "zip-extract".into(),
            message: format!("failed to extract to {}: {e}", dest.display()),
        })?;

    Ok(Some(StepRollback::RemovePath(dest.to_path_buf())))
}

fn required_archive_sha256(sha256: Option<&str>) -> Result<String, InstallError> {
    let value = sha256.unwrap_or_default().trim();
    let checksum = value.strip_prefix("sha256:").unwrap_or(value);
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InstallError::StepFailed {
            step: "zip-integrity".into(),
            message: "exactly 64 hexadecimal SHA-256 digits are required".into(),
        });
    }
    Ok(checksum.to_ascii_lowercase())
}

fn execute_path_copy(source: &Path, dest: &Path) -> Result<Option<StepRollback>, InstallError> {
    if dest.exists() {
        return Err(InstallError::StepFailed {
            step: "path-copy".into(),
            message: format!("destination already exists: {}", dest.display()),
        });
    }

    if !source.exists() {
        return Err(InstallError::StepFailed {
            step: "path-copy".into(),
            message: format!("source does not exist: {}", source.display()),
        });
    }

    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if source.is_dir() {
        copy_dir_recursive(source, dest)?;
    } else {
        std::fs::copy(source, dest)?;
    }

    Ok(Some(StepRollback::RemovePath(dest.to_path_buf())))
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
                pip_target = Some((package.clone(), python.clone()));
            }
            InstallStepAction::AdobePluginLink { dest, manifest, .. } => {
                adobe::verify_linked_plugin(dest, manifest.as_deref())?
            }
            InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => {}
        }
    }
    if let Some(requirement) = plan.adapter.core_requirement.as_deref() {
        let Some((package, python)) = pip_target else {
            return Err(InstallError::StepFailed {
                step: "verify".into(),
                message: format!(
                    "cannot verify Core requirement '{requirement}' without a target Python package"
                ),
            });
        };
        verify_target_core_requirement(plan, requirement, &package, python.as_deref())?;
    }
    Ok(None)
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
    let core = core_compatibility_for_requirement(report, requirement);
    if !core.is_ok() {
        return Err(InstallError::CoreRequirementFailed {
            requirement: requirement.into(),
            running: core.running.as_deref().unwrap_or("unknown").into(),
            verdict: core.verdict.as_str().into(),
            declaration_valid: core.bound.is_ok(),
            reason: core.bound.message(),
        });
    }
    Ok(())
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
    }
}

/// Recursively copy a directory and its contents.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), InstallError> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ── tests ────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "install/recovery_tests.rs"]
mod recovery_tests;

#[cfg(test)]
mod tests;
