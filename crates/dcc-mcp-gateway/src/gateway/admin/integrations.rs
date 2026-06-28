//! Re-export shim — integration config now lives in `infra::integrations`.
//!
//! This file exists for backward compatibility. New code should import from
//! `crate::gateway::admin::infra::integrations` or `crate::gateway::admin::integrations`.

pub use super::infra::integrations::*;
