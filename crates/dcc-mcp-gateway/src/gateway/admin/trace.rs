//! Re-export shim — trace types now live in `domain::trace`.
//!
//! This file exists for backward compatibility. New code should import from
//! `crate::gateway::admin::domain::trace` or `crate::gateway::admin::trace`.

pub use super::domain::trace::*;
