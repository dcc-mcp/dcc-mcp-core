use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::domain::install::{
    InstallPlan, InstallPlanError, InstallPlanner, InstallPolicy, InstallRequest, InstallStepAction,
};

const BUNDLED_CATALOG: &str = include_str!("../../../../dcc-mcp-catalog.yml");
const INSTALL_DISABLED_ENV: &str = "DCC_MCP_INSTALL_DISABLED";
const INSTALL_DISABLED_PROMPT_ENV: &str = "DCC_MCP_INSTALL_DISABLED_PROMPT";
const DEFAULT_INSTALL_DISABLED_PROMPT: &str = "Automatic DCC adapter installation is disabled in this environment. Ask your Pipeline TD or studio deployment owner to deploy {adapter} for {dcc_type}, then start the DCC plugin and rerun `dcc-mcp-cli list`.";

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

/// Result of a single executed step with optional rollback data.
struct StepResult {
    step_name: String,
    /// Rollback action to undo this step, if applicable.
    rollback: Option<StepRollback>,
}

/// Describes how to undo a completed step.
#[derive(Debug)]
enum StepRollback {
    /// Remove a file or directory that was created.
    RemovePath(PathBuf),
    /// Run a shell command to revert.
    Command { program: String, args: Vec<String> },
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

    /// Generate and execute an install plan with user consent.
    ///
    /// Steps are executed sequentially.  If any step fails, all prior steps
    /// are rolled back in reverse order.  Returns the full plan (with step
    /// results) on success.
    pub fn execute(&self, request: InstallRequest) -> Result<InstallPlan, InstallError> {
        let plan = self.plan(request)?;
        self.execute_plan(&plan)
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

    fn execute_plan(&self, plan: &InstallPlan) -> Result<InstallPlan, InstallError> {
        if !plan.install_policy.auto_install_enabled {
            if let Some(prompt) = &plan.install_policy.prompt {
                eprintln!("{prompt}");
            }
            return Ok(plan.clone());
        }

        // Filter to executable steps
        let executable_steps: Vec<_> = plan.steps.iter().filter(|s| s.action.is_some()).collect();

        if executable_steps.is_empty() {
            eprintln!("No executable steps in the install plan.");
            return Ok(plan.clone());
        }

        // ── consent gating ────────────────────────────────────────────────
        eprintln!();
        eprintln!("╔══════════════════════════════════════════════╗");
        eprintln!("║         DCC-MCP Install Plan                ║");
        eprintln!("╠══════════════════════════════════════════════╣");
        eprintln!("║  Adapter:  {:<30} ║", plan.adapter.name);
        eprintln!("║  DCC type: {:<30} ║", plan.dcc_type);
        if let Some(ver) = &plan.version {
            eprintln!("║  Version:  {:<30} ║", ver);
        }
        eprintln!("╠══════════════════════════════════════════════╣");
        eprintln!("║  Steps:                                    ║");
        for (i, step) in executable_steps.iter().enumerate() {
            eprintln!("║    {}. {:<34} ║", i + 1, step.name);
            eprintln!("║       {:<34} ║", step.description);
        }
        eprintln!("╚══════════════════════════════════════════════╝");
        eprintln!();

        if !ask_consent("Proceed with installation? [Y/n]")? {
            return Err(InstallError::ConsentDenied);
        }

        // ── execute with rollback support ────────────────────────────────
        let mut completed: Vec<StepResult> = Vec::new();

        for step in &executable_steps {
            let action = step.action.as_ref().expect("filtered to Some above");
            eprint!(
                "  [{}/{}] {} ... ",
                completed.len() + 1,
                executable_steps.len(),
                step.name
            );

            let result = match action {
                InstallStepAction::Verify => execute_verify(plan),
                _ => execute_action(action),
            };

            match result {
                Ok(rollback) => {
                    eprintln!("OK");
                    completed.push(StepResult {
                        step_name: step.name.clone(),
                        rollback,
                    });
                }
                Err(e) => {
                    eprintln!("FAILED");
                    // Roll back all completed steps in reverse order
                    eprintln!("  Rolling back...");
                    rollback_all(&completed);
                    return Err(InstallError::StepFailed {
                        step: step.name.clone(),
                        message: e.to_string(),
                    });
                }
            }
        }

        eprintln!();
        eprintln!("Installation complete for {}.", plan.adapter.name);
        Ok(plan.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoInstallPolicy {
    enabled: bool,
    prompt_template: String,
}

impl AutoInstallPolicy {
    fn from_env() -> Self {
        let disabled = std::env::var(INSTALL_DISABLED_ENV)
            .ok()
            .is_some_and(|value| env_flag_enabled(&value));
        let prompt_template = std::env::var(INSTALL_DISABLED_PROMPT_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_INSTALL_DISABLED_PROMPT.to_string());
        Self {
            enabled: !disabled,
            prompt_template,
        }
    }

    #[cfg(test)]
    fn disabled(prompt_template: impl Into<String>) -> Self {
        Self {
            enabled: false,
            prompt_template: prompt_template.into(),
        }
    }
}

fn env_flag_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "disabled" | "disable"
    )
}

fn render_install_policy_prompt(template: &str, plan: &InstallPlan) -> String {
    template
        .replace("{adapter}", &plan.adapter.name)
        .replace("{dcc_type}", &plan.dcc_type)
        .replace("{version}", plan.version.as_deref().unwrap_or(""))
}

// ── step executors ───────────────────────────────────────────────────────────────

/// Execute a single install action, returning an optional rollback handle.
fn execute_action(action: &InstallStepAction) -> Result<Option<StepRollback>, InstallError> {
    match action {
        InstallStepAction::PipInstall {
            package,
            extras,
            python,
        } => execute_pip_install(package, extras.as_deref(), python.as_deref()),
        InstallStepAction::GitClone { url, ref_, dest } => {
            execute_git_clone(url, ref_.as_deref(), dest)
        }
        InstallStepAction::ZipExtract { url, sha256, dest } => {
            execute_zip_extract(url, sha256.as_deref(), dest)
        }
        InstallStepAction::PathCopy { source, dest } => execute_path_copy(source, dest),
        InstallStepAction::RegisterDcc {
            dcc_type,
            entry_point,
        } => execute_register_dcc(dcc_type, entry_point.as_deref()),
        InstallStepAction::Verify => Err(InstallError::StepFailed {
            step: "verify".into(),
            message: "verify requires the full install plan".into(),
        }),
    }
}

fn execute_pip_install(
    package: &str,
    extras: Option<&[String]>,
    python: Option<&str>,
) -> Result<Option<StepRollback>, InstallError> {
    let python_cmd = python.unwrap_or("python");
    let mut cmd = Command::new(python_cmd);
    cmd.args(pip_install_args(package, extras));

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

fn pip_package_spec(package: &str, extras: Option<&[String]>) -> String {
    if extras.is_some_and(|values| !values.is_empty()) {
        format!("{}[{}]", package, extras.unwrap().join(","))
    } else {
        package.to_string()
    }
}

fn pip_install_args(package: &str, extras: Option<&[String]>) -> Vec<String> {
    vec![
        "-m".into(),
        "pip".into(),
        "install".into(),
        pip_package_spec(package, extras),
    ]
}

fn pip_uninstall_args(package: &str) -> Vec<String> {
    vec![
        "-m".into(),
        "pip".into(),
        "uninstall".into(),
        "-y".into(),
        package.to_string(),
    ]
}

fn pip_show_args(package: &str) -> Vec<String> {
    vec![
        "-m".into(),
        "pip".into(),
        "show".into(),
        package.to_string(),
    ]
}

fn execute_git_clone(
    url: &str,
    ref_: Option<&str>,
    dest: &Path,
) -> Result<Option<StepRollback>, InstallError> {
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

    let mut cmd = Command::new("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(r) = ref_.filter(|v| !v.trim().is_empty()) {
        cmd.arg("--branch").arg(r);
    }
    cmd.arg(url).arg(dest);

    let status = cmd.status().map_err(|e| InstallError::StepFailed {
        step: "git-clone".into(),
        message: format!("failed to launch git: {e}"),
    })?;

    if !status.success() {
        return Err(InstallError::StepFailed {
            step: "git-clone".into(),
            message: format!("git clone exited with {status}"),
        });
    }

    Ok(Some(StepRollback::RemovePath(dest.to_path_buf())))
}

fn execute_zip_extract(
    url: &str,
    sha256: Option<&str>,
    dest: &Path,
) -> Result<Option<StepRollback>, InstallError> {
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

    // Verify SHA-256 if requested
    if let Some(expected) = sha256 {
        use sha2::Digest;
        let actual = sha2::Sha256::digest(&bytes);
        let actual_hex = actual
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if !actual_hex.eq_ignore_ascii_case(expected) {
            return Err(InstallError::StepFailed {
                step: "zip-checksum".into(),
                message: format!("SHA-256 mismatch: expected {expected}, got {actual_hex}"),
            });
        }
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
) -> Result<Option<StepRollback>, InstallError> {
    // Registration is owned by the DCC plugin's sidecar. The CLI can install
    // packages, but it must not pretend a host process is registered until the
    // plugin starts, stays alive, and advertises itself in the registry.
    eprintln!();
    eprint!("    └─ note: start or enable the '{_dcc_type}' plugin, ");
    eprintln!("then re-run `dcc-mcp-cli list` to confirm self-registration.");
    Ok(None)
}

fn execute_verify(plan: &InstallPlan) -> Result<Option<StepRollback>, InstallError> {
    for step in &plan.steps {
        let Some(action) = &step.action else {
            continue;
        };
        match action {
            InstallStepAction::GitClone { dest, .. }
            | InstallStepAction::ZipExtract { dest, .. }
            | InstallStepAction::PathCopy { dest, .. } => verify_installed_path(dest)?,
            InstallStepAction::PipInstall {
                package, python, ..
            } => verify_pip_package(package, python.as_deref())?,
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

fn verify_pip_package(package: &str, python: Option<&str>) -> Result<(), InstallError> {
    let python_cmd = python.unwrap_or("python");
    let status = Command::new(python_cmd)
        .args(pip_show_args(package))
        .status()
        .map_err(|e| InstallError::StepFailed {
            step: "verify".into(),
            message: format!("failed to launch {python_cmd}: {e}"),
        })?;
    if !status.success() {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!("{python_cmd} could not verify installed pip package '{package}'"),
        });
    }
    Ok(())
}

// ── consent ──────────────────────────────────────────────────────────────────────

/// Prompt the user for Y/n consent.  Returns `true` if the user agrees.
fn ask_consent(prompt: &str) -> Result<bool, InstallError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    loop {
        write!(stdout, "{prompt} ")?;
        stdout.flush()?;

        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let trimmed = line.trim().to_lowercase();

        match trimmed.as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {
                write!(stdout, "  Please answer Y or n: ")?;
                stdout.flush()?;
            }
        }
    }
}

// ── rollback ─────────────────────────────────────────────────────────────────────

/// Roll back all completed steps in reverse order, best-effort.
fn rollback_all(completed: &[StepResult]) {
    for result in completed.iter().rev() {
        if let Some(rollback) = &result.rollback
            && let Err(e) = execute_rollback(rollback)
        {
            eprintln!("  ⚠  rollback of '{}' failed: {e}", result.step_name);
        }
    }
}

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
            let status = Command::new(program).args(args).status().map_err(|e| {
                InstallError::RollbackFailed {
                    step: program.clone(),
                    message: format!("failed to launch {program}: {e}"),
                }
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
            })
            .unwrap();

        assert_eq!(plan.adapter.name, "dcc-mcp-maya");
        assert!(matches!(
            plan.steps[0].action,
            Some(InstallStepAction::PipInstall { .. })
        ));
    }

    #[test]
    fn bundled_catalog_keeps_skill_packs_guidance_only() {
        let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
        let plan = service
            .plan(InstallRequest {
                dcc_type: "unreal".into(),
                version: None,
                catalog_path: None,
                python: None,
            })
            .unwrap();

        assert_eq!(plan.adapter.name, "dcc-mcp-unreal-skills");
        assert!(plan.steps.iter().all(|s| s.action.is_none()));
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
            })
            .unwrap();

        assert!(!plan.install_policy.auto_install_enabled);
        assert_eq!(
            plan.install_policy.prompt.as_deref(),
            Some("Auto install unavailable; contact PipelineTD to deploy dcc-mcp-maya for maya.")
        );
    }

    #[test]
    fn execute_returns_plan_without_running_steps_when_auto_install_is_disabled() {
        let service = InstallService::with_auto_install_policy(
            PathBuf::from("__missing_dcc_mcp_catalog__.yml"),
            AutoInstallPolicy::disabled(
                "Automatic install disabled; ask PipelineTD for {adapter}.",
            ),
        );

        let plan = service
            .execute(InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
                python: Some("/__nonexistent__/python".into()),
            })
            .unwrap();

        assert!(!plan.install_policy.auto_install_enabled);
        assert_eq!(plan.steps[0].name, "install-pip");
    }

    #[test]
    fn pip_install_missing_python_reports_error() {
        let result =
            execute_pip_install("nonexistent-package", None, Some("/__nonexistent__/python"));
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
    fn register_dcc_is_noop() {
        let result = execute_register_dcc("maya", Some("dcc_mcp_maya.cli:main"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn pip_install_uses_python_module_invocation() {
        assert_eq!(
            pip_install_args("dcc-mcp-maya", Some(&["maya".to_string()])),
            vec![
                "-m".to_string(),
                "pip".to_string(),
                "install".to_string(),
                "dcc-mcp-maya[maya]".to_string(),
            ]
        );
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
            adapter: dcc_mcp_catalog::CatalogEntry {
                name: "dcc-mcp-maya".into(),
                description: "Maya adapter".into(),
                dcc: vec!["maya".into()],
                url: None,
                tags: vec![],
                version: None,
                min_core_version: None,
                install: None,
                maintainer: None,
                icon: None,
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
}
