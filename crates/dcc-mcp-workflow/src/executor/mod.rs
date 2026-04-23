//! [`WorkflowExecutor`] — runs a [`WorkflowSpec`] end-to-end.
//!
//! This is the step execution engine for issue #348. It consumes the
//! skeleton types landed in the parent PR plus every subsequent workflow
//! improvement (`StepPolicy`, `FileRef`, `$/dcc.workflowUpdated`,
//! parent-job cascade, SQLite persistence) and implements all six
//! [`StepKind`] variants.
//!
//! # Shape
//!
//! ```text
//! WorkflowExecutor::run(spec, inputs, parent)
//!   → creates a root WorkflowRun (id, cancel_token)
//!   → spawns a Tokio task driving the top-level step sequence
//!   → returns a WorkflowRunHandle { workflow_id, root_job_id, cancel_token, join }
//! ```
//!
//! Each step kind has its own driver:
//!
//! | Kind           | Driver                                               |
//! |----------------|------------------------------------------------------|
//! | `Tool`         | [`WorkflowExecutor::run_tool_step`] via [`ToolCaller`] |
//! | `ToolRemote`   | [`WorkflowExecutor::run_remote_step`] via [`RemoteCaller`] |
//! | `Foreach`      | [`WorkflowExecutor::run_foreach`] (JSONPath items)   |
//! | `Parallel`     | [`WorkflowExecutor::run_parallel`] (all-or-abort)    |
//! | `Approve`      | [`WorkflowExecutor::run_approve`] (gate + timeout)   |
//! | `Branch`       | [`WorkflowExecutor::run_branch`] (JSONPath condition) |
//!
//! Each step honours `StepPolicy` (timeout + retry + idempotency).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_artefact::{ArtefactBody, FileRef, SharedArtefactStore};
use jsonpath_rust::JsonPath;
use serde_json::Value;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::approval::{ApprovalGate, ApprovalResponse};
use crate::callers::{
    ActionDispatcherCaller, NullRemoteCaller, SharedRemoteCaller, SharedToolCaller,
};
use crate::context::{StepOutput, WorkflowContext};
use crate::error::WorkflowError;
use crate::idempotency::IdempotencyCache;
use crate::notifier::{NullNotifier, SharedNotifier, WorkflowUpdate, WorkflowUpdateProgress};
use crate::policy::{BackoffKind, IdempotencyScope, RetryPolicy, StepPolicy};
use crate::spec::{Step, StepId, StepKind, WorkflowSpec, WorkflowStatus};

pub mod builder;
pub mod core;
pub mod handle;
pub mod helpers;
pub mod output;
pub mod policy_wrapper;
pub mod step_drivers;
#[cfg(test)]
pub mod tests;

pub use builder::WorkflowExecutorBuilder;
pub use handle::WorkflowRunHandle;
pub use helpers::*;
pub use output::*;
pub use policy_wrapper::*;

/// Shared state for an active workflow — cloned into every step driver.
#[derive(Clone)]
pub struct RunState {
    workflow_id: Uuid,
    root_job_id: Uuid,
    context: WorkflowContext,
    notifier: SharedNotifier,
    artefacts: Option<SharedArtefactStore>,
    tool_caller: SharedToolCaller,
    remote_caller: SharedRemoteCaller,
    idempotency: IdempotencyCache,
    approval_gate: ApprovalGate,
    cancel_token: CancellationToken,
    #[cfg(feature = "job-persist-sqlite")]
    storage: Option<Arc<crate::sqlite::WorkflowStorage>>,
    total_steps: u32,
    completed: Arc<parking_lot::Mutex<u32>>,
    /// Snapshot accumulator so the executor can expose `step_outputs` to
    /// the outer MCP tool at any time.
    outputs_snapshot: Arc<parking_lot::RwLock<HashMap<String, Value>>>,
}

impl RunState {
    fn progress(&self) -> WorkflowUpdateProgress {
        WorkflowUpdateProgress {
            completed_steps: *self.completed.lock(),
            total_steps: self.total_steps,
        }
    }

    fn emit(&self, status: WorkflowStatus, step: Option<&str>, detail: Value) {
        self.notifier.emit(WorkflowUpdate {
            workflow_id: self.workflow_id,
            job_id: self.root_job_id,
            status,
            current_step_id: step.map(str::to_string),
            progress: self.progress(),
            detail,
        });
    }

    fn inc_completed(&self) {
        *self.completed.lock() += 1;
    }

    fn record_output_snapshot(&self, step_id: &str, output: &Value) {
        self.outputs_snapshot
            .write()
            .insert(step_id.to_string(), output.clone());
    }

    /// Snapshot of every step's output for persistence. Clones the inner
    /// map.
    #[allow(dead_code)]
    fn outputs_json(&self) -> Value {
        let map = self.outputs_snapshot.read().clone();
        let mut out = serde_json::Map::new();
        for (k, v) in map {
            out.insert(k, v);
        }
        Value::Object(out)
    }
}

/// Top-level workflow step execution engine.
///
/// Cheap to clone — all state is `Arc`-wrapped.
#[derive(Clone)]
pub struct WorkflowExecutor {
    tool_caller: SharedToolCaller,
    remote_caller: SharedRemoteCaller,
    notifier: SharedNotifier,
    artefacts: Option<SharedArtefactStore>,
    idempotency: IdempotencyCache,
    approval_gate: ApprovalGate,
    #[cfg(feature = "job-persist-sqlite")]
    storage: Option<Arc<crate::sqlite::WorkflowStorage>>,
    /// Default approval timeout when a step declares no `timeout_secs`.
    /// `None` means indefinite (matches the issue #348 spec).
    default_approve_timeout: Option<Duration>,
}

impl std::fmt::Debug for WorkflowExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowExecutor")
            .field("has_artefacts", &self.artefacts.is_some())
            .field("idempotency_entries", &self.idempotency.len())
            .field("pending_approvals", &self.approval_gate.pending_count())
            .finish()
    }
}
