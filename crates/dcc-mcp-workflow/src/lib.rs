//! # dcc-mcp-workflow
//!
//! First-class Workflow primitive for the DCC-MCP ecosystem — a spec-driven,
//! persistable, cancellable pipeline of tool calls that runs as an async MCP
//! tool.
//!
//! **This crate is currently a skeleton.** It lands the type definitions, the
//! YAML parser, validation (step-id uniqueness, JSONPath well-formedness,
//! tool-name conformance), the four built-in MCP tools
//! (`workflows.run` / `workflows.get_status` / `workflows.cancel` /
//! `workflows.lookup`) and the `WorkflowCatalog` reader for the
//! `metadata.dcc-mcp.workflows` glob.
//!
//! **Step execution is intentionally not implemented yet** — the three
//! execution-facing tools return a well-formed error
//! (`step execution pending follow-up PR`). This shape is deliberate so
//! downstream issues (#349 / #351 / #353 / #354) can build against stable
//! signatures in parallel.
//!
//! See issue [#348](https://github.com/loonghao/dcc-mcp-core/issues/348).

#![deny(missing_docs)]

pub mod catalog;
pub mod error;
pub mod job;
pub mod policy;
#[cfg(feature = "python-bindings")]
pub mod python;
pub mod spec;
#[cfg(feature = "job-persist-sqlite")]
pub mod sqlite;
pub mod tools;

#[cfg(test)]
mod tests;

pub use catalog::{WorkflowCatalog, WorkflowSummary};
pub use error::{ValidationError, WorkflowError};
pub use job::{WorkflowJob, WorkflowProgress};
pub use policy::{
    BackoffKind, IdempotencyScope, RawRetryPolicy, RawStepPolicy, RetryPolicy, StepPolicy,
};
pub use spec::{Step, StepId, StepKind, WorkflowId, WorkflowSpec, WorkflowStatus};
pub use tools::register_builtin_workflow_tools;
