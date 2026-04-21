//! # dcc-mcp-workflow
//!
//! First-class Workflow primitive for the DCC-MCP ecosystem — a spec-driven,
//! persistable, cancellable pipeline of tool calls that runs as an async MCP
//! tool.
//!
//! # Crate map
//!
//! | Module | Role |
//! |--------|------|
//! | [`spec`]        | `WorkflowSpec`, `Step`, `StepKind`, status, YAML parser, validator |
//! | [`policy`]      | Per-step retry / timeout / idempotency policy types |
//! | [`context`]     | Shared execution context + `{{template}}` resolver |
//! | [`notifier`]    | `WorkflowNotifier` trait (`$/dcc.workflowUpdated` abstraction) |
//! | [`approval`]    | `Approve` step gate (`notifications/$/dcc.approveResponse`) |
//! | [`idempotency`] | In-process idempotency cache |
//! | [`callers`]     | `ToolCaller` / `RemoteCaller` abstractions + `ActionDispatcher` adapter |
//! | [`executor`]    | `WorkflowExecutor` — runs every `StepKind` variant |
//! | [`host`]        | `WorkflowHost` — shared executor + run registry coordinator |
//! | [`job`]         | `WorkflowJob` — aggregated progress snapshot |
//! | [`catalog`]     | `WorkflowCatalog` from skill metadata |
//! | [`sqlite`]      | SQLite persistence for workflow runs (gated behind `job-persist-sqlite`) |
//! | [`tools`]       | `workflows.*` built-in MCP tool metadata + handler wiring |
//!
//! See issue [#348](https://github.com/loonghao/dcc-mcp-core/issues/348).

#![deny(missing_docs)]

pub mod approval;
pub mod callers;
pub mod catalog;
pub mod context;
pub mod error;
pub mod executor;
pub mod host;
pub mod idempotency;
pub mod job;
pub mod notifier;
pub mod policy;
#[cfg(feature = "python-bindings")]
pub mod python;
pub mod spec;
#[cfg(feature = "job-persist-sqlite")]
pub mod sqlite;
pub mod tools;

#[cfg(test)]
mod tests;

pub use approval::{ApprovalGate, ApprovalResponse};
pub use callers::{
    ActionDispatcherCaller, NullRemoteCaller, RemoteCaller, SharedRemoteCaller, SharedToolCaller,
    ToolCaller,
};
pub use catalog::{WorkflowCatalog, WorkflowSummary};
pub use context::{StepOutput, TemplateError, WorkflowContext};
pub use error::{ValidationError, WorkflowError};
pub use executor::{WorkflowExecutor, WorkflowExecutorBuilder, WorkflowRunHandle};
pub use host::{
    RunSnapshot, WorkflowHost, WorkflowRegistry, cancel_handler, get_status_handler, run_handler,
};
pub use idempotency::IdempotencyCache;
pub use job::{WorkflowJob, WorkflowProgress};
pub use notifier::{
    NullNotifier, RecordingNotifier, SharedNotifier, WorkflowNotifier, WorkflowUpdate,
    WorkflowUpdateProgress,
};
pub use policy::{
    BackoffKind, IdempotencyScope, RawRetryPolicy, RawStepPolicy, RetryPolicy, StepPolicy,
};
pub use spec::{Step, StepId, StepKind, WorkflowId, WorkflowSpec, WorkflowStatus};
pub use tools::register_builtin_workflow_tools;
