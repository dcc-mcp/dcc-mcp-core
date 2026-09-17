//! Project-bound DCC launch lifecycle contract (`dcc-mcp-cli start-instance`).
//!
//! Core owns the **lifecycle and readiness** contract; adapters own the
//! **validated launch plan** they publish as a project receipt or as a
//! core-side plan document. Core never invents a DCC-specific command line:
//! with no launch plan the operation fails closed with
//! [`BlockingState::LaunchPlanMissing`] instead of guessing an executable.
//!
//! Safety boundaries encoded here:
//!
//! * launching a GUI process needs explicit operator authorization;
//! * `start-instance` never downloads, installs, upgrades, or edits config;
//! * process creation is never reported as adapter readiness;
//! * one operation ID is preserved from launch through terminal readiness;
//! * an instance opened on another project is never stolen, closed, or reused.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Schema version of the adapter-authored launch-plan document.
pub const LAUNCH_PLAN_SCHEMA_VERSION: u8 = 1;
/// Schema version of the terminal `start-instance` report.
pub const START_INSTANCE_REPORT_SCHEMA_VERSION: u8 = 1;
/// Schema version of a persisted lifecycle operation record.
pub const LIFECYCLE_OPERATION_SCHEMA_VERSION: u8 = 1;

/// Argv placeholders Core expands before spawning the host.
pub const PLACEHOLDER_EXECUTABLE: &str = "{executable}";
pub const PLACEHOLDER_PROJECT: &str = "{project}";
pub const PLACEHOLDER_DCC_TYPE: &str = "{dcc_type}";
pub const PLACEHOLDER_VERSION: &str = "{version}";

/// Adapter-authored launch plan, relative to the bound project root.
pub const PROJECT_LAUNCH_PLAN_RELATIVE_PATH: &str = ".dcc-mcp/launch-plan.json";
/// Directory (relative to the core registry dir) holding core-side launch plans.
pub const LAUNCH_PLAN_DIR_NAME: &str = "launch-plans";
/// Directory (relative to the core registry dir) holding lifecycle operations.
pub const LIFECYCLE_DIR_NAME: &str = "start-instance";
/// Index file mapping `dcc_type|project` to the owning operation.
pub const LIFECYCLE_INDEX_FILE: &str = "index.json";

/// Environment variables handed to the launched host so the adapter can
/// correlate registration, sidecar bootstrap, and readiness with one operation.
pub const OPERATION_ID_ENV: &str = "DCC_MCP_START_OPERATION_ID";
pub const PROJECT_ENV: &str = "DCC_MCP_START_PROJECT";
pub const DCC_TYPE_ENV: &str = "DCC_MCP_START_DCC_TYPE";

/// Registry-row metadata keys binding a live instance to a bound project.
pub const PROJECT_METADATA_KEYS: &[&str] = &["dcc_mcp_project", "project", "dcc_mcp.project"];
/// Registry-row metadata keys binding a live instance to one launch operation.
pub const OPERATION_METADATA_KEYS: &[&str] = &[
    "dcc_mcp_operation_id",
    "operation_id",
    "dcc_mcp.operation_id",
];

/// Ordered lifecycle stages. Used as `timeout_stage` whenever an operation
/// stops short of terminal readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStage {
    /// Resolving and validating the adapter launch plan.
    ResolvePlan,
    /// Waiting for explicit operator authorization to launch a GUI process.
    Authorization,
    /// Binding the canonical project path and its adapter markers.
    ProjectBinding,
    /// Looking for an already-running instance bound to the same project.
    Reuse,
    /// Spawning the recorded DCC executable.
    Launch,
    /// Waiting for the launched host to register in the FileRegistry.
    Registration,
    /// Waiting for the registered instance to report readiness bits.
    Readiness,
    /// The operation reached a terminal state.
    Terminal,
}

impl LaunchStage {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResolvePlan => "resolve_plan",
            Self::Authorization => "authorization",
            Self::ProjectBinding => "project_binding",
            Self::Reuse => "reuse",
            Self::Launch => "launch",
            Self::Registration => "registration",
            Self::Readiness => "readiness",
            Self::Terminal => "terminal",
        }
    }
}

/// Terminal reason an operation did not reach readiness, and the safe next
/// action the operator or agent should take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockingState {
    /// Terminal readiness reached.
    None,
    /// The host requires a restart before the adapter can serve calls.
    RestartRequired,
    /// The project is locked by another Editor/host process.
    ProjectLock,
    /// The host cannot serve calls because of a license problem.
    License,
    /// A modal dialog is blocking the host (import, update, compile prompt).
    ModalDialog,
    /// The adapter/sidecar never bootstrapped on the launched host.
    AdapterBootstrap,
    /// The recorded executable is gone or is not a file.
    MissingExecutable,
    /// The recorded version does not match the requested version.
    VersionMismatch,
    /// Several live instances claim the same project; Core refuses to choose.
    AmbiguousReuse,
    /// No launch plan could be resolved for the DCC type + project.
    LaunchPlanMissing,
    /// The launch needs explicit operator authorization.
    AuthorizationRequired,
    /// The operation hit its timeout before terminal readiness.
    Timeout,
    /// The operator cancelled the operation.
    Cancelled,
    /// The launch plan is malformed or unsafe to execute.
    InvalidLaunchPlan,
    /// The project path does not exist or is not a directory.
    ProjectNotFound,
    /// The project does not carry the adapter's binding markers.
    ProjectMarkerMissing,
    /// Spawning the recorded executable failed.
    LaunchFailed,
}

impl BlockingState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::RestartRequired => "restart_required",
            Self::ProjectLock => "project_lock",
            Self::License => "license",
            Self::ModalDialog => "modal_dialog",
            Self::AdapterBootstrap => "adapter_bootstrap",
            Self::MissingExecutable => "missing_executable",
            Self::VersionMismatch => "version_mismatch",
            Self::AmbiguousReuse => "ambiguous_reuse",
            Self::LaunchPlanMissing => "launch_plan_missing",
            Self::AuthorizationRequired => "authorization_required",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::InvalidLaunchPlan => "invalid_launch_plan",
            Self::ProjectNotFound => "project_not_found",
            Self::ProjectMarkerMissing => "project_marker_missing",
            Self::LaunchFailed => "launch_failed",
        }
    }

    /// True when the caller can retry the same operation without changing
    /// machine state.
    #[must_use]
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::AdapterBootstrap
                | Self::ProjectLock
                | Self::ModalDialog
                | Self::LaunchFailed
                | Self::AmbiguousReuse
        )
    }

    /// Machine-readable next action. `command` is copy-pasteable and always
    /// non-interactive so an agent can execute it verbatim.
    #[must_use]
    pub fn next_action(
        self,
        dcc_type: &str,
        project: Option<&Path>,
        operation_id: Option<&str>,
    ) -> Value {
        let project_args = |extra: &[&str]| -> Vec<String> {
            let mut command = vec![
                "dcc-mcp-cli".to_string(),
                "--output".to_string(),
                "json".to_string(),
                "--non-interactive".to_string(),
                "start-instance".to_string(),
                "--dcc-type".to_string(),
                dcc_type.to_string(),
            ];
            if let Some(project) = project {
                command.push("--project".to_string());
                command.push(project.display().to_string());
            }
            command.extend(extra.iter().map(|flag| (*flag).to_string()));
            command.push("--yes".to_string());
            command
        };
        let stop_command = |operation: Option<&str>| -> Vec<String> {
            let mut command = vec![
                "dcc-mcp-cli".to_string(),
                "--output".to_string(),
                "json".to_string(),
                "--non-interactive".to_string(),
                "stop-instance".to_string(),
            ];
            if let Some(operation) = operation {
                command.push("--operation-id".to_string());
                command.push(operation.to_string());
            }
            command
        };

        match self {
            Self::None => json!({
                "id": "call_instance",
                "summary": "Instance is ready; continue with search/describe/call on the returned instance.",
                "requires_consent": false,
                "command": vec![
                    "dcc-mcp-cli".to_string(),
                    "--output".to_string(),
                    "json".to_string(),
                    "search".to_string(),
                    "--dcc-type".to_string(),
                    dcc_type.to_string(),
                ],
            }),
            Self::RestartRequired => json!({
                "id": "restart_host",
                "summary": "The host must be restarted before the adapter can serve calls. Stop the owned instance, then start it again.",
                "requires_consent": true,
                "command": stop_command(operation_id),
            }),
            Self::ProjectLock => json!({
                "id": "release_project_lock",
                "summary": "Another host process holds this project. Close it or pick a different project, then retry.",
                "requires_consent": true,
                "command": project_args(&[]),
            }),
            Self::License => json!({
                "id": "resolve_license",
                "summary": "The host reported a license problem. Resolve it in the host UI, then retry the same operation.",
                "requires_consent": true,
                "command": project_args(&[]),
            }),
            Self::ModalDialog => json!({
                "id": "dismiss_modal_dialog",
                "summary": "A modal dialog is blocking the host. Dismiss it in the host UI, then retry the same operation.",
                "requires_consent": true,
                "command": project_args(&[]),
            }),
            Self::AdapterBootstrap => json!({
                "id": "inspect_adapter_bootstrap",
                "summary": "The host is running but the adapter/sidecar never registered. Inspect the instance diagnostics, then retry.",
                "requires_consent": false,
                "command": vec![
                    "dcc-mcp-cli".to_string(),
                    "--output".to_string(),
                    "json".to_string(),
                    "doctor".to_string(),
                ],
            }),
            Self::MissingExecutable => json!({
                "id": "reinstall_adapter",
                "summary": "The recorded DCC executable is missing. Re-run the adapter install so the receipt points at a real executable.",
                "requires_consent": false,
                "command": vec![
                    "dcc-mcp-cli".to_string(),
                    "--output".to_string(),
                    "json".to_string(),
                    "--non-interactive".to_string(),
                    "install".to_string(),
                    "--dcc-type".to_string(),
                    dcc_type.to_string(),
                ],
            }),
            Self::VersionMismatch => json!({
                "id": "pin_matching_version",
                "summary": "The recorded version does not match the requested version. Start without --version, or install the requested version.",
                "requires_consent": false,
                "command": project_args(&[]),
            }),
            Self::AmbiguousReuse => json!({
                "id": "select_exact_instance",
                "summary": "Several live instances claim this project. Stop the extra instance or pass --instance-id to target one exactly.",
                "requires_consent": true,
                "command": vec![
                    "dcc-mcp-cli".to_string(),
                    "--output".to_string(),
                    "json".to_string(),
                    "list".to_string(),
                ],
            }),
            Self::LaunchPlanMissing => json!({
                "id": "install_adapter",
                "summary": "No launch plan is published for this DCC type and project. Install the adapter so it writes a validated launch plan.",
                "requires_consent": false,
                "command": vec![
                    "dcc-mcp-cli".to_string(),
                    "--output".to_string(),
                    "json".to_string(),
                    "--non-interactive".to_string(),
                    "install".to_string(),
                    "--dcc-type".to_string(),
                    dcc_type.to_string(),
                ],
            }),
            Self::AuthorizationRequired => json!({
                "id": "authorize_launch",
                "summary": "Launching a GUI host needs explicit operator authorization. Re-run with --yes (or interactively confirm).",
                "requires_consent": true,
                "command": project_args(&[]),
            }),
            Self::Timeout => json!({
                "id": "retry_with_longer_timeout",
                "summary": "The operation timed out before terminal readiness. Retry with a larger --timeout-secs, or inspect the instance diagnostics.",
                "requires_consent": false,
                "command": project_args(&["--wait-ready", "--timeout-secs", "300"]),
            }),
            Self::Cancelled => json!({
                "id": "operator_cancelled",
                "summary": "The operator cancelled the launch. Re-run when the host may be started.",
                "requires_consent": true,
                "command": project_args(&[]),
            }),
            Self::InvalidLaunchPlan => json!({
                "id": "repair_launch_plan",
                "summary": "The launch plan is malformed or unsafe. Re-install the adapter so it republishes a validated launch plan.",
                "requires_consent": false,
                "command": vec![
                    "dcc-mcp-cli".to_string(),
                    "--output".to_string(),
                    "json".to_string(),
                    "--non-interactive".to_string(),
                    "install".to_string(),
                    "--dcc-type".to_string(),
                    dcc_type.to_string(),
                ],
            }),
            Self::ProjectNotFound => json!({
                "id": "fix_project_path",
                "summary": "The project path does not exist or is not a directory. Pass the exact absolute project root.",
                "requires_consent": false,
                "command": vec![
                    "dcc-mcp-cli".to_string(),
                    "--output".to_string(),
                    "json".to_string(),
                    "--non-interactive".to_string(),
                    "start-instance".to_string(),
                    "--dcc-type".to_string(),
                    dcc_type.to_string(),
                    "--project".to_string(),
                    "<absolute-project-path>".to_string(),
                    "--yes".to_string(),
                ],
            }),
            Self::ProjectMarkerMissing => json!({
                "id": "verify_project_binding",
                "summary": "The project is missing the adapter's binding markers, so it may not be the project this adapter was installed for.",
                "requires_consent": false,
                "command": vec![
                    "dcc-mcp-cli".to_string(),
                    "--output".to_string(),
                    "json".to_string(),
                    "doctor".to_string(),
                ],
            }),
            Self::LaunchFailed => json!({
                "id": "inspect_launch_failure",
                "summary": "Spawning the recorded executable failed. Inspect the executable path and OS error, then retry.",
                "requires_consent": false,
                "command": project_args(&[]),
            }),
        }
    }
}

/// Where a launch plan came from. Most specific source wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchPlanSource {
    /// `--launch-plan <path>` or `--executable <path>` on the command line.
    Explicit,
    /// `<project>/.dcc-mcp/launch-plan.json` published by the adapter.
    ProjectReceipt,
    /// Core state registry: `<registry_dir>/launch-plans/<dcc_type>.json`.
    StateRegistry,
}

impl LaunchPlanSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::ProjectReceipt => "project_receipt",
            Self::StateRegistry => "state_registry",
        }
    }
}

/// Adapter-authored launch plan document.
#[derive(Debug, Clone, Deserialize)]
pub struct LaunchPlanDocument {
    #[serde(default)]
    pub schema_version: Option<u8>,
    pub dcc_type: String,
    pub executable: PathBuf,
    /// Argv template. Placeholders: `{executable}`, `{project}`, `{dcc_type}`,
    /// `{version}`. Required — Core never invents a DCC command line.
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Relative paths that must exist inside the project for Core to accept the
    /// binding (for example `ProjectSettings/ProjectVersion.txt`).
    #[serde(default)]
    pub project_markers: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

/// A validated, fully expanded launch plan ready to spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub dcc_type: String,
    pub executable: PathBuf,
    pub argv: Vec<String>,
    pub version: Option<String>,
    pub project_markers: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub source: LaunchPlanSource,
    pub plan_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchPlanError {
    UnsupportedSchema {
        found: u8,
        supported: u8,
    },
    DccTypeMismatch {
        expected: String,
        found: String,
    },
    MissingExecutable(PathBuf),
    EmptyArgv,
    ArgvEscapesExecutable(String),
    VersionMismatch {
        expected: String,
        found: Option<String>,
    },
    UnreadablePlan(PathBuf),
    MalformedPlan(String),
}

impl LaunchPlanError {
    #[must_use]
    pub fn blocking_state(&self) -> BlockingState {
        match self {
            Self::UnsupportedSchema { .. }
            | Self::DccTypeMismatch { .. }
            | Self::EmptyArgv
            | Self::ArgvEscapesExecutable(_)
            | Self::UnreadablePlan(_)
            | Self::MalformedPlan(_) => BlockingState::InvalidLaunchPlan,
            Self::MissingExecutable(_) => BlockingState::MissingExecutable,
            Self::VersionMismatch { .. } => BlockingState::VersionMismatch,
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::UnsupportedSchema { found, supported } => format!(
                "launch plan schema_version {found} is newer than the supported version {supported}"
            ),
            Self::DccTypeMismatch { expected, found } => {
                format!("launch plan declares dcc_type '{found}' but '{expected}' was requested")
            }
            Self::MissingExecutable(path) => {
                format!("launch plan executable is missing: {}", path.display())
            }
            Self::EmptyArgv => {
                "launch plan argv is empty; adapters must publish an explicit argv".to_string()
            }
            Self::ArgvEscapesExecutable(found) => {
                format!("launch plan argv[0] '{found}' is not the recorded executable")
            }
            Self::VersionMismatch { expected, found } => format!(
                "requested version '{expected}' does not match launch plan version {:?}",
                found.as_deref()
            ),
            Self::UnreadablePlan(path) => {
                format!("launch plan could not be read: {}", path.display())
            }
            Self::MalformedPlan(detail) => format!("launch plan is malformed: {detail}"),
        }
    }
}

impl LaunchPlan {
    /// Expand and validate a launch plan document against a bound project.
    pub fn from_document(
        document: LaunchPlanDocument,
        dcc_type: &str,
        project: &Path,
        requested_version: Option<&str>,
        source: LaunchPlanSource,
        plan_path: Option<PathBuf>,
    ) -> Result<Self, LaunchPlanError> {
        let declared = document
            .schema_version
            .unwrap_or(LAUNCH_PLAN_SCHEMA_VERSION);
        if declared > LAUNCH_PLAN_SCHEMA_VERSION {
            return Err(LaunchPlanError::UnsupportedSchema {
                found: declared,
                supported: LAUNCH_PLAN_SCHEMA_VERSION,
            });
        }
        if !document.dcc_type.eq_ignore_ascii_case(dcc_type) {
            return Err(LaunchPlanError::DccTypeMismatch {
                expected: dcc_type.to_string(),
                found: document.dcc_type,
            });
        }
        if requested_version.is_some() && document.version.as_deref() != requested_version {
            return Err(LaunchPlanError::VersionMismatch {
                expected: requested_version.unwrap_or_default().to_string(),
                found: document.version,
            });
        }
        let executable = document.executable;
        if !executable.is_absolute() || !executable.is_file() {
            return Err(LaunchPlanError::MissingExecutable(executable));
        }
        if document.argv.is_empty() {
            return Err(LaunchPlanError::EmptyArgv);
        }

        let version = document.version.clone().or_else(|| {
            requested_version
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
        let argv = document
            .argv
            .iter()
            .map(|token| {
                token
                    .replace(PLACEHOLDER_EXECUTABLE, &executable.display().to_string())
                    .replace(PLACEHOLDER_PROJECT, &project.display().to_string())
                    .replace(PLACEHOLDER_DCC_TYPE, dcc_type)
                    .replace(PLACEHOLDER_VERSION, version.as_deref().unwrap_or_default())
            })
            .collect::<Vec<_>>();
        if !same_executable(argv.first().map(String::as_str), &executable) {
            return Err(LaunchPlanError::ArgvEscapesExecutable(
                argv.first().cloned().unwrap_or_default(),
            ));
        }

        Ok(Self {
            dcc_type: document.dcc_type,
            executable,
            argv,
            version,
            project_markers: document.project_markers,
            cwd: document.cwd,
            source,
            plan_path,
        })
    }

    /// Redacted copy safe to print: the argv is operator-auditable and contains
    /// no secrets, but the plan path is host-local so callers choose exposure.
    #[must_use]
    pub fn summary(&self) -> Value {
        json!({
            "dcc_type": self.dcc_type,
            "executable": self.executable,
            "argv": self.argv,
            "version": self.version,
            "project_markers": self.project_markers,
            "source": self.source.as_str(),
            "plan_path": self.plan_path,
        })
    }
}

/// `argv[0]` must be the recorded executable itself. A loose file-name match
/// would let a plan spawn a different binary than the one Core verified, so the
/// comparison is exact.
fn same_executable(argv0: Option<&str>, executable: &Path) -> bool {
    let Some(argv0) = argv0.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    Path::new(argv0) == executable
}

/// Persisted lifecycle operation record. Backs convergence (a second identical
/// request must not launch another host) and the guarded stop operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleOperation {
    pub schema_version: u8,
    pub operation_id: String,
    pub dcc_type: String,
    pub project: PathBuf,
    pub executable: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// True when this operation spawned the process itself.
    pub launched: bool,
    /// True when this operation owns the process and may stop it.
    pub owned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking_state: Option<String>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

impl LifecycleOperation {
    #[must_use]
    pub fn new(
        operation_id: impl Into<String>,
        dcc_type: &str,
        project: &Path,
        executable: &Path,
        version: Option<String>,
    ) -> Self {
        let now = unix_now_secs();
        Self {
            schema_version: LIFECYCLE_OPERATION_SCHEMA_VERSION,
            operation_id: operation_id.into(),
            dcc_type: dcc_type.to_string(),
            project: project.to_path_buf(),
            executable: executable.to_path_buf(),
            version,
            pid: None,
            launched: false,
            owned: false,
            instance_id: None,
            blocking_state: None,
            created_at_unix: now,
            updated_at_unix: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at_unix = unix_now_secs();
    }
}

/// Request for one project-bound start-and-wait operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartInstanceRequest {
    pub dcc_type: String,
    pub project: PathBuf,
    /// Explicit launch plan document path.
    pub launch_plan: Option<PathBuf>,
    /// Require the launch plan to declare this exact version.
    pub version: Option<String>,
    pub wait_ready: bool,
    pub timeout: Duration,
    pub interval: Duration,
    /// Readiness bits required for terminal success.
    pub required: Vec<String>,
    /// Operator authorization to launch a GUI process.
    pub authorized: bool,
    /// Resolve and report the plan without spawning anything.
    pub dry_run: bool,
    pub registry_dir: PathBuf,
    /// Reuse only this exact instance when converging.
    pub instance_id: Option<String>,
}

/// Classify a live instance's advertised failure/blocking metadata into a safe
/// operator-facing state. Unknown metadata stays [`BlockingState::None`] so Core
/// never invents a diagnosis.
#[must_use]
pub fn classify_blocking_state(
    metadata: &std::collections::HashMap<String, String>,
) -> BlockingState {
    let read = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|key| metadata.get(*key).map(String::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| !value.eq_ignore_ascii_case("none"))
            .map(str::to_string)
    };

    if let Some(value) = read(&[
        "restart_required",
        "dcc_mcp_restart_required",
        "dcc_mcp.restart_required",
    ]) && is_truthy(&value)
    {
        return BlockingState::RestartRequired;
    }
    if let Some(value) = read(&[
        "blocking_dialog",
        "modal_dialog",
        "dcc_mcp_blocking_dialog",
        "dcc_mcp.modal_dialog",
    ]) && is_truthy(&value)
    {
        return BlockingState::ModalDialog;
    }
    if let Some(value) = read(&[
        "project_lock",
        "project_locked",
        "dcc_mcp_project_lock",
        "dcc_mcp.project_lock",
    ]) && is_truthy(&value)
    {
        return BlockingState::ProjectLock;
    }
    if let Some(value) = read(&[
        "license_state",
        "license_status",
        "dcc_mcp_license_state",
        "dcc_mcp.license_state",
    ]) && !value.eq_ignore_ascii_case("valid")
        && !value.eq_ignore_ascii_case("ok")
        && !value.eq_ignore_ascii_case("active")
    {
        return BlockingState::License;
    }
    let failure_stage = read(&["failure_stage"]).unwrap_or_default();
    let failure_reason = read(&["failure_reason"]).unwrap_or_default();
    let haystack = format!("{failure_stage} {failure_reason}").to_ascii_lowercase();
    if haystack.contains("bootstrap") || haystack.contains("sidecar") {
        return BlockingState::AdapterBootstrap;
    }
    if haystack.contains("license") {
        return BlockingState::License;
    }
    if haystack.contains("dialog") || haystack.contains("modal") {
        return BlockingState::ModalDialog;
    }
    if haystack.contains("lock") {
        return BlockingState::ProjectLock;
    }
    if haystack.contains("restart") {
        return BlockingState::RestartRequired;
    }
    BlockingState::None
}

fn is_truthy(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "blocked" | "blocking" | "present" | "open"
    )
}

#[must_use]
pub fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Writes a real file so `LaunchPlan::from_document` can verify the
    /// recorded executable exists.
    fn fake_executable(dir: &Path) -> PathBuf {
        let path = dir.join("Unity");
        std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        path
    }

    fn document(executable: PathBuf) -> LaunchPlanDocument {
        LaunchPlanDocument {
            schema_version: Some(LAUNCH_PLAN_SCHEMA_VERSION),
            dcc_type: "unity".to_string(),
            executable,
            argv: vec![
                PLACEHOLDER_EXECUTABLE.to_string(),
                "-projectPath".to_string(),
                PLACEHOLDER_PROJECT.to_string(),
            ],
            version: Some("2022.3.10f1".to_string()),
            project_markers: vec!["ProjectSettings/ProjectVersion.txt".to_string()],
            cwd: None,
        }
    }

    #[test]
    fn placeholders_expand_to_executable_project_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let executable = fake_executable(dir.path());

        let plan = LaunchPlan::from_document(
            document(executable.clone()),
            "unity",
            Path::new("/work/MyProject"),
            None,
            LaunchPlanSource::ProjectReceipt,
            None,
        )
        .unwrap();

        assert_eq!(plan.argv[0], executable.display().to_string());
        assert_eq!(plan.argv[2], "/work/MyProject");
        assert_eq!(plan.version.as_deref(), Some("2022.3.10f1"));
        assert_eq!(plan.source, LaunchPlanSource::ProjectReceipt);
    }

    #[test]
    fn argv_must_stay_on_the_recorded_executable() {
        let dir = tempfile::tempdir().unwrap();
        let executable = fake_executable(dir.path());
        let other = dir.path().join("Other");
        std::fs::write(&other, b"#!/bin/sh\nexit 0\n").unwrap();
        let mut document = document(executable);
        document.argv = vec![other.display().to_string()];

        let error = LaunchPlan::from_document(
            document,
            "unity",
            Path::new("/work/MyProject"),
            None,
            LaunchPlanSource::Explicit,
            None,
        )
        .unwrap_err();

        assert!(matches!(error, LaunchPlanError::ArgvEscapesExecutable(_)));
        assert_eq!(error.blocking_state(), BlockingState::InvalidLaunchPlan);
    }

    #[test]
    fn missing_executable_is_reported_as_a_missing_executable_state() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("DoesNotExist");

        let error = LaunchPlan::from_document(
            document(missing.clone()),
            "unity",
            Path::new("/work/MyProject"),
            None,
            LaunchPlanSource::Explicit,
            None,
        )
        .unwrap_err();

        assert_eq!(error, LaunchPlanError::MissingExecutable(missing));
        assert_eq!(error.blocking_state(), BlockingState::MissingExecutable);
    }

    #[test]
    fn empty_argv_fails_closed_instead_of_guessing_a_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut document = document(fake_executable(dir.path()));
        document.argv = Vec::new();

        let error = LaunchPlan::from_document(
            document,
            "unity",
            Path::new("/work/MyProject"),
            None,
            LaunchPlanSource::Explicit,
            None,
        )
        .unwrap_err();

        assert_eq!(error, LaunchPlanError::EmptyArgv);
        assert_eq!(error.blocking_state(), BlockingState::InvalidLaunchPlan);
    }

    #[test]
    fn version_pin_mismatch_is_reported() {
        let dir = tempfile::tempdir().unwrap();

        let error = LaunchPlan::from_document(
            document(fake_executable(dir.path())),
            "unity",
            Path::new("/work/MyProject"),
            Some("6000.0.1f1"),
            LaunchPlanSource::Explicit,
            None,
        )
        .unwrap_err();

        assert!(matches!(error, LaunchPlanError::VersionMismatch { .. }));
        assert_eq!(error.blocking_state(), BlockingState::VersionMismatch);
    }

    #[test]
    fn newer_schema_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut document = document(fake_executable(dir.path()));
        document.schema_version = Some(LAUNCH_PLAN_SCHEMA_VERSION + 1);

        let error = LaunchPlan::from_document(
            document,
            "unity",
            Path::new("/work/MyProject"),
            None,
            LaunchPlanSource::Explicit,
            None,
        )
        .unwrap_err();

        assert!(matches!(error, LaunchPlanError::UnsupportedSchema { .. }));
    }

    #[test]
    fn blocking_state_classification_prefers_explicit_advertisements() {
        let cases = [
            (
                vec![("restart_required".to_string(), "true".to_string())],
                BlockingState::RestartRequired,
            ),
            (
                vec![("dcc_mcp_project_lock".to_string(), "true".to_string())],
                BlockingState::ProjectLock,
            ),
            (
                vec![("license_state".to_string(), "expired".to_string())],
                BlockingState::License,
            ),
            (
                vec![("blocking_dialog".to_string(), "yes".to_string())],
                BlockingState::ModalDialog,
            ),
            (
                vec![("failure_stage".to_string(), "sidecar_bootstrap".to_string())],
                BlockingState::AdapterBootstrap,
            ),
            (
                vec![("license_state".to_string(), "valid".to_string())],
                BlockingState::None,
            ),
        ];
        for (entries, expected) in cases {
            let metadata: HashMap<String, String> = entries.into_iter().collect();
            assert_eq!(classify_blocking_state(&metadata), expected);
        }
    }

    #[test]
    fn next_actions_are_non_interactive_and_name_the_blocking_state() {
        let action = BlockingState::ProjectLock.next_action(
            "unity",
            Some(Path::new("/work/MyProject")),
            Some("op-1"),
        );
        let command = action["command"].as_array().unwrap();

        assert_eq!(action["id"], "release_project_lock");
        assert!(command.iter().any(|flag| flag == "--non-interactive"));
        assert!(command.iter().any(|flag| flag == "start-instance"));
    }

    #[test]
    fn retryable_states_exclude_terminal_operator_decisions() {
        assert!(BlockingState::Timeout.retryable());
        assert!(BlockingState::ProjectLock.retryable());
        assert!(!BlockingState::License.retryable());
        assert!(!BlockingState::MissingExecutable.retryable());
        assert!(!BlockingState::None.retryable());
    }
}
