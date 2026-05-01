//! Consolidated integration-test binary for `dcc-mcp-http`.
//!
//! All nine former top-level integration-test files are compiled as *modules*
//! of this single binary.  Cargo used to emit one separate test binary per
//! file in `tests/`; with this layout it emits exactly **one** binary, which
//! cuts link overhead from 9 invocations to 1 and removes ~50 s of redundant
//! codegen (measured with `cargo build --all-targets --timings`).
//!
//! Feature-gated modules (`job_persistence`, `prometheus_endpoint`) carry
//! their own `#![cfg(feature = "...")]` guards and compile to empty modules
//! when the feature is off — no `required-features` dance needed.

#[path = "http/async_dispatch_cancel.rs"]
mod async_dispatch_cancel;

#[path = "http/backend_timeout.rs"]
mod backend_timeout;

#[path = "http/gateway_passthrough.rs"]
mod gateway_passthrough;

#[path = "http/gateway_reachability.rs"]
mod gateway_reachability;

#[path = "http/gateway_tool_exposure.rs"]
mod gateway_tool_exposure;

#[path = "http/job_persistence.rs"]
mod job_persistence;

#[path = "http/jobs_get_status.rs"]
mod jobs_get_status;

#[path = "http/notifications.rs"]
mod notifications;

#[path = "http/prometheus_endpoint.rs"]
mod prometheus_endpoint;

#[path = "http/resources.rs"]
mod resources;
