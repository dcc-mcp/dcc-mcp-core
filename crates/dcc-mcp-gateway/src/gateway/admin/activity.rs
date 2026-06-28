//! Re-export shim — activity projection now lives in `infra::activity`.
//!
//! This file exists for backward compatibility. New code should import from
//! `crate::gateway::admin::infra::activity` or `crate::gateway::admin::activity`.

pub use super::infra::activity::*;
