//! # dcc-mcp-scheduler
//!
//! Scheduler subsystem for the DCC-MCP ecosystem — fires pre-registered
//! workflows on cron schedules or HTTP webhook events without a human
//! tool call.
//!
//! See issue [#352](https://github.com/loonghao/dcc-mcp-core/issues/352).
//!
//! ## Design summary
//!
//! * Schedules live in sibling `*.schedules.yaml` files — never embedded in
//!   `SKILL.md`. A skill points at them via
//!   `metadata.dcc-mcp.workflow.schedules` (mirrors the #356 sibling-file
//!   pattern used for workflows).
//! * [`SchedulerService::start`] consumes a directory of schedule files,
//!   spawns one Tokio task per cron schedule, and returns an
//!   [`axum::Router`] holding every declared webhook route.
//! * On fire, the scheduler builds a [`TriggerFire`] and hands it to the
//!   caller-supplied [`JobSink`]. The sink is responsible for actually
//!   enqueueing a `WorkflowJob` via whatever dispatch path exists (see
//!   `dcc_mcp_workflow::WorkflowJob`). The scheduler does **not** execute
//!   workflows itself.
//! * `max_concurrent` is enforced by an in-memory counter the caller
//!   decrements through [`SchedulerHandle::mark_terminal`] when it observes
//!   a terminal workflow status (e.g. via `$/dcc.workflowUpdated`).
//! * Webhook HMAC-SHA256 validation uses the `X-Hub-Signature-256` header
//!   convention and a constant-time comparison.
//!
//! ## Non-goals
//!
//! * Distributed scheduling / leader election (single-node only).
//! * Hot-reload of schedules (pick up on server restart).
//! * Fire-history UI.
//!
//! ## Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use dcc_mcp_scheduler::{
//!     JobSink, SchedulerConfig, SchedulerService, TriggerFire,
//! };
//!
//! struct Printer;
//! impl JobSink for Printer {
//!     fn enqueue(&self, fire: TriggerFire) -> Result<(), String> {
//!         println!("fire: schedule={} workflow={}", fire.schedule_id, fire.workflow);
//!         Ok(())
//!     }
//! }
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let cfg = SchedulerConfig::from_dir("./schedules")?;
//! let (handle, router) = SchedulerService::new(cfg, Arc::new(Printer)).start();
//! // mount `router` on your axum server, keep `handle` alive to keep tasks running
//! # Ok(()) }
//! ```

#![deny(missing_docs)]

pub mod error;
pub mod service;
pub mod sink;
pub mod spec;
pub mod template;
pub mod webhook;

#[cfg(feature = "python-bindings")]
pub mod python;

pub use error::SchedulerError;
pub use service::{SchedulerConfig, SchedulerHandle, SchedulerService};
pub use sink::{JobSink, TriggerFire};
pub use spec::{ScheduleSpec, TriggerSpec};
pub use webhook::{HMAC_HEADER, verify_hub_signature_256};
