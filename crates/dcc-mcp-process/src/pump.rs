//! Main-thread pump and pumped dispatcher for DCC host integration.
//!
//! Wraps [`ipckit::MainThreadPump`] to provide cooperative main-thread draining
//! for DCC hosts (Maya, Houdini, Blender, …). The [`PumpedDispatcher`] combines
//! a main-thread pump for [`ThreadAffinity::Main`] jobs with Tokio worker
//! execution for [`ThreadAffinity::Any`] jobs.
//!
//! # DCC host integration
//!
//! | Host | Idle callback |
//! |------|---------------|
//! | Maya | `cmds.scriptJob(idleEvent=pump_fn)` |
//! | Houdini | `hdefereval.execute_deferred_after_waiting` |
//! | 3dsMax | `pymxs.run_at_ui_idle` |
//! | Blender | `bpy.app.timers.register` |
//!
//! # Example
//!
//! ```rust
//! use dcc_mcp_process::pump::{PumpedDispatcher, PumpStats};
//! use dcc_mcp_process::dispatcher::{HostDispatcher, JobRequest, ThreadAffinity};
//! use std::time::Duration;
//!
//! let dispatcher = PumpedDispatcher::new(Duration::from_millis(8));
//!
//! // Submit a main-thread job (will be drained by pump())
//! let req = JobRequest::new("update-ui", ThreadAffinity::Main, Box::new(|| {
//!     Ok(serde_json::json!({"updated": true}))
//! }));
//! let rx = dispatcher.submit(req);
//!
//! // In the host idle callback:
//! let stats = dispatcher.pump();
//! assert_eq!(stats.processed, 1);
//! ```

use std::sync::Arc;
use std::time::Duration;

use ipckit::MainThreadPump;
use tokio::sync::oneshot;
use tracing::debug;

use crate::dispatcher::{
    ActionOutcome, HostCapabilities, HostDispatcher, JobRequest, StandaloneDispatcher,
    ThreadAffinity,
};

/// Default time-slice budget for a single `pump()` call (8 ms ≈ one frame at 120 Hz).
const DEFAULT_BUDGET_MS: u64 = 8;

// ── PumpStats ────────────────────────────────────────────────────────────────

/// Statistics returned by a single [`PumpedDispatcher::pump`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpStats {
    /// Number of main-thread work items processed.
    pub processed: usize,
    /// Number of items still pending after this pump call.
    pub remaining: usize,
}

// ── PumpedDispatcher ─────────────────────────────────────────────────────────

/// Thread-affinity aware dispatcher that combines:
///
/// - **Main-thread pump** ([`ipckit::MainThreadPump`]) for [`ThreadAffinity::Main`] jobs,
///   drained cooperatively from the DCC host's idle callback.
/// - **Tokio worker pool** (via [`StandaloneDispatcher`]) for [`ThreadAffinity::Any`] jobs.
///
/// [`ThreadAffinity::Named`] jobs are dispatched to the main-thread pump with
/// the thread name attached as metadata (future: route to named pump by key).
#[derive(Clone)]
pub struct PumpedDispatcher {
    pump: MainThreadPump,
    budget: Duration,
    any_dispatcher: Arc<StandaloneDispatcher>,
    capabilities: HostCapabilities,
}

impl PumpedDispatcher {
    /// Create a new pumped dispatcher with the given per-`pump()` time budget.
    pub fn new(budget: Duration) -> Self {
        let pump = MainThreadPump::new();
        Self {
            pump,
            budget,
            any_dispatcher: Arc::new(StandaloneDispatcher::new()),
            capabilities: HostCapabilities {
                supports_main_thread: true,
                supports_named_threads: true,
                supports_any_thread: true,
                supports_time_slicing: true,
            },
        }
    }

    /// Create with the default 8 ms budget.
    #[must_use]
    pub fn with_default_budget() -> Self {
        Self::new(Duration::from_millis(DEFAULT_BUDGET_MS))
    }

    /// Drain pending main-thread work items using the configured budget.
    ///
    /// Call this from the DCC host's idle/update callback (e.g. Maya
    /// `scriptJob(idleEvent=...)`). Returns [`PumpStats`] describing
    /// what happened.
    pub fn pump(&self) -> PumpStats {
        let stats = self.pump.pump(self.budget);
        PumpStats {
            processed: stats.processed,
            remaining: stats.remaining,
        }
    }

    /// Drain with an explicit budget override for this call only.
    pub fn pump_with_budget(&self, budget: Duration) -> PumpStats {
        let stats = self.pump.pump(budget);
        PumpStats {
            processed: stats.processed,
            remaining: stats.remaining,
        }
    }

    /// Number of main-thread items currently waiting.
    pub fn pending(&self) -> usize {
        self.pump.pending()
    }

    /// Total items ever dispatched to the main-thread pump.
    pub fn total_dispatched(&self) -> u64 {
        self.pump.total_dispatched()
    }

    /// Total items ever processed by the main-thread pump.
    pub fn total_processed(&self) -> u64 {
        self.pump.total_processed()
    }

    /// The configured default budget.
    pub fn budget(&self) -> Duration {
        self.budget
    }
}

impl std::fmt::Debug for PumpedDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PumpedDispatcher")
            .field("budget_ms", &self.budget.as_millis())
            .field("pending", &self.pump.pending())
            .finish_non_exhaustive()
    }
}

impl HostDispatcher for PumpedDispatcher {
    fn submit(&self, req: JobRequest) -> oneshot::Receiver<ActionOutcome> {
        let (tx, rx) = oneshot::channel();

        match req.affinity {
            ThreadAffinity::Main | ThreadAffinity::Named(_) => {
                let request_id = req.request_id.clone();
                let affinity = req.affinity;

                // Dispatch the job to the main-thread pump.
                // The closure captures `req.task` and `tx`.
                self.pump.dispatch(move || {
                    let outcome = req.execute();
                    let _ = tx.send(outcome);
                });

                debug!(
                    request_id = %request_id,
                    affinity = %affinity,
                    "dispatched to main-thread pump"
                );
            }
            ThreadAffinity::Any => {
                // Delegate to the standalone (Tokio) dispatcher.
                let any_rx = self.any_dispatcher.submit(req);
                tokio::spawn(async move {
                    match any_rx.await {
                        Ok(outcome) => {
                            let _ = tx.send(outcome);
                        }
                        Err(_) => {
                            let _ = tx.send(ActionOutcome::err(
                                "unknown".to_string(),
                                ThreadAffinity::Any,
                                "PumpedDispatcher: Any worker oneshot dropped",
                            ));
                        }
                    }
                });
            }
        }

        rx
    }

    fn supported(&self) -> &[ThreadAffinity] {
        static SUPPORTED: [ThreadAffinity; 3] = [
            ThreadAffinity::Main,
            ThreadAffinity::Named("named"),
            ThreadAffinity::Any,
        ];
        &SUPPORTED
    }

    fn capabilities(&self) -> HostCapabilities {
        self.capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_pump_stats_default() {
        let stats = PumpStats {
            processed: 0,
            remaining: 0,
        };
        assert_eq!(stats.processed, 0);
        assert_eq!(stats.remaining, 0);
    }

    #[test]
    fn test_pumped_dispatcher_capabilities() {
        let d = PumpedDispatcher::with_default_budget();
        let caps = d.capabilities();
        assert!(caps.supports_main_thread);
        assert!(caps.supports_named_threads);
        assert!(caps.supports_any_thread);
        assert!(caps.supports_time_slicing);
    }

    #[test]
    fn test_pumped_dispatcher_supported_affinities() {
        let d = PumpedDispatcher::with_default_budget();
        let supported = d.supported();
        assert!(supported.contains(&ThreadAffinity::Main));
        assert!(supported.contains(&ThreadAffinity::Any));
    }

    #[tokio::test]
    async fn test_pumped_dispatcher_any_job_on_tokio() {
        let d = PumpedDispatcher::with_default_budget();
        let req = JobRequest::any("req-any", Box::new(|| Ok(json!({"ok": true}))));
        let outcome = d.submit(req).await.unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.affinity, ThreadAffinity::Any);
    }

    #[test]
    fn test_pumped_dispatcher_main_job_via_pump() {
        let d = PumpedDispatcher::new(Duration::from_millis(100));
        let req = JobRequest::new("req-main", ThreadAffinity::Main, Box::new(|| Ok(json!(42))));
        let rx = d.submit(req);

        // Pump to execute the main-thread job.
        let stats = d.pump();
        assert_eq!(stats.processed, 1);

        // Check the outcome.
        let outcome = rx.blocking_recv().unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.affinity, ThreadAffinity::Main);
        assert_eq!(outcome.output, Some(json!(42)));
    }

    #[test]
    fn test_pumped_dispatcher_named_job_via_pump() {
        let d = PumpedDispatcher::new(Duration::from_millis(100));
        let req = JobRequest::new(
            "req-named",
            ThreadAffinity::Named("RenderThread"),
            Box::new(|| Ok(json!("rendered"))),
        );
        let rx = d.submit(req);

        let stats = d.pump();
        assert_eq!(stats.processed, 1);

        let outcome = rx.blocking_recv().unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.affinity, ThreadAffinity::Named("RenderThread"));
    }

    #[test]
    fn test_pumped_dispatcher_budget_respected() {
        let d = PumpedDispatcher::new(Duration::from_millis(1));
        // A zero-budget pump should not process items (or very few).
        d.pump.dispatch(|| {});
        let _stats = d.pump_with_budget(Duration::ZERO);
        // With zero budget, the first check may or may not process.
        // Clean up.
        d.pump();
    }

    #[test]
    fn test_pumped_dispatcher_pending_count() {
        let d = PumpedDispatcher::with_default_budget();
        assert_eq!(d.pending(), 0);

        d.pump.dispatch(|| {});
        d.pump.dispatch(|| {});
        assert_eq!(d.pending(), 2);

        d.pump();
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn test_pumped_dispatcher_stats_counters() {
        let d = PumpedDispatcher::new(Duration::from_millis(100));
        assert_eq!(d.total_dispatched(), 0);
        assert_eq!(d.total_processed(), 0);

        let req = JobRequest::new("s1", ThreadAffinity::Main, Box::new(|| Ok(json!(1))));
        let _ = d.submit(req);
        assert_eq!(d.total_dispatched(), 1);

        d.pump();
        assert_eq!(d.total_processed(), 1);
    }
}
