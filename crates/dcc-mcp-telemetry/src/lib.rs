//! # dcc-mcp-telemetry
//!
//! OpenTelemetry tracing, metrics, and structured logging for the DCC-MCP ecosystem.
//!
//! ## Features
//!
//! | Feature | Description |
//! |---------|-------------|
//! | (default) | stdout + tracing subscriber, in-memory metrics |
//! | `python-bindings` | PyO3 bindings: `TelemetryConfig`, `ActionRecorder`, etc. |
//! | `otlp-exporter` | OTLP gRPC export to Jaeger / Grafana Tempo / Prometheus |
//!
//! ## Modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`error`] | `TelemetryError` enum |
//! | [`types`] | `TelemetryConfig`, `ActionMetrics`, `ExporterBackend`, span keys |
//! | [`provider`] | Global provider init/shutdown, `tracer()` / `meter()` accessors |
//! | [`recorder`] | `ActionRecorder` — per-action timing and success-rate metrics |
//! | [`span`] | Convenience `tracing` span helpers (`action_span`, etc.) |
//!
//! ## Quick Start
//!
//! ```text
//! use dcc_mcp_telemetry::{
//!     types::TelemetryConfig,
//!     provider,
//!     recorder::ActionRecorder,
//! };
//!
//! // Initialise once at startup (noop backend good for tests)
//! let cfg = TelemetryConfig::builder("my-service")
//!     .with_noop_exporter()
//!     .build();
//! provider::init(&cfg).unwrap();
//!
//! // Record an action
//! let recorder = ActionRecorder::new("my-service");
//! let guard = recorder.start("create_sphere", "maya");
//! // ... do work ...
//! guard.finish(true);
//!
//! // Query metrics
//! let m = recorder.metrics("create_sphere").unwrap();
//! println!("invocations={} success_rate={:.2}", m.invocation_count, m.success_rate());
//!
//! // Flush at shutdown
//! provider::shutdown();
//! ```

pub mod error;
pub mod provider;
pub mod recorder;
pub mod span;
pub mod types;

#[cfg(feature = "python-bindings")]
pub mod python;

// Convenient root-level re-exports
pub use error::TelemetryError;
pub use provider::{init, is_initialized, meter, shutdown, tracer};
pub use recorder::ActionRecorder;
pub use recorder::ActionRecorder as ToolRecorder;
pub use types::ActionMetrics as ToolMetrics;
pub use types::{ActionMetrics, ExporterBackend, LogFormat, TelemetryConfig};

#[cfg(feature = "python-bindings")]
pub use python::{
    PyActionMetrics, PyActionRecorder, PyRecordingGuard, PyTelemetryConfig,
    py_is_telemetry_initialized, py_shutdown_telemetry,
};
