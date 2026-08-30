use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use dcc_mcp_models::DccName;
use serde::Serialize;
use thiserror::Error;

use crate::domain::install::{
    InstallPlan, InstallPlanError, InstallPlanner, InstallPolicy, InstallRequest,
    InstallStepAction, normalized_dcc_key,
};

const BUNDLED_CATALOG: &str = include_str!("../../../../dcc-mcp-catalog.yml");

mod pip;
mod policy;
mod report;

use pip::{pip_install_args, pip_show_args, pip_uninstall_args};
use policy::{AutoInstallPolicy, ask_consent, render_install_policy_prompt};
pub use report::{
    InstallExecutionError, InstallExecutionReport, InstallReportNextStep, InstallRollbackReport,
    InstallStepReport, InstallStepRollbackReport, InstallVerifyReport,
};
use report::{
    action_failure, empty_execution_report, execution_report_for_plan, failed_report,
    safe_report_identifier, stable_step_id, success_next_steps,
};

#[derive(Debug, Error)]
pub enum InstallError {
    #[error(transparent)]
    Catalog(#[from] dcc_mcp_catalog::CatalogError),
    #[error(transparent)]
    Plan(#[from] InstallPlanError),
    #[error("consent denied by user")]
    ConsentDenied,
    #[error("step '{step}' failed: {message}")]
    StepFailed { step: String, message: String },
    #[error("rollback of step '{step}' failed: {message}")]
    RollbackFailed { step: String, message: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DccTypesCatalog {
    pub total: usize,
    pub dcc_types: Vec<DccTypeSummary>,
    pub custom_types_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DccTypeSummary {
    pub dcc_type: String,
    pub adapters: Vec<DccAdapterSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DccAdapterSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub catalog_install_available: bool,
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
    default_catalog_path: PathBuf,
    auto_install_policy: AutoInstallPolicy,
}

impl InstallService {
    #[must_use]
    pub fn new(default_catalog_path: PathBuf) -> Self {
        Self {
            default_catalog_path,
            auto_install_policy: AutoInstallPolicy::from_env(),
        }
    }

    #[cfg(test)]
    fn with_auto_install_policy(
        default_catalog_path: PathBuf,
        auto_install_policy: AutoInstallPolicy,
    ) -> Self {
        Self {
            default_catalog_path,
            auto_install_policy,
        }
    }

    /// Generate an install plan (display-only, no execution).
    pub fn plan(&self, request: InstallRequest) -> Result<InstallPlan, InstallError> {
        let entries = self.load_entries(request.catalog_path.as_deref())?;
        let plan = InstallPlanner::plan(&entries, request)?;
        Ok(self.apply_auto_install_policy(plan))
    }

    /// List adapter-backed DCC types from the same catalog used by `install`.
    pub fn dcc_types(&self, catalog_path: Option<&Path>) -> Result<DccTypesCatalog, InstallError> {
        let entries = self.load_entries(catalog_path)?;
        let mut grouped: BTreeMap<String, BTreeMap<String, DccAdapterSummary>> = BTreeMap::new();
        let mut canonical_by_normalized: BTreeMap<String, String> = BTreeMap::new();

        for entry in entries.iter().filter(|entry| {
            entry
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case("adapter"))
        }) {
            let adapter = DccAdapterSummary {
                name: entry.name.clone(),
                version: entry.version.clone(),
                url: entry.url.clone(),
                catalog_install_available: entry.install.is_some(),
            };
            for dcc_type in &entry.dcc {
                let parsed = DccName::parse(dcc_type).to_string();
                let normalized = normalized_dcc_key(&parsed);
                if normalized.is_empty() {
                    continue;
                }
                let canonical = canonical_by_normalized
                    .entry(normalized)
                    .or_insert_with(|| parsed.clone())
                    .clone();
                grouped
                    .entry(canonical)
                    .or_default()
                    .insert(adapter.name.clone(), adapter.clone());
            }
        }

        let dcc_types = grouped
            .into_iter()
            .map(|(dcc_type, adapters)| DccTypeSummary {
                dcc_type,
                adapters: adapters.into_values().collect(),
            })
            .collect::<Vec<_>>();

        Ok(DccTypesCatalog {
            total: dcc_types.len(),
            dcc_types,
            custom_types_supported: true,
        })
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
            Err(_) => failed_report(
                empty_execution_report(requested_dcc),
                "preflight",
                10,
                "INSTALL_PLAN_FAILED",
                None,
            ),
        }
    }

    fn load_entries(
        &self,
        requested_path: Option<&Path>,
    ) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, InstallError> {
        if let Some(path) = requested_path {
            return dcc_mcp_catalog::load_from_file(path).map_err(Into::into);
        }

        let entries = dcc_mcp_catalog::load_from_file(Path::new(&self.default_catalog_path))?;
        if entries.is_empty() {
            return dcc_mcp_catalog::load_from_str(BUNDLED_CATALOG).map_err(Into::into);
        }
        Ok(entries)
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
                Err(_) => {
                    eprintln!("FAILED");
                    report.steps[*step_index].status = "failed".into();
                    report.steps[*step_index].rollback.status = "not_available".into();
                    let (failure_stage, failure_exit, failure_code) = action_failure(action);

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
    let python_cmd = python.unwrap_or("python");
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
            } => verify_pip_package(package, version.as_deref(), python.as_deref())?,
            InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => {}
        }
    }
    Ok(None)
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
    let python_cmd = python.unwrap_or("python");
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
mod tests {
    use super::*;
    use crate::domain::install::InstallStep;

    #[test]
    fn service_uses_bundled_catalog_when_default_path_is_missing() {
        let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
        let plan = service
            .plan(InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
                python: None,
                dcc_path: None,
            })
            .unwrap();

        assert_eq!(plan.adapter.name, "dcc-mcp-maya");
        assert!(matches!(
            plan.steps[0].action,
            Some(InstallStepAction::PipInstall { .. })
        ));
    }

    #[test]
    fn bundled_catalog_prefers_an_adapter_over_its_skill_pack() {
        let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
        let plan = service
            .plan(InstallRequest {
                dcc_type: "unreal".into(),
                version: None,
                catalog_path: None,
                python: None,
                dcc_path: None,
            })
            .unwrap();

        assert_eq!(plan.adapter.name, "dcc-mcp-unreal");
        assert!(matches!(
            plan.steps[0].action,
            Some(InstallStepAction::PipInstall { .. })
        ));
    }

    #[test]
    fn bundled_catalog_lists_adapter_dcc_types_and_collapses_aliases() {
        let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
        let catalog = service.dcc_types(None).unwrap();

        assert!(catalog.custom_types_supported);
        assert_eq!(catalog.dcc_types.len(), 36);
        for alias in ["after effects", "after-effects", "comfy-ui"] {
            assert!(
                catalog
                    .dcc_types
                    .iter()
                    .all(|entry| entry.dcc_type != alias),
                "alias {alias} must collapse into its first canonical catalog spelling"
            );
        }
        assert_eq!(
            catalog
                .dcc_types
                .iter()
                .filter(|entry| entry.dcc_type == "3dsmax")
                .count(),
            1
        );
        for (expected_type, expected_adapter) in [
            ("c4d", "dcc-mcp-cinema4d"),
            ("comfyui", "dcc-mcp-comfyui"),
            ("freecad", "dcc-mcp-freecad"),
            ("gimp", "dcc-mcp-gimp"),
            ("godot", "dcc-mcp-godot"),
            ("krita", "dcc-mcp-krita"),
            ("mari", "dcc-mcp-mari"),
            ("material-maker", "dcc-mcp-material-maker"),
            ("obs", "dcc-mcp-obs"),
            ("openscad", "dcc-mcp-openscad"),
            ("powerpoint", "dcc-mcp-PowerPoint"),
            ("renderdoc", "dcc-mcp-renderdoc"),
            ("sketchup", "dcc-mcp-sketchup"),
            ("shogun", "dcc-mcp-shogun"),
            ("tiled", "dcc-mcp-tiled"),
            ("touchdesigner", "dcc-mcp-touchdesigner"),
            ("unity", "dcc-mcp-unity"),
            ("wwise", "dcc-mcp-wwise"),
        ] {
            let entry = catalog
                .dcc_types
                .iter()
                .find(|entry| entry.dcc_type == expected_type)
                .unwrap_or_else(|| panic!("missing cataloged DCC type {expected_type}"));
            assert!(
                entry
                    .adapters
                    .iter()
                    .any(|adapter| adapter.name == expected_adapter)
            );
        }
    }

    #[test]
    fn install_plan_reports_disabled_auto_install_policy_with_custom_prompt() {
        let service = InstallService::with_auto_install_policy(
            PathBuf::from("__missing_dcc_mcp_catalog__.yml"),
            AutoInstallPolicy::disabled(
                "Auto install unavailable; contact PipelineTD to deploy {adapter} for {dcc_type}.",
            ),
        );
        let plan = service
            .plan(InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
                python: None,
                dcc_path: None,
            })
            .unwrap();

        assert!(!plan.install_policy.auto_install_enabled);
        assert_eq!(
            plan.install_policy.prompt.as_deref(),
            Some("Auto install unavailable; contact PipelineTD to deploy dcc-mcp-maya for maya.")
        );
    }

    #[test]
    fn execute_reports_preflight_failure_when_auto_install_is_disabled() {
        let service = InstallService::with_auto_install_policy(
            PathBuf::from("__missing_dcc_mcp_catalog__.yml"),
            AutoInstallPolicy::disabled(
                "Automatic install disabled; ask PipelineTD for {adapter}.",
            ),
        );

        let report = service.execute(
            InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
                python: Some("/__nonexistent__/python".into()),
                dcc_path: None,
            },
            true,
        );

        assert_eq!(report.status, "failed");
        assert_eq!(report.stage, "preflight");
        assert_eq!(report.exit_code, 10);
        assert_eq!(report.error.as_ref().unwrap().code, "AUTO_INSTALL_DISABLED");
        assert!(report.steps.iter().all(|step| step.status == "not_run"));
        assert!(!report.verify.directly_usable);
        assert_eq!(report.verify.failure_stage.as_deref(), Some("preflight"));
        assert_eq!(
            report.verify.failure_reason.as_deref(),
            Some("AUTO_INSTALL_DISABLED")
        );
    }

    #[test]
    fn pip_install_missing_python_reports_error() {
        let result = execute_pip_install(
            "nonexistent-package",
            Some("1.0.0"),
            None,
            Some("/__nonexistent__/python"),
            Some(
                "https://files.pythonhosted.org/packages/example/nonexistent_package-1.0.0-py3-none-any.whl",
            ),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(&err, InstallError::StepFailed { step, .. } if step.contains("pip-install")),
            "expected StepFailed, got {err}"
        );
    }

    #[test]
    fn git_clone_nonexistent_url_fails() {
        let dest = PathBuf::from("/__nonexistent__/test-repo");
        let result = execute_git_clone("https://__nonexistent__.invalid/repo.git", None, &dest);
        assert!(result.is_err());
    }

    #[test]
    fn git_and_zip_integrity_fail_before_network_or_filesystem_io() {
        let root = tempfile::tempdir().unwrap();
        let git_dest = root.path().join("git-install");
        let git_error = execute_git_clone(
            "https://__nonexistent__.invalid/repo.git",
            Some("main"),
            &git_dest,
        )
        .unwrap_err();
        assert!(git_error.to_string().contains("40-character commit"));
        assert!(!git_dest.exists());

        let zip_dest = root.path().join("zip-install");
        let zip_error = execute_zip_extract(
            "https://__nonexistent__.invalid/archive.zip",
            None,
            &zip_dest,
        )
        .unwrap_err();
        assert!(zip_error.to_string().contains("64 hexadecimal"));
        assert!(!zip_dest.exists());

        assert_eq!(
            required_archive_sha256(Some(&format!("sha256:{}", "A".repeat(64)))).unwrap(),
            "a".repeat(64)
        );
    }

    #[test]
    fn path_copy_missing_source_fails() {
        let result = execute_path_copy(
            &PathBuf::from("/__nonexistent__/source"),
            &PathBuf::from("/__nonexistent__/dest"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn rollback_remove_path_does_not_error_on_nonexistent() {
        let rb = StepRollback::RemovePath(PathBuf::from("/__nonexistent__/path"));
        assert!(execute_rollback(&rb).is_ok());
    }

    #[test]
    fn register_dcc_is_explicitly_deferred() {
        let result = execute_register_dcc("maya", Some("dcc_mcp_maya.cli:main"), None);
        assert!(matches!(result, StepExecution::Deferred));
    }

    #[test]
    fn pip_install_uses_verified_direct_artifact_reference() {
        let spec = verified_pip_artifact_spec(
            "dcc-mcp-maya",
            Some("0.9.22"),
            Some(&["maya".to_string()]),
            Some(
                "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-0.9.22-py3-none-any.whl",
            ),
            Some("A".repeat(64).as_str()),
        )
        .unwrap();
        assert_eq!(
            pip_install_args(&spec),
            vec![
                "-m".to_string(),
                "pip".to_string(),
                "install".to_string(),
                "--upgrade".to_string(),
                format!(
                    "dcc-mcp-maya[maya] @ https://files.pythonhosted.org/packages/example/dcc_mcp_maya-0.9.22-py3-none-any.whl#sha256={}",
                    "a".repeat(64)
                ),
            ]
        );

        let error = verified_pip_artifact_spec(
            "dcc-mcp-unity",
            Some("0.11.2"),
            None,
            Some("https://pypi.org/project/dcc-mcp-unity"),
            None,
        );
        assert!(error.unwrap_err().to_string().contains("pip-integrity"));
        assert_eq!(
            pip_uninstall_args("dcc-mcp-maya"),
            vec![
                "-m".to_string(),
                "pip".to_string(),
                "uninstall".to_string(),
                "-y".to_string(),
                "dcc-mcp-maya".to_string(),
            ]
        );
        assert_eq!(
            pip_show_args("dcc-mcp-maya"),
            vec![
                "-m".to_string(),
                "pip".to_string(),
                "show".to_string(),
                "dcc-mcp-maya".to_string(),
            ]
        );
    }

    fn verify_plan(action: InstallStepAction) -> InstallPlan {
        InstallPlan {
            dcc_type: "maya".into(),
            version: None,
            dcc_path: None,
            adapter: dcc_mcp_catalog::CatalogEntry {
                name: "dcc-mcp-maya".into(),
                description: "Maya adapter".into(),
                dcc: vec!["maya".into()],
                targets: vec![],
                url: None,
                issues_url: None,
                tags: vec![],
                version: None,
                min_core_version: None,
                install: None,
                package: None,
                maintainer: None,
                category: None,
                policy: None,
                requires: None,
                icon: None,
                showcase: None,
            },
            steps: vec![
                InstallStep {
                    name: "install".into(),
                    description: "Install adapter".into(),
                    action: Some(action),
                },
                InstallStep {
                    name: "verify".into(),
                    description: "Verify adapter".into(),
                    action: Some(InstallStepAction::Verify),
                },
            ],
            next_steps: vec![],
            install_policy: InstallPolicy::enabled(),
        }
    }

    #[test]
    fn verify_accepts_non_empty_installed_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source");
        let dest = tmp.path().join("dest");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("adapter.txt"), "installed").unwrap();

        let result = execute_verify(&verify_plan(InstallStepAction::PathCopy { source, dest }));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn verify_rejects_missing_installed_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source");
        let dest = tmp.path().join("missing");

        let err = execute_verify(&verify_plan(InstallStepAction::PathCopy {
            source,
            dest: dest.clone(),
        }))
        .unwrap_err();
        assert!(
            matches!(&err, InstallError::StepFailed { step, message }
                if step == "verify" && message.contains(&dest.display().to_string())),
            "expected verify StepFailed, got {err}"
        );
    }

    #[test]
    fn verify_rejects_empty_installed_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source");
        let dest = tmp.path().join("empty");
        fs::create_dir_all(&dest).unwrap();

        let err = execute_verify(&verify_plan(InstallStepAction::PathCopy {
            source,
            dest: dest.clone(),
        }))
        .unwrap_err();
        assert!(
            matches!(&err, InstallError::StepFailed { step, message }
                if step == "verify" && message.contains("installed directory is empty")),
            "expected verify StepFailed, got {err}"
        );
    }

    #[test]
    fn execution_report_success_keeps_live_dcc_readiness_unproven() {
        let service = InstallService::new(PathBuf::from("__unused__.yml"));
        let plan = verify_plan(InstallStepAction::PathCopy {
            source: PathBuf::from("source"),
            dest: PathBuf::from("dest"),
        });

        let report = service.execute_plan_with(
            &plan,
            true,
            |_action, _plan| Ok(StepExecution::Completed(None)),
            |_rollback| Ok(()),
        );

        assert_eq!(report.status, "ok");
        assert_eq!(report.stage, "complete");
        assert_eq!(report.exit_code, 0);
        assert!(report.error.is_none());
        assert_eq!(report.steps[0].id, "install-path");
        assert_eq!(report.steps[0].status, "ok");
        assert_eq!(report.steps[0].rollback.status, "not_available");
        assert_eq!(report.steps[1].id, "verify");
        assert_eq!(report.steps[1].status, "ok");
        assert!(!report.verify.directly_usable);
        assert_eq!(
            report.verify.failure_stage.as_deref(),
            Some("host-readiness")
        );
        assert_eq!(
            report.verify.failure_reason.as_deref(),
            Some("LIVE_DCC_VERIFICATION_REQUIRED")
        );
        assert_eq!(report.receipt_path, None);

        let mut actual = serde_json::to_value(report).unwrap();
        actual["core_version"] = serde_json::json!("0.0.0-test");
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/install-execution-report-v1-success.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn execution_report_mid_failure_rolls_back_completed_steps_in_reverse() {
        let service = InstallService::new(PathBuf::from("__unused__.yml"));
        let mut plan = verify_plan(InstallStepAction::PathCopy {
            source: PathBuf::from("source"),
            dest: PathBuf::from("dest"),
        });
        plan.steps.insert(
            1,
            InstallStep {
                name: "register-dcc".into(),
                description: "Register adapter".into(),
                action: Some(InstallStepAction::RegisterDcc {
                    dcc_type: "maya".into(),
                    entry_point: None,
                    dcc_path: None,
                }),
            },
        );
        let mut calls = 0;
        let mut rollback_order = Vec::new();

        let report = service.execute_plan_with(
            &plan,
            true,
            |_action, _plan| {
                calls += 1;
                if calls == 1 {
                    Ok(StepExecution::Completed(Some(StepRollback::RemovePath(
                        PathBuf::from("secret-a"),
                    ))))
                } else {
                    Err(InstallError::StepFailed {
                        step: "secret-stage".into(),
                        message: "token=super-secret".into(),
                    })
                }
            },
            |rollback| {
                if let StepRollback::RemovePath(path) = rollback {
                    rollback_order.push(path.clone());
                }
                Ok(())
            },
        );

        assert_eq!(rollback_order, vec![PathBuf::from("secret-a")]);
        assert_eq!(report.status, "failed");
        assert_eq!(report.stage, "install");
        assert_eq!(report.exit_code, 30);
        assert_eq!(report.error.as_ref().unwrap().code, "INSTALL_STEP_FAILED");
        assert_eq!(report.steps[0].status, "ok");
        assert!(report.steps[0].rollback.attempted);
        assert_eq!(report.steps[0].rollback.status, "ok");
        assert_eq!(report.steps[1].status, "failed");
        assert_eq!(report.steps[2].status, "not_run");
        assert!(report.rollback.attempted);
        assert_eq!(report.rollback.status, "ok");
        assert_eq!(report.rollback.failure_count, 0);
        assert_eq!(
            report.verify.failure_reason.as_deref(),
            Some("INSTALL_STEP_FAILED")
        );

        let public = serde_json::to_string(&report).unwrap();
        assert!(!public.contains("secret-a"));
        assert!(!public.contains("secret-stage"));
        assert!(!public.contains("super-secret"));
    }

    #[test]
    fn execution_report_rollback_failure_is_partial_and_stable() {
        let service = InstallService::new(PathBuf::from("__unused__.yml"));
        let mut plan = verify_plan(InstallStepAction::PathCopy {
            source: PathBuf::from("source"),
            dest: PathBuf::from("dest"),
        });
        plan.steps.insert(
            1,
            InstallStep {
                name: "register-dcc".into(),
                description: "Register adapter".into(),
                action: Some(InstallStepAction::RegisterDcc {
                    dcc_type: "maya".into(),
                    entry_point: None,
                    dcc_path: None,
                }),
            },
        );
        let mut calls = 0;

        let report = service.execute_plan_with(
            &plan,
            true,
            |_action, _plan| {
                calls += 1;
                if calls == 1 {
                    Ok(StepExecution::Completed(Some(StepRollback::RemovePath(
                        PathBuf::from("secret-path"),
                    ))))
                } else {
                    Err(InstallError::StepFailed {
                        step: "install".into(),
                        message: "primary secret".into(),
                    })
                }
            },
            |_rollback| {
                Err(InstallError::RollbackFailed {
                    step: "secret-program".into(),
                    message: "rollback secret".into(),
                })
            },
        );

        assert_eq!(report.status, "partial");
        assert_eq!(report.stage, "rollback");
        assert_eq!(report.exit_code, 30);
        let error = report.error.as_ref().unwrap();
        assert_eq!(error.code, "ROLLBACK_FAILED");
        assert_eq!(error.primary_code.as_deref(), Some("INSTALL_STEP_FAILED"));
        assert_eq!(report.steps[0].rollback.status, "failed");
        assert_eq!(report.rollback.status, "failed");
        assert_eq!(report.rollback.failure_count, 1);
        assert_eq!(
            report.verify.failure_reason.as_deref(),
            Some("ROLLBACK_FAILED")
        );

        let public = serde_json::to_string(&report).unwrap();
        for secret in [
            "secret-path",
            "secret-program",
            "primary secret",
            "rollback secret",
        ] {
            assert!(!public.contains(secret));
        }

        let mut actual = serde_json::to_value(report).unwrap();
        actual["core_version"] = serde_json::json!("0.0.0-test");
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/install-execution-report-v1-rollback-failed.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn failed_execution_report_matches_shared_cross_language_fixture() {
        let service = InstallService::new(PathBuf::from("__unused__.yml"));
        let mut plan = verify_plan(InstallStepAction::PathCopy {
            source: PathBuf::from("source"),
            dest: PathBuf::from("dest"),
        });
        plan.steps.insert(
            1,
            InstallStep {
                name: "register-dcc".into(),
                description: "Register adapter".into(),
                action: Some(InstallStepAction::RegisterDcc {
                    dcc_type: "maya".into(),
                    entry_point: None,
                    dcc_path: None,
                }),
            },
        );
        let mut calls = 0;
        let report = service.execute_plan_with(
            &plan,
            true,
            |_action, _plan| {
                calls += 1;
                if calls == 1 {
                    Ok(StepExecution::Completed(Some(StepRollback::RemovePath(
                        PathBuf::from("private"),
                    ))))
                } else {
                    Err(InstallError::StepFailed {
                        step: "private".into(),
                        message: "private".into(),
                    })
                }
            },
            |_rollback| Ok(()),
        );
        let mut actual = serde_json::to_value(report).unwrap();
        actual["core_version"] = serde_json::json!("0.0.0-test");
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/install-execution-report-v1-failed.json"
        ))
        .unwrap();

        assert_eq!(actual, expected);
    }
}
