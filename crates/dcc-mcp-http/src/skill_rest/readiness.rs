//! Three-state readiness probe (#660).
//!
//! Plain `process alive` isn't enough for an embedded DCC backend.
//! Orchestrators need to distinguish three things:
//!
//! 1. Process alive — the HTTP listener answers.
//! 2. Dispatcher ready — the action dispatcher is wired.
//! 3. DCC ready — the host DCC has finished initialising
//!    (Maya scripting engine up, Blender scene ready, ...).
//!
//! Only when all three are green should a gateway route traffic here.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Snapshot returned by [`ReadinessProbe::report`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct ReadinessReport {
    /// `true` if the process is alive.
    pub process: bool,
    /// `true` if the dispatcher is ready to accept calls.
    pub dispatcher: bool,
    /// `true` if the DCC host has finished initialising.
    pub dcc: bool,
}

impl ReadinessReport {
    /// All three checks must be green for the backend to be fully
    /// ready.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.process && self.dispatcher && self.dcc
    }
}

/// Pluggable readiness probe. Cheap to clone.
pub trait ReadinessProbe: Send + Sync {
    fn report(&self) -> ReadinessReport;
}

/// Simple mutable probe whose three state bits are toggled by the
/// embedder. Perfect default — richer probes can replace it without
/// touching the router.
#[derive(Debug, Clone, Default)]
pub struct StaticReadiness {
    inner: Arc<RwLock<ReadinessReport>>,
}

impl StaticReadiness {
    /// Build a probe starting in the not-ready state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ReadinessReport {
                process: true,
                dispatcher: false,
                dcc: false,
            })),
        }
    }

    /// Start fully ready — convenient for tests and single-binary
    /// servers with no host DCC to wait on.
    #[must_use]
    pub fn fully_ready() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ReadinessReport {
                process: true,
                dispatcher: true,
                dcc: true,
            })),
        }
    }

    /// Toggle dispatcher readiness.
    pub fn set_dispatcher_ready(&self, ready: bool) {
        self.inner.write().dispatcher = ready;
    }

    /// Toggle DCC host readiness.
    pub fn set_dcc_ready(&self, ready: bool) {
        self.inner.write().dcc = ready;
    }
}

impl ReadinessProbe for StaticReadiness {
    fn report(&self) -> ReadinessReport {
        self.inner.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_not_ready_until_all_three_are_green() {
        let r = StaticReadiness::new();
        assert!(!r.report().is_ready());
        r.set_dispatcher_ready(true);
        assert!(!r.report().is_ready());
        r.set_dcc_ready(true);
        assert!(r.report().is_ready());
    }

    #[test]
    fn fully_ready_helper_is_green() {
        let r = StaticReadiness::fully_ready();
        assert!(r.report().is_ready());
    }

    #[test]
    fn report_toggle_reflected_immediately() {
        let r = StaticReadiness::fully_ready();
        r.set_dcc_ready(false);
        assert!(!r.report().dcc);
    }
}
