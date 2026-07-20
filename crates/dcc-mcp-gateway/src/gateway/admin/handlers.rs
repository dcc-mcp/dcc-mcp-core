//! Re-export shim — handlers now live in `application::handlers`.
//!
//! This file exists for backward compatibility. New code should import from
//! `crate::gateway::admin::application::handlers` or `crate::gateway::admin::handlers`.

pub use super::application::handlers::*;
pub use super::application::health::*;
