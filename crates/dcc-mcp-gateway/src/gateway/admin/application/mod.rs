//! HTTP handler functions — orchestration and routing.
//!
//! Each handler is a pure async fn that takes `State<AdminState>`, extracts
//! query/path parameters, delegates to infra/domain modules, and returns
//! axum responses.

#[cfg(feature = "admin")]
pub mod analytics;

#[cfg(feature = "admin")]
pub mod governance;

#[cfg(feature = "admin")]
pub(super) mod skill_health;

#[cfg(feature = "admin")]
pub(super) mod traffic;

#[cfg(feature = "admin")]
pub mod handlers;

#[cfg(feature = "admin")]
pub mod health;

#[cfg(feature = "admin")]
pub use handlers::*;
#[cfg(feature = "admin")]
pub use health::*;
