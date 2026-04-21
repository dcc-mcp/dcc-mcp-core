//! [`WorkflowHost`] — coordinator that owns a [`WorkflowExecutor`] plus a
//! registry of in-flight runs, and exposes **synchronous** start / status /
//! cancel entry points suitable for wiring into
//! [`dcc_mcp_actions::dispatcher::ActionDispatcher`] handlers or Python /
//! PyO3 call-sites.
//!
//! # Why a host?
//!
//! The executor itself is transport-agnostic and stateless across runs:
//! every call to [`WorkflowExecutor::run`] returns an independent
//! [`WorkflowRunHandle`]. The MCP tools `workflows.run` /
//! `workflows.get_status` / `workflows.cancel` need a *shared* registry
//! that can be looked up by `workflow_id` across later tool calls. That
//! registry is what [`WorkflowHost`] provides.
//!
//! # Runtime requirements
//!
//! `WorkflowHost::start_sync` requires a running Tokio runtime (it calls
//! [`tokio::spawn`] internally via [`WorkflowExecutor::run`]). Inside
//! `ActionDispatcher::dispatch`, handlers are invoked on whichever thread
//! called `dispatch`; the HTTP server runs them on its Tokio runtime, so
//! the spawn succeeds. Outside a Tokio runtime (pure unit tests), use
//! [`WorkflowHost::start_with_handle`] with an explicit handle.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::runtime::Handle;
use uuid::Uuid;

use crate::executor::{WorkflowExecutor, WorkflowRunHandle};
use crate::spec::{WorkflowSpec, WorkflowStatus};

/// Snapshot of a single recorded run.
#[derive(Debug, Clone)]
pub struct RunSnapshot {
    /// Runtime workflow id.
    pub workflow_id: Uuid,
    /// Root job id for the outer workflow.
    pub root_job_id: Uuid,
    /// Last-known terminal status, if the run has completed.
    pub terminal_status: Option<WorkflowStatus>,
}

/// Internal record for a tracked run.
#[derive(Debug)]
struct RunRecord {
    workflow_id: Uuid,
    root_job_id: Uuid,
    cancel_token: tokio_util::sync::CancellationToken,
    terminal: Arc<Mutex<Option<WorkflowStatus>>>,
}

/// Shared registry of active workflow runs coordinated by [`WorkflowHost`].
#[derive(Debug, Default, Clone)]
pub struct WorkflowRegistry {
    runs: Arc<Mutex<HashMap<Uuid, Arc<RunRecord>>>>,
}

impl WorkflowRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn insert(&self, record: Arc<RunRecord>) {
        self.runs.lock().insert(record.workflow_id, record);
    }

    fn get(&self, id: &Uuid) -> Option<Arc<RunRecord>> {
        self.runs.lock().get(id).cloned()
    }

    /// Number of tracked runs (running + terminal).
    pub fn len(&self) -> usize {
        self.runs.lock().len()
    }

    /// Whether no runs have been tracked.
    pub fn is_empty(&self) -> bool {
        self.runs.lock().is_empty()
    }

    /// Snapshot every tracked run's status.
    pub fn snapshots(&self) -> Vec<RunSnapshot> {
        self.runs
            .lock()
            .values()
            .map(|r| RunSnapshot {
                workflow_id: r.workflow_id,
                root_job_id: r.root_job_id,
                terminal_status: *r.terminal.lock(),
            })
            .collect()
    }
}

/// Coordinator that owns a [`WorkflowExecutor`] plus a [`WorkflowRegistry`].
///
/// Cheap to clone — the underlying executor and registry are `Arc`-wrapped.
#[derive(Debug, Clone)]
pub struct WorkflowHost {
    executor: Arc<WorkflowExecutor>,
    registry: WorkflowRegistry,
}

impl WorkflowHost {
    /// Construct a host around an existing executor.
    #[must_use]
    pub fn new(executor: WorkflowExecutor) -> Self {
        Self {
            executor: Arc::new(executor),
            registry: WorkflowRegistry::new(),
        }
    }

    /// Access the underlying executor.
    #[must_use]
    pub fn executor(&self) -> Arc<WorkflowExecutor> {
        Arc::clone(&self.executor)
    }

    /// Access the run registry.
    #[must_use]
    pub fn registry(&self) -> WorkflowRegistry {
        self.registry.clone()
    }

    /// Kick off a workflow on the **ambient** Tokio runtime.
    ///
    /// Returns `(workflow_id, root_job_id)`. The caller can poll status
    /// via [`Self::status`] or cancel via [`Self::cancel`].
    pub fn start(
        &self,
        spec: WorkflowSpec,
        inputs: Value,
        parent_job_id: Option<Uuid>,
    ) -> Result<(Uuid, Uuid), crate::error::WorkflowError> {
        let handle = self.executor.run(spec, inputs, parent_job_id)?;
        Ok(self.track(handle))
    }

    /// Kick off a workflow on a specific Tokio runtime handle. Useful when
    /// the caller is not itself on a runtime worker.
    pub fn start_with_handle(
        &self,
        rt: &Handle,
        spec: WorkflowSpec,
        inputs: Value,
        parent_job_id: Option<Uuid>,
    ) -> Result<(Uuid, Uuid), crate::error::WorkflowError> {
        let executor = Arc::clone(&self.executor);
        let _guard = rt.enter();
        let handle = executor.run(spec, inputs, parent_job_id)?;
        Ok(self.track(handle))
    }

    fn track(&self, handle: WorkflowRunHandle) -> (Uuid, Uuid) {
        let WorkflowRunHandle {
            workflow_id,
            root_job_id,
            cancel_token,
            join,
        } = handle;
        let terminal: Arc<Mutex<Option<WorkflowStatus>>> = Arc::new(Mutex::new(None));
        let terminal_clone = Arc::clone(&terminal);
        // The original handle's JoinHandle is moved into a background task
        // so callers don't have to .await it. When the task resolves we
        // record the final status for later `get_status` calls.
        tokio::spawn(async move {
            let status = join.await.unwrap_or(WorkflowStatus::Failed);
            *terminal_clone.lock() = Some(status);
        });
        let record = Arc::new(RunRecord {
            workflow_id,
            root_job_id,
            cancel_token,
            terminal,
        });
        self.registry.insert(Arc::clone(&record));
        (workflow_id, root_job_id)
    }

    /// Cancel a tracked workflow. Returns `false` if the id is unknown.
    pub fn cancel(&self, workflow_id: Uuid) -> bool {
        if let Some(rec) = self.registry.get(&workflow_id) {
            rec.cancel_token.cancel();
            true
        } else {
            false
        }
    }

    /// Fetch the current snapshot for `workflow_id`.
    pub fn status(&self, workflow_id: Uuid) -> Option<RunSnapshot> {
        self.registry.get(&workflow_id).map(|r| RunSnapshot {
            workflow_id: r.workflow_id,
            root_job_id: r.root_job_id,
            terminal_status: *r.terminal.lock(),
        })
    }
}

// ── Structured handler outputs ───────────────────────────────────────────

/// Handler for the `workflows.run` MCP tool.
///
/// Accepts `{ spec: <WorkflowSpec YAML string>, inputs?: <object> }` or
/// `{ spec: <WorkflowSpec JSON object>, inputs?: <object> }` and returns
/// `{ workflow_id, root_job_id, status: "pending" }` on success.
pub fn run_handler(host: &WorkflowHost, args: Value) -> Result<Value, String> {
    let spec = parse_spec_arg(&args)?;
    let inputs = args.get("inputs").cloned().unwrap_or_else(|| json!({}));
    let parent = args
        .get("parent_job_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok());
    let (workflow_id, root_job_id) = host
        .start(spec, inputs, parent)
        .map_err(|e| format!("workflow start failed: {e}"))?;
    Ok(json!({
        "workflow_id": workflow_id.to_string(),
        "root_job_id": root_job_id.to_string(),
        "status": WorkflowStatus::Pending.as_str(),
    }))
}

/// Handler for the `workflows.get_status` MCP tool.
pub fn get_status_handler(host: &WorkflowHost, args: Value) -> Result<Value, String> {
    let id = parse_workflow_id(&args)?;
    match host.status(id) {
        None => Err(format!("no workflow run tracked for id={id}")),
        Some(snap) => {
            let status = snap
                .terminal_status
                .map(|s| s.as_str())
                .unwrap_or_else(|| WorkflowStatus::Running.as_str());
            Ok(json!({
                "workflow_id": snap.workflow_id.to_string(),
                "root_job_id": snap.root_job_id.to_string(),
                "status": status,
                "terminal": snap.terminal_status.is_some(),
            }))
        }
    }
}

/// Handler for the `workflows.cancel` MCP tool.
pub fn cancel_handler(host: &WorkflowHost, args: Value) -> Result<Value, String> {
    let id = parse_workflow_id(&args)?;
    let cancelled = host.cancel(id);
    Ok(json!({
        "workflow_id": id.to_string(),
        "cancelled": cancelled,
    }))
}

fn parse_spec_arg(args: &Value) -> Result<WorkflowSpec, String> {
    let spec_field = args
        .get("spec")
        .ok_or_else(|| "missing required field: spec".to_string())?;
    match spec_field {
        Value::String(s) => WorkflowSpec::from_yaml(s).map_err(|e| e.to_string()),
        Value::Object(_) => {
            let yaml = serde_yaml_ng::to_string(spec_field).map_err(|e| e.to_string())?;
            WorkflowSpec::from_yaml(&yaml).map_err(|e| e.to_string())
        }
        other => Err(format!(
            "spec must be a YAML string or JSON object, got {}",
            type_name(other)
        )),
    }
}

fn parse_workflow_id(args: &Value) -> Result<Uuid, String> {
    let raw = args
        .get("workflow_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing required string field: workflow_id".to_string())?;
    Uuid::parse_str(raw).map_err(|e| format!("invalid workflow_id: {e}"))
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callers::test_support::MockToolCaller;
    use crate::executor::WorkflowExecutor;

    fn host_with_echo() -> WorkflowHost {
        let caller = Arc::new(MockToolCaller::new());
        caller.add("scene.echo", Ok);
        let executor = WorkflowExecutor::builder().tool_caller(caller).build();
        WorkflowHost::new(executor)
    }

    const YAML: &str = r#"
name: host-test
description: ""
inputs: {}
steps:
  - id: s1
    tool: scene.echo
    args:
      hello: world
"#;

    #[tokio::test]
    async fn start_tracks_run_and_status_transitions_to_terminal() {
        let host = host_with_echo();
        let out = run_handler(
            &host,
            json!({
                "spec": YAML,
                "inputs": {},
            }),
        )
        .unwrap();

        let wid = out["workflow_id"].as_str().unwrap().to_string();
        assert_eq!(out["status"], "pending");
        assert_eq!(host.registry().len(), 1);

        // Drive the spawned task to completion.
        for _ in 0..20 {
            let status = get_status_handler(&host, json!({"workflow_id": wid})).unwrap();
            if status["terminal"].as_bool() == Some(true) {
                assert_eq!(status["status"], "completed");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("workflow never reached terminal state");
    }

    #[tokio::test]
    async fn cancel_handler_flips_token() {
        let host = host_with_echo();
        let out = run_handler(
            &host,
            json!({
                "spec": YAML,
                "inputs": {},
            }),
        )
        .unwrap();
        let wid = out["workflow_id"].as_str().unwrap();
        let cancelled = cancel_handler(&host, json!({"workflow_id": wid})).unwrap();
        assert_eq!(cancelled["cancelled"], true);
    }

    #[tokio::test]
    async fn cancel_handler_unknown_id_returns_false() {
        let host = host_with_echo();
        let fake = Uuid::new_v4().to_string();
        let out = cancel_handler(&host, json!({"workflow_id": fake})).unwrap();
        assert_eq!(out["cancelled"], false);
    }

    #[test]
    fn run_handler_rejects_missing_spec() {
        let host = host_with_echo();
        let err = run_handler(&host, json!({"inputs": {}})).unwrap_err();
        assert!(err.contains("spec"));
    }

    #[test]
    fn get_status_rejects_invalid_uuid() {
        let host = host_with_echo();
        let err = get_status_handler(&host, json!({"workflow_id": "not-a-uuid"})).unwrap_err();
        assert!(err.contains("invalid workflow_id"));
    }

    #[tokio::test]
    async fn run_handler_accepts_json_object_spec() {
        let host = host_with_echo();
        let spec_obj = json!({
            "name": "json-spec",
            "description": "",
            "inputs": {},
            "steps": [
                {"id": "s1", "tool": "scene.echo", "args": {"k": "v"}},
            ],
        });
        let out = run_handler(&host, json!({"spec": spec_obj, "inputs": {}})).unwrap();
        assert_eq!(out["status"], "pending");
    }
}
