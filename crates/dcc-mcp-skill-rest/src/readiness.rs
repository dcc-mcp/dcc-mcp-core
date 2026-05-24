//! Readiness probe (#660, #1158).
//!
//! Plain `process alive` isn't enough for an embedded DCC backend.
//! Orchestrators need to distinguish the independently observable
//! pieces of runtime state:
//!
//! 1. Process alive — the HTTP listener answers.
//! 2. DCC ready — the host DCC has finished initialising
//!    (Maya scripting engine up, Blender scene ready, ...).
//! 3. Skill catalog ready — search/load metadata is usable.
//! 4. Dispatcher ready — the action dispatcher is wired.
//! 5. Host execution bridge and main-thread executor readiness —
//!    main-thread-only DCC tools can be routed safely.
//!
//! Only when the base routing bits are green should a gateway route
//! traffic here. Main-thread bridge bits are intentionally separate so
//! smoke tests can require them when validating main-thread tools.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

fn default_true() -> bool {
    true
}

/// Snapshot returned by [`ReadinessProbe::report`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct ReadinessReport {
    /// `true` if the process is alive.
    pub process: bool,
    /// `true` if the DCC host has finished initialising.
    pub dcc: bool,
    /// `true` if the skill catalog has been discovered and can answer
    /// search/load queries. Defaults to `true` for compatibility with
    /// pre-#1158 readiness payloads that only carried the original
    /// three fields.
    #[serde(default = "default_true")]
    pub skill_catalog: bool,
    /// `true` if the dispatcher is ready to accept calls.
    pub dispatcher: bool,
    /// `true` if a host execution bridge is attached for main-thread
    /// DCC API calls.
    #[serde(default)]
    pub host_execution_bridge: bool,
    /// `true` if the bridge has a running main-thread executor / pump.
    #[serde(default)]
    pub main_thread_executor: bool,
}

impl ReadinessReport {
    /// Base routing checks must be green for the backend to be ready
    /// for normal gateway traffic. Bridge-specific bits are exposed for
    /// automation to require explicitly when validating main-thread
    /// execution.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.process && self.dcc && self.skill_catalog && self.dispatcher
    }

    #[must_use]
    pub fn status_hint(&self) -> String {
        format!(
            "readiness: process={}, dcc={}, skill_catalog={}, dispatcher={}, \
             host_execution_bridge={}, main_thread_executor={}",
            self.process,
            self.dcc,
            self.skill_catalog,
            self.dispatcher,
            self.host_execution_bridge,
            self.main_thread_executor
        )
    }
}

/// Pluggable readiness probe. Cheap to clone.
pub trait ReadinessProbe: Send + Sync {
    fn report(&self) -> ReadinessReport;
}

/// Simple mutable probe whose state bits are toggled by the
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
                dcc: false,
                skill_catalog: true,
                dispatcher: false,
                host_execution_bridge: false,
                main_thread_executor: false,
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
                dcc: true,
                skill_catalog: true,
                dispatcher: true,
                host_execution_bridge: true,
                main_thread_executor: true,
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

    /// Toggle skill-catalog readiness.
    pub fn set_skill_catalog_ready(&self, ready: bool) {
        self.inner.write().skill_catalog = ready;
    }

    /// Toggle host execution bridge readiness.
    pub fn set_host_execution_bridge_ready(&self, ready: bool) {
        self.inner.write().host_execution_bridge = ready;
    }

    /// Toggle main-thread executor readiness.
    pub fn set_main_thread_executor_ready(&self, ready: bool) {
        self.inner.write().main_thread_executor = ready;
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
    fn report_is_not_ready_until_base_routing_bits_are_green() {
        let r = StaticReadiness::new();
        assert!(!r.report().is_ready());
        r.set_dispatcher_ready(true);
        assert!(!r.report().is_ready());
        r.set_dcc_ready(true);
        assert!(r.report().is_ready());
        r.set_skill_catalog_ready(false);
        assert!(!r.report().is_ready());
    }

    #[test]
    fn fully_ready_helper_is_green() {
        let r = StaticReadiness::fully_ready();
        assert!(r.report().is_ready());
        assert!(r.report().host_execution_bridge);
        assert!(r.report().main_thread_executor);
    }

    #[test]
    fn report_toggle_reflected_immediately() {
        let r = StaticReadiness::fully_ready();
        r.set_dcc_ready(false);
        assert!(!r.report().dcc);
    }

    #[test]
    fn old_three_field_payload_defaults_new_optional_bits() {
        let report: ReadinessReport =
            serde_json::from_str(r#"{"process":true,"dispatcher":true,"dcc":true}"#).unwrap();
        assert!(report.skill_catalog);
        assert!(!report.host_execution_bridge);
        assert!(!report.main_thread_executor);
        assert!(report.is_ready());
    }
}
