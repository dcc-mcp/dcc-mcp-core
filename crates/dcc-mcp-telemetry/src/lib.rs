//! # dcc-mcp-telemetry
//!
//! OpenTelemetry tracing, metrics, and structured logging for the DCC-MCP ecosystem.
//!
//! ## Features
//!
//! | Feature | Description |
//! |---------|-------------|
//! | (default) | stdout + tracing subscriber, in-memory metrics |
//! | `python-bindings` | PyO3 bindings: `TelemetryConfig`, `ToolRecorder`, etc. |
//! | `otlp-exporter` | OTLP gRPC export to Jaeger / Grafana Tempo / Prometheus |
//!
//! ## Modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`error`] | `TelemetryError` enum |
//! | [`types`] | `TelemetryConfig`, `ToolMetrics`, `ExporterBackend`, span keys |
//! | [`provider`] | Global provider init/shutdown, `tracer()` / `meter()` accessors |
//! | [`recorder`] | `ToolRecorder` — per-action timing and success-rate metrics |
//! | [`span`] | Convenience `tracing` span helpers (`action_span`, etc.) |
//!
//! ## Quick Start
//!
//! ```text
//! use dcc_mcp_telemetry::{
//!     types::TelemetryConfig,
//!     provider,
//!     recorder::ToolRecorder,
//! };
//!
//! // Initialise once at startup (noop backend good for tests)
//! let cfg = TelemetryConfig::builder("my-service")
//!     .with_noop_exporter()
//!     .build();
//! provider::init(&cfg).unwrap();
//!
//! // Record an action
//! let recorder = ToolRecorder::new("my-service");
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

#[cfg(feature = "prometheus")]
pub mod prometheus;

// Convenient root-level re-exports
pub use error::TelemetryError;
pub use provider::{init, is_initialized, meter, shutdown, tracer};
pub use recorder::ToolRecorder;
pub use types::{ExporterBackend, LogFormat, TelemetryConfig, ToolMetrics};

#[cfg(feature = "prometheus")]
pub use prometheus::{PROMETHEUS_CONTENT_TYPE, PrometheusExporter};

#[cfg(feature = "python-bindings")]
pub use python::{
    PyRecordingGuard, PyTelemetryConfig, PyToolMetrics, PyToolRecorder,
    py_is_telemetry_initialized, py_shutdown_telemetry,
};
