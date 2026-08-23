//! HTTP handler functions — orchestration and routing.
//!
//! Each handler is a pure async fn that takes `State<AdminState>`, extracts
//! query/path parameters, delegates to infra/domain modules, and returns
//! axum responses.

#[cfg(feature = "admin")]
pub mod analytics;

#[cfg(feature = "admin")]
pub(super) mod agent_trace;

#[cfg(feature = "admin")]
pub mod artifacts;

#[cfg(feature = "admin")]
pub(super) mod debug_response;

#[cfg(feature = "admin")]
pub(super) mod events;

#[cfg(feature = "admin")]
pub(crate) mod experiments;

#[cfg(feature = "admin")]
pub(super) mod general;

#[cfg(feature = "admin")]
pub mod governance;

#[cfg(feature = "admin")]
pub(super) mod issue_report;

#[cfg(feature = "admin")]
pub(super) mod memory;

#[cfg(feature = "admin")]
pub mod marketplace;

#[cfg(feature = "admin")]
pub(super) mod recordings;

#[cfg(feature = "admin")]
pub(crate) mod router;

#[cfg(feature = "admin")]
pub mod sessions;

#[cfg(feature = "admin")]
pub(super) mod skill_health;

#[cfg(feature = "admin")]
pub(super) mod skill_paths;

#[cfg(feature = "admin")]
pub mod skill_reload;

#[cfg(feature = "admin")]
pub(super) mod traffic;

#[cfg(feature = "admin")]
pub mod workers;

#[cfg(feature = "admin")]
pub mod workflows;

#[cfg(feature = "admin")]
pub mod handlers;

#[cfg(feature = "admin")]
pub mod health;

#[cfg(feature = "admin")]
pub use handlers::*;
#[cfg(feature = "admin")]
pub use health::*;
