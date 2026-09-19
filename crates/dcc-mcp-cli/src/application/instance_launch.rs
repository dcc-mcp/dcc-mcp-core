//! Project-bound DCC launch and wait-ready workflow.
//!
//! `dcc-mcp-cli start-instance` closes the zero-instance gap: with no live
//! instance for a DCC type it launches the recorded host bound to an exact
//! project, waits for registry registration, and — with `--wait-ready` — waits
//! for terminal readiness before returning a deterministic instance selection.
//!
//! Contract boundaries enforced here:
//!
//! * launching a GUI process requires explicit operator authorization;
//! * nothing is downloaded, installed, upgraded, or reconfigured;
//! * process creation is never reported as readiness;
//! * one operation ID spans launch, registration, bootstrap, and readiness;
//! * an instance bound to another project is reported, never reused;
//! * a second identical request converges instead of launching a second host.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use dcc_mcp_transport::discovery::types::ServiceEntry;
use serde_json::{Value, json};

use crate::application::local_control;
use crate::application::local_instance;
use crate::domain::rest::WaitReadyRequest;
use crate::domain::start_instance::{
    BlockingState, DCC_TYPE_ENV, LIFECYCLE_OPERATION_SCHEMA_VERSION, LaunchPlan, LaunchStage,
    LifecycleOperation, OPERATION_ID_ENV, OPERATION_METADATA_KEYS, PROJECT_ENV,
    PROJECT_METADATA_KEYS, START_INSTANCE_REPORT_SCHEMA_VERSION, StartInstanceRequest,
    classify_blocking_state,
};

pub mod plan;
mod store;

pub use plan::{PlanResolution, missing_project_marker};
pub use store::LifecycleStore;
use store::binding_key;

/// Readiness/host-progress metadata keys surfaced verbatim in diagnostics. Core
/// does not interpret them; the adapter owns their meaning.
const HOST_PROGRESS_METADATA_KEYS: &[&str] = &[
    "compiling",
    "importing",
    "refreshing",
    "updating",
    "play_mode",
    "domain_reload",
    "progress_stage",
    "progress_phase",
    "progress_message",
];
/// Metadata keys that may carry a native window handle when the adapter has one.
const WINDOW_HANDLE_METADATA_KEYS: &[&str] = &[
    "window_handle",
    "native_window_handle",
    "hwnd",
    "dcc_mcp_window_handle",
    "dcc_mcp.window_handle",
];

/// Run one project-bound start-and-wait operation and return its terminal report.
///
/// Expected failures (missing plan, authorization, timeout, ambiguity) return a
/// structured report with `ok: false` instead of an error, so callers keep a
/// machine-readable next action. Only unexpected I/O failures propagate as
/// `Err`.
pub async fn start_instance(request: StartInstanceRequest) -> anyhow::Result<Value> {
    let started = Instant::now();
    let dcc_type = request.dcc_type.trim().to_ascii_lowercase();
    let requested_project = request.project.clone();
    let store = LifecycleStore::new(&request.registry_dir);
    let operation_id = uuid::Uuid::new_v4().to_string();
    let report = ReportContext {
        dcc_type: &dcc_type,
        requested_project: &requested_project,
        project: None,
        operation_id: &operation_id,
        registry_dir: &request.registry_dir,
        store: &store,
        started,
        dry_run: request.dry_run,
        authorized: request.authorized,
    };

    // ── Stage: project binding ──────────────────────────────────────────────
    let Some(project) = canonical_dir(&request.project) else {
        return Ok(report.failure(
            LaunchStage::ProjectBinding,
            BlockingState::ProjectNotFound,
            format!(
                "project path does not exist or is not a directory: {}",
                request.project.display()
            ),
            json!({}),
        ));
    };
    let report = report.with_project(&project);

    // ── Stage: resolve the adapter launch plan ──────────────────────────────
    let plan = match plan::resolve(&request, &project) {
        PlanResolution::Resolved(plan) => plan,
        PlanResolution::Missing => {
            let searched = plan::candidate_paths(&request, &project);
            return Ok(report.failure(
                LaunchStage::ResolvePlan,
                BlockingState::LaunchPlanMissing,
                format!(
                    "no launch plan published for '{dcc_type}' and project {}; searched {}",
                    project.display(),
                    searched
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                json!({ "searched": searched }),
            ));
        }
        PlanResolution::Invalid { plan_path, error } => {
            return Ok(report.failure(
                LaunchStage::ResolvePlan,
                error.blocking_state(),
                error.message(),
                json!({ "launch_plan_path": plan_path }),
            ));
        }
    };

    if let Some(marker) = missing_project_marker(&plan, &project) {
        return Ok(report.failure(
            LaunchStage::ProjectBinding,
            BlockingState::ProjectMarkerMissing,
            format!(
                "project {} is missing the adapter binding marker '{marker}'",
                project.display()
            ),
            json!({ "missing_marker": marker }),
        ));
    }

    let mut operation = LifecycleOperation::new(
        &operation_id,
        &dcc_type,
        &project,
        &plan.executable,
        plan.version.clone(),
    );

    // ── Stage: dry run ──────────────────────────────────────────────────────
    if request.dry_run {
        return Ok(report.dry_run(&plan));
    }

    // ── Stage: authorization ────────────────────────────────────────────────
    if !request.authorized {
        return Ok(report.failure(
            LaunchStage::Authorization,
            BlockingState::AuthorizationRequired,
            format!(
                "launching {}{} requires explicit operator authorization; re-run with --yes",
                plan.executable.display(),
                plan.version
                    .as_deref()
                    .map(|version| format!(" ({version})"))
                    .unwrap_or_default()
            ),
            json!({ "launch_plan": plan.summary() }),
        ));
    }

    // ── Stage: reuse / convergence ──────────────────────────────────────────
    match find_reusable(
        &request.registry_dir,
        &dcc_type,
        &project,
        &store,
        request.instance_id.as_deref(),
    )? {
        Reuse::Ambiguous(candidates) => {
            return Ok(report.failure(
                LaunchStage::Reuse,
                BlockingState::AmbiguousReuse,
                format!(
                    "several live {dcc_type} instances claim project {}; refusing to choose",
                    project.display()
                ),
                json!({ "candidates": candidates, "launch_plan": plan.summary() }),
            ));
        }
        Reuse::Instance {
            entry,
            owning_operation,
        } => {
            let converged = owning_operation
                .as_ref()
                .map(|owner| owner.operation_id.clone())
                .filter(|id| id != &operation_id);
            operation.launched = false;
            operation.owned = owning_operation.as_ref().is_some_and(|owner| owner.owned);
            operation.pid = entry.pid.or(entry.host_pid);
            operation.instance_id = Some(entry.instance_id.to_string());
            operation.touch();
            // Classify before persisting: `blocking_state` is part of the
            // record, so assigning it after `store.save` would leave the
            // on-disk operation without it.
            let blocking = classify_blocking_state(&entry.metadata);
            operation.blocking_state = Some(blocking.as_str().to_string());
            store.save(&operation)?;

            let ready =
                blocking == BlockingState::None && local_instance::direct_control_ready(&entry);

            let mut value =
                report.terminal(&entry, true, blocking, ready, &plan, &operation, 1, None);
            value["converged_on_operation_id"] =
                converged.map(Value::String).unwrap_or(Value::Null);
            return Ok(value);
        }
        Reuse::None => {}
    }

    // ── Stage: launch ───────────────────────────────────────────────────────
    let child = match spawn_host(&plan, &dcc_type, &project, &operation_id) {
        Ok(child) => child,
        Err(error) => {
            return Ok(report.failure(
                LaunchStage::Launch,
                BlockingState::LaunchFailed,
                format!("spawning {} failed: {error}", plan.executable.display()),
                json!({
                    "launch_plan": plan.summary(),
                    "os_error": error.to_string(),
                }),
            ));
        }
    };
    let pid = child.id();
    drop(child);

    operation.launched = true;
    operation.owned = true;
    operation.pid = Some(pid);
    operation.touch();
    store.save(&operation)?;

    // ── Stage: registration ─────────────────────────────────────────────────
    let registration = wait_for_registration(
        &request.registry_dir,
        &dcc_type,
        &project,
        &operation_id,
        Some(pid),
        request.timeout,
        request.interval,
    )
    .await?;

    let Some(entry) = registration.entry else {
        operation.blocking_state = Some(BlockingState::Timeout.as_str().to_string());
        operation.touch();
        store.save(&operation)?;
        let mut value = report.failure(
            LaunchStage::Registration,
            BlockingState::Timeout,
            format!(
                "{dcc_type} was launched (pid {pid}) but did not register within {:?}",
                request.timeout
            ),
            json!({
                "launch_plan": plan.summary(),
                "pid": pid,
                "attempts": registration.attempts,
                "unbound_instances": registration.unbound_instances,
                "other_project_instances": registration.other_project_instances,
            }),
        );
        // The host is running even though readiness was never reached, so the
        // operator needs the pid to inspect or stop it.
        value["pid"] = Value::from(pid);
        value["launched"] = Value::Bool(true);
        value["owned"] = Value::Bool(true);
        return Ok(value);
    };

    operation.instance_id = Some(entry.instance_id.to_string());
    operation.touch();
    store.save(&operation)?;

    // ── Stage: readiness ────────────────────────────────────────────────────
    let mut readiness = None;
    if request.wait_ready {
        let remaining = request
            .timeout
            .saturating_sub(started.elapsed())
            .max(Duration::from_secs(1));
        readiness = Some(
            local_control::wait_ready_local(
                request.registry_dir.clone(),
                WaitReadyRequest {
                    dcc_type: Some(dcc_type.clone()),
                    instance_id: Some(entry.instance_id.to_string()),
                    required: request.required.clone(),
                    timeout: remaining,
                    interval: request.interval,
                },
            )
            .await?,
        );
    }

    let ready = match readiness.as_ref() {
        Some(readiness) => readiness
            .get("ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        None => true,
    };
    let blocking = if !ready {
        match classify_blocking_state(&entry.metadata) {
            BlockingState::None => BlockingState::Timeout,
            classified => classified,
        }
    } else if request.wait_ready {
        BlockingState::None
    } else {
        classify_blocking_state(&entry.metadata)
    };
    // Without `--wait-ready` the operation still must not claim readiness it
    // has not observed: registration alone is not adapter readiness.
    let ready = if request.wait_ready {
        ready
    } else {
        ready && blocking == BlockingState::None && local_instance::direct_control_ready(&entry)
    };

    operation.blocking_state = Some(blocking.as_str().to_string());
    operation.touch();
    store.save(&operation)?;

    Ok(report.terminal(
        &entry,
        false,
        blocking,
        ready,
        &plan,
        &operation,
        registration.attempts,
        readiness.as_ref(),
    ))
}

/// Resolve the operation an operator wants to stop, guarded by lifecycle
/// ownership. Returns the stored operation when `operation_id` is supplied.
pub fn resolve_owned_operation(
    registry_dir: &Path,
    dcc_type: Option<&str>,
    instance_id: Option<&str>,
    operation_id: Option<&str>,
) -> anyhow::Result<Option<LifecycleOperation>> {
    let Some(operation_id) = operation_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let store = LifecycleStore::new(registry_dir);
    let Some(operation) = store.load(operation_id)? else {
        anyhow::bail!("no lifecycle operation '{operation_id}' is recorded locally");
    };
    if !operation.owned {
        anyhow::bail!(
            "operation '{operation_id}' did not launch its own process; refusing to stop an instance it does not own"
        );
    }
    // A stop may only target an instance the operation actually registered. An
    // owned operation is persisted before registration assigns an instance id,
    // so an absent or blank id means there is nothing to stop; degrade here and
    // the caller would route to an empty instance segment.
    match operation.instance_id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => {}
        _ => anyhow::bail!(
            "operation '{operation_id}' has not registered an instance yet; refusing to stop an unknown instance"
        ),
    }
    if let Some(dcc_type) = dcc_type
        && !operation.dcc_type.eq_ignore_ascii_case(dcc_type)
    {
        anyhow::bail!(
            "operation '{operation_id}' owns a {} instance, not '{dcc_type}'",
            operation.dcc_type
        );
    }
    if let Some(instance_id) = instance_id
        && operation.instance_id.as_deref() != Some(instance_id)
        && !instance_matches_operation(instance_id, &operation)
    {
        anyhow::bail!(
            "operation '{operation_id}' does not own instance '{instance_id}' (owned: {:?})",
            operation.instance_id
        );
    }
    Ok(Some(operation))
}

fn instance_matches_operation(instance_id: &str, operation: &LifecycleOperation) -> bool {
    let Some(owned) = operation.instance_id.as_deref() else {
        return false;
    };
    let instance_id = instance_id.to_ascii_lowercase();
    let owned = owned.to_ascii_lowercase();
    owned.starts_with(&instance_id) || instance_id.starts_with(&owned)
}

/// Owned-operation lookup used when the caller only knows dcc_type + project.
pub fn owned_operation_for(
    registry_dir: &Path,
    dcc_type: &str,
    project: &Path,
) -> anyhow::Result<Option<LifecycleOperation>> {
    LifecycleStore::new(registry_dir).find_owned(dcc_type, project)
}

// ── Reuse / convergence ────────────────────────────────────────────────────

#[allow(clippy::large_enum_variant)]
enum Reuse {
    None,
    Instance {
        entry: ServiceEntry,
        owning_operation: Option<LifecycleOperation>,
    },
    Ambiguous(Vec<String>),
}

/// `instance_hint` is the caller's `--instance-id`: it narrows convergence to
/// one exact instance instead of letting several project matches stay
/// ambiguous.
fn find_reusable(
    registry_dir: &Path,
    dcc_type: &str,
    project: &Path,
    store: &LifecycleStore,
    instance_hint: Option<&str>,
) -> anyhow::Result<Reuse> {
    let entries = local_instance::select_entries(registry_dir, Some(dcc_type), instance_hint)?;
    if entries.is_empty() {
        return Ok(Reuse::None);
    }
    let owning_operation = store.find_owned(dcc_type, project)?;
    let owned_id = owning_operation
        .as_ref()
        .map(|operation| operation.operation_id.as_str());
    let owned_pid = owning_operation
        .as_ref()
        .and_then(|operation| operation.pid);

    let matched: Vec<ServiceEntry> = entries
        .into_iter()
        .filter(|entry| {
            entry_metadata(entry, OPERATION_METADATA_KEYS).is_some_and(|id| Some(id) == owned_id)
                || owned_pid
                    .is_some_and(|pid| entry.pid == Some(pid) || entry.host_pid == Some(pid))
                || entry_project(entry).is_some_and(|bound| same_path(&bound, project))
        })
        .collect();
    if matched.is_empty() {
        return Ok(Reuse::None);
    }

    let routable: Vec<ServiceEntry> = matched
        .iter()
        .filter(|entry| local_instance::direct_control_ready(entry))
        .cloned()
        .collect();
    let mut pool = routable;
    if pool.is_empty() {
        pool = matched;
    }
    if pool.len() > 1 {
        return Ok(Reuse::Ambiguous(
            pool.iter()
                .map(|entry| {
                    format!(
                        "{}:{}",
                        entry.dcc_type,
                        local_instance::instance_short(entry)
                    )
                })
                .collect(),
        ));
    }
    let entry = pool.remove(0);
    Ok(Reuse::Instance {
        entry,
        owning_operation,
    })
}

// ── Registration wait ──────────────────────────────────────────────────────

struct Registration {
    entry: Option<ServiceEntry>,
    attempts: u64,
    unbound_instances: Vec<Value>,
    other_project_instances: Vec<Value>,
}

async fn wait_for_registration(
    registry_dir: &Path,
    dcc_type: &str,
    project: &Path,
    operation_id: &str,
    pid: Option<u32>,
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<Registration> {
    let started = Instant::now();
    let interval = interval.max(Duration::from_millis(50));
    let mut attempts = 0_u64;
    loop {
        attempts += 1;
        let entries = local_instance::select_entries(registry_dir, Some(dcc_type), None)?;
        let (unbound, others) = classify_neighbours(&entries, project);
        let entry = select_registration(&entries, project, operation_id, pid);
        let last = Registration {
            entry: entry.clone(),
            attempts,
            unbound_instances: unbound,
            other_project_instances: others,
        };
        if entry.is_some() || started.elapsed() >= timeout {
            return Ok(last);
        }
        tokio::time::sleep(interval).await;
    }
}

/// Prefer the operation ID, then the launched PID, then the project binding.
/// Operation ID wins because PIDs are reused after a crash.
fn select_registration(
    entries: &[ServiceEntry],
    project: &Path,
    operation_id: &str,
    pid: Option<u32>,
) -> Option<ServiceEntry> {
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry_metadata(entry, OPERATION_METADATA_KEYS) == Some(operation_id))
    {
        return Some(entry.clone());
    }
    if let Some(entry) = pid.and_then(|pid| {
        entries
            .iter()
            .find(|entry| entry.pid == Some(pid) || entry.host_pid == Some(pid))
    }) {
        return Some(entry.clone());
    }
    let bound: Vec<&ServiceEntry> = entries
        .iter()
        .filter(|entry| entry_project(entry).is_some_and(|bound| same_path(&bound, project)))
        .collect();
    match bound.as_slice() {
        [] => None,
        [entry] => Some((*entry).clone()),
        many => many
            .iter()
            .find(|entry| local_instance::direct_control_ready(entry))
            .or_else(|| many.first())
            .map(|entry| (*entry).clone()),
    }
}

/// Split same-DCC neighbours into instances Core cannot bind to a project and
/// instances explicitly bound to a different project. Neither is ever reused.
fn classify_neighbours(entries: &[ServiceEntry], project: &Path) -> (Vec<Value>, Vec<Value>) {
    let mut unbound = Vec::new();
    let mut others = Vec::new();
    for entry in entries {
        match entry_project(entry) {
            None => unbound.push(neighbour(entry, None)),
            Some(bound) if !same_path(&bound, project) => {
                others.push(neighbour(entry, Some(bound.display().to_string())))
            }
            Some(_) => {}
        }
    }
    (unbound, others)
}

fn neighbour(entry: &ServiceEntry, project: Option<String>) -> Value {
    json!({
        "dcc_type": entry.dcc_type,
        "instance_id": entry.instance_id.to_string(),
        "instance_short": local_instance::instance_short(entry),
        "pid": entry.pid,
        "host_pid": entry.host_pid,
        "project": project,
        "direct_control_ready": local_instance::direct_control_ready(entry),
    })
}

// ── Process launch ─────────────────────────────────────────────────────────

fn spawn_host(
    plan: &LaunchPlan,
    dcc_type: &str,
    project: &Path,
    operation_id: &str,
) -> std::io::Result<std::process::Child> {
    let mut command = Command::new(&plan.argv[0]);
    command.args(plan.argv.iter().skip(1));
    let cwd = plan
        .cwd
        .as_deref()
        .filter(|path| path.is_dir())
        .unwrap_or(project);
    command.current_dir(cwd);
    // One operation ID spans launch → registration → bootstrap → readiness, so
    // the adapter can stamp it into its registry row.
    command.env(OPERATION_ID_ENV, operation_id);
    command.env(PROJECT_ENV, project.display().to_string());
    command.env(DCC_TYPE_ENV, dcc_type);
    // Detached GUI host: inheriting the CLI's pipes would make the command
    // appear to hang for the lifetime of the DCC.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn()
}

// ── Report assembly ────────────────────────────────────────────────────────

struct ReportContext<'a> {
    dcc_type: &'a str,
    requested_project: &'a Path,
    project: Option<&'a Path>,
    operation_id: &'a str,
    registry_dir: &'a Path,
    store: &'a LifecycleStore,
    started: Instant,
    dry_run: bool,
    authorized: bool,
}

impl<'a> ReportContext<'a> {
    fn with_project(self, project: &'a Path) -> Self {
        Self {
            project: Some(project),
            ..self
        }
    }

    fn project(&self) -> &Path {
        self.project.unwrap_or(self.requested_project)
    }

    fn base(&self) -> Value {
        json!({
            "schema_version": START_INSTANCE_REPORT_SCHEMA_VERSION,
            "operation_id": self.operation_id,
            "dcc_type": self.dcc_type,
            "project": self.requested_project.display().to_string(),
            "project_canonical": self.project().display().to_string(),
            "elapsed_ms": self.started.elapsed().as_millis() as u64,
            "dry_run": self.dry_run,
            "authorized": self.authorized,
            "registry_dir": self.registry_dir,
            "lifecycle_dir": self.store.dir(),
            "source": "local_start_instance",
        })
    }

    fn failure(
        &self,
        stage: LaunchStage,
        blocking: BlockingState,
        message: String,
        diagnostics: Value,
    ) -> Value {
        let mut value = self.base();
        value["ok"] = Value::Bool(false);
        value["ready"] = Value::Bool(false);
        value["launched"] = Value::Bool(false);
        value["reused"] = Value::Bool(false);
        value["owned"] = Value::Bool(false);
        value["stage"] = Value::String(stage.as_str().to_string());
        value["timeout_stage"] = Value::String(stage.as_str().to_string());
        value["blocking_state"] = Value::String(blocking.as_str().to_string());
        value["retryable"] = Value::Bool(blocking.retryable());
        value["message"] = Value::String(message);
        value["instance"] = Value::Null;
        value["instance_id"] = Value::Null;
        value["instance_short"] = Value::Null;
        value["mcp_url"] = Value::Null;
        value["dispatch_state"] = Value::Null;
        value["converged_on_operation_id"] = Value::Null;
        value["diagnostics"] = diagnostics;
        value["next_action"] =
            blocking.next_action(self.dcc_type, Some(self.project()), Some(self.operation_id));
        value
    }

    fn dry_run(&self, plan: &LaunchPlan) -> Value {
        let mut value = self.base();
        value["ok"] = Value::Bool(true);
        value["ready"] = Value::Bool(false);
        value["launched"] = Value::Bool(false);
        value["reused"] = Value::Bool(false);
        value["stage"] = Value::String(LaunchStage::ResolvePlan.as_str().to_string());
        value["timeout_stage"] = Value::Null;
        value["blocking_state"] = Value::String(BlockingState::None.as_str().to_string());
        value["would_launch"] = Value::Bool(true);
        value["launch_plan"] = plan.summary();
        value["message"] = Value::String(format!(
            "dry run resolved a launch plan for '{}'; re-run with --yes to launch",
            self.dcc_type
        ));
        value["next_action"] = BlockingState::AuthorizationRequired.next_action(
            self.dcc_type,
            Some(self.project()),
            Some(self.operation_id),
        );
        value
    }

    #[allow(clippy::too_many_arguments)]
    fn terminal(
        &self,
        entry: &ServiceEntry,
        reused: bool,
        blocking: BlockingState,
        ready: bool,
        plan: &LaunchPlan,
        operation: &LifecycleOperation,
        attempts: u64,
        readiness: Option<&Value>,
    ) -> Value {
        let instance = local_instance::instance_to_value(entry.clone()).unwrap_or_else(|_| {
            json!({
                "instance_id": entry.instance_id.to_string(),
                "dcc_type": entry.dcc_type,
            })
        });
        let dispatch_state = instance
            .pointer("/direct_control/dispatch_status")
            .cloned()
            .unwrap_or(Value::Null);
        let diagnostics = instance
            .pointer("/direct_control/diagnostics")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let window_handle = entry_metadata(entry, WINDOW_HANDLE_METADATA_KEYS).map(str::to_string);
        let host_progress: Value = HOST_PROGRESS_METADATA_KEYS
            .iter()
            .filter_map(|key| {
                entry
                    .metadata
                    .get(*key)
                    .map(|value| ((*key).to_string(), Value::String(value.clone())))
            })
            .collect::<serde_json::Map<String, Value>>()
            .into();

        let mut value = self.base();
        value["ok"] = Value::Bool(ready);
        value["ready"] = Value::Bool(ready);
        value["launched"] = Value::Bool(operation.launched);
        value["reused"] = Value::Bool(reused);
        value["owned"] = Value::Bool(operation.owned);
        value["executable"] = Value::String(plan.executable.display().to_string());
        value["argv"] = Value::from(plan.argv.clone());
        value["version"] = plan
            .version
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null);
        value["launch_plan_source"] = Value::String(plan.source.as_str().to_string());
        value["launch_plan_path"] = plan
            .plan_path
            .clone()
            .map(|path| Value::String(path.display().to_string()))
            .unwrap_or(Value::Null);
        value["pid"] = operation.pid.map(Value::from).unwrap_or(Value::Null);
        value["window_handle"] = window_handle.map(Value::String).unwrap_or(Value::Null);
        value["instance"] = instance;
        value["instance_id"] = Value::String(entry.instance_id.to_string());
        value["instance_short"] = Value::String(local_instance::instance_short(entry));
        value["mcp_url"] = Value::String(local_instance::mcp_url(entry));
        value["readyz_url"] = Value::String(local_instance::readyz_url(entry));
        value["dispatch_state"] = dispatch_state;
        value["host_progress"] = host_progress;
        value["attempts"] = Value::from(attempts);
        value["diagnostics"] = diagnostics;
        if let Some(readiness) = readiness {
            value["readiness"] = readiness.get("readiness").cloned().unwrap_or(Value::Null);
            value["missing"] = readiness
                .get("missing")
                .cloned()
                .unwrap_or_else(|| json!([]));
            value["readiness_attempts"] = readiness
                .get("attempts")
                .and_then(Value::as_u64)
                .map(Value::from)
                .unwrap_or(Value::Null);
        } else {
            value["readiness"] = Value::Null;
            value["missing"] = json!([]);
        }
        if ready {
            value["stage"] = Value::String(LaunchStage::Terminal.as_str().to_string());
            value["timeout_stage"] = Value::Null;
        } else {
            let stage = if readiness.is_some() {
                LaunchStage::Readiness
            } else {
                LaunchStage::Registration
            };
            value["stage"] = Value::String(stage.as_str().to_string());
            value["timeout_stage"] = Value::String(stage.as_str().to_string());
        }
        value["blocking_state"] = Value::String(blocking.as_str().to_string());
        value["retryable"] = Value::Bool(blocking.retryable());
        value["next_action"] =
            blocking.next_action(self.dcc_type, Some(self.project()), Some(self.operation_id));
        value
    }
}

// ── Shared path / metadata helpers ─────────────────────────────────────────

fn canonical_dir(path: &Path) -> Option<PathBuf> {
    path.is_dir()
        .then(|| std::fs::canonicalize(path).ok())
        .flatten()
        .map(|canonical| readable_path(&canonical))
}

/// `std::fs::canonicalize` returns `\\?\`-prefixed paths on Windows. Reports
/// and suggested commands are operator-facing, so drop the prefix: the path
/// stays absolute and canonical, and `same_path` still matches it against a
/// freshly canonicalized one.
fn readable_path(path: &Path) -> PathBuf {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf())
}

pub fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
        || left
            .display()
            .to_string()
            .eq_ignore_ascii_case(&right.display().to_string())
}

pub fn entry_metadata<'a>(entry: &'a ServiceEntry, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        entry
            .metadata
            .get(*key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                entry
                    .extras
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
    })
}

pub fn entry_project(entry: &ServiceEntry) -> Option<PathBuf> {
    entry_metadata(entry, PROJECT_METADATA_KEYS).map(PathBuf::from)
}

/// Redacted operation summary reused by `stop-instance` reports.
pub fn operation_summary(operation: &LifecycleOperation) -> Value {
    json!({
        "schema_version": LIFECYCLE_OPERATION_SCHEMA_VERSION,
        "operation_id": operation.operation_id,
        "dcc_type": operation.dcc_type,
        "project": operation.project,
        "pid": operation.pid,
        "launched": operation.launched,
        "owned": operation.owned,
        "instance_id": operation.instance_id,
        "binding_key": binding_key(&operation.dcc_type, &operation.project),
        "created_at_unix": operation.created_at_unix,
        "updated_at_unix": operation.updated_at_unix,
    })
}

#[cfg(test)]
#[path = "instance_launch_tests.rs"]
mod tests;
