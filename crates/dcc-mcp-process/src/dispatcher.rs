//! Thread-affinity aware dispatch primitives for host-side execution.
//!
//! This module introduces an explicit scheduling contract (`ThreadAffinity`)
//! and a host-agnostic dispatcher trait (`HostDispatcher`) that future DCC
//! adapters can implement safely.

use serde_json::Value;
use tokio::sync::oneshot;
use tracing::{debug, warn};

use crate::error::ProcessError;

/// Declares where a job is allowed to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadAffinity {
    /// Must execute on the DCC application's main thread.
    Main,
    /// Must execute on a named host-managed thread.
    Named(&'static str),
    /// Can execute on any worker thread.
    Any,
}

impl std::fmt::Display for ThreadAffinity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Main => write!(f, "main"),
            Self::Named(name) => write!(f, "named:{name}"),
            Self::Any => write!(f, "any"),
        }
    }
}

/// Runtime capabilities surfaced by a host dispatcher implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostCapabilities {
    pub supports_main_thread: bool,
    pub supports_named_threads: bool,
    pub supports_any_thread: bool,
    pub supports_time_slicing: bool,
}

impl HostCapabilities {
    /// Capabilities for the reference standalone dispatcher.
    #[must_use]
    pub fn standalone() -> Self {
        Self {
            supports_main_thread: false,
            supports_named_threads: false,
            supports_any_thread: true,
            supports_time_slicing: false,
        }
    }
}

impl Default for HostCapabilities {
    fn default() -> Self {
        Self::standalone()
    }
}

/// Execution result for a single submitted host job.
#[derive(Debug, Clone)]
pub struct ActionOutcome {
    pub request_id: String,
    pub affinity: ThreadAffinity,
    pub success: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
}

impl ActionOutcome {
    pub(crate) fn ok(request_id: String, affinity: ThreadAffinity, output: Value) -> Self {
        Self {
            request_id,
            affinity,
            success: true,
            output: Some(output),
            error: None,
        }
    }

    pub(crate) fn err(
        request_id: String,
        affinity: ThreadAffinity,
        error: impl Into<String>,
    ) -> Self {
        Self {
            request_id,
            affinity,
            success: false,
            output: None,
            error: Some(error.into()),
        }
    }
}

/// Callable job payload invoked by a host dispatcher.
pub type JobFn = Box<dyn FnOnce() -> Result<Value, ProcessError> + Send + 'static>;

/// Request payload submitted to a [`HostDispatcher`].
pub struct JobRequest {
    pub request_id: String,
    pub affinity: ThreadAffinity,
    pub timeout_ms: Option<u64>,
    task: JobFn,
}

impl std::fmt::Debug for JobRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobRequest")
            .field("request_id", &self.request_id)
            .field("affinity", &self.affinity)
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

impl JobRequest {
    /// Create a job request with an explicit affinity.
    #[must_use]
    pub fn new(request_id: impl Into<String>, affinity: ThreadAffinity, task: JobFn) -> Self {
        Self {
            request_id: request_id.into(),
            affinity,
            timeout_ms: None,
            task,
        }
    }

    /// Convenience constructor for affinity-agnostic jobs.
    #[must_use]
    pub fn any(request_id: impl Into<String>, task: JobFn) -> Self {
        Self::new(request_id, ThreadAffinity::Any, task)
    }

    /// Set an optional soft timeout budget for the dispatcher.
    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    pub(crate) fn execute(self) -> ActionOutcome {
        let request_id = self.request_id;
        let affinity = self.affinity;
        let task = self.task;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
        match result {
            Ok(Ok(output)) => ActionOutcome::ok(request_id, affinity, output),
            Ok(Err(err)) => ActionOutcome::err(request_id, affinity, err.to_string()),
            Err(_) => ActionOutcome::err(request_id, affinity, "dispatcher task panicked"),
        }
    }
}

/// Unified host dispatch contract for thread-affinity aware execution.
pub trait HostDispatcher: Send + Sync + 'static {
    /// Submit a job and receive its eventual outcome asynchronously.
    fn submit(&self, req: JobRequest) -> oneshot::Receiver<ActionOutcome>;
    /// Affinities currently supported by this dispatcher.
    fn supported(&self) -> &[ThreadAffinity];
    /// Additional capability bits used by higher-level schedulers.
    fn capabilities(&self) -> HostCapabilities;
}

/// Reference dispatcher for standalone/CLI runtimes.
///
/// The standalone variant supports only [`ThreadAffinity::Any`] and runs jobs
/// on the active Tokio runtime.
#[derive(Debug, Clone, Default)]
pub struct StandaloneDispatcher {
    capabilities: HostCapabilities,
}

impl StandaloneDispatcher {
    #[must_use]
    pub fn new() -> Self {
        Self {
            capabilities: HostCapabilities::standalone(),
        }
    }
}

impl HostDispatcher for StandaloneDispatcher {
    fn submit(&self, req: JobRequest) -> oneshot::Receiver<ActionOutcome> {
        let (tx, rx) = oneshot::channel();

        if !matches!(req.affinity, ThreadAffinity::Any) {
            let message = format!(
                "Unsupported thread affinity '{affinity}' for standalone dispatcher",
                affinity = req.affinity
            );
            warn!(request_id = %req.request_id, "{message}");
            let _ = tx.send(ActionOutcome::err(req.request_id, req.affinity, message));
            return rx;
        }

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            let _ = tx.send(ActionOutcome::err(
                req.request_id,
                req.affinity,
                "No active Tokio runtime for StandaloneDispatcher",
            ));
            return rx;
        };

        debug!(
            request_id = %req.request_id,
            affinity = %req.affinity,
            timeout_ms = req.timeout_ms.unwrap_or_default(),
            "dispatching standalone job"
        );
        handle.spawn(async move {
            let _ = tx.send(req.execute());
        });

        rx
    }

    fn supported(&self) -> &[ThreadAffinity] {
        static SUPPORTED: [ThreadAffinity; 1] = [ThreadAffinity::Any];
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
    fn test_thread_affinity_display() {
        assert_eq!(ThreadAffinity::Main.to_string(), "main");
        assert_eq!(
            ThreadAffinity::Named("maya-ui").to_string(),
            "named:maya-ui"
        );
        assert_eq!(ThreadAffinity::Any.to_string(), "any");
    }

    #[test]
    fn test_standalone_capabilities() {
        let caps = HostCapabilities::standalone();
        assert!(!caps.supports_main_thread);
        assert!(!caps.supports_named_threads);
        assert!(caps.supports_any_thread);
        assert!(!caps.supports_time_slicing);
    }

    #[tokio::test]
    async fn test_standalone_dispatcher_executes_any_affinity_job() {
        let dispatcher = StandaloneDispatcher::new();
        let req = JobRequest::any("req-1", Box::new(|| Ok(json!({"ok": true}))));

        let outcome = dispatcher.submit(req).await.unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.request_id, "req-1");
        assert_eq!(outcome.affinity, ThreadAffinity::Any);
        assert_eq!(outcome.output, Some(json!({"ok": true})));
        assert!(outcome.error.is_none());
    }

    #[tokio::test]
    async fn test_standalone_dispatcher_rejects_non_any_affinity() {
        let dispatcher = StandaloneDispatcher::new();
        let req = JobRequest::new(
            "req-main",
            ThreadAffinity::Main,
            Box::new(|| Ok(json!(null))),
        );

        let outcome = dispatcher.submit(req).await.unwrap();
        assert!(!outcome.success);
        let error = outcome.error.unwrap_or_default();
        assert!(error.contains("Unsupported thread affinity"));
    }

    #[tokio::test]
    async fn test_standalone_dispatcher_surfaces_task_error() {
        let dispatcher = StandaloneDispatcher::new();
        let req = JobRequest::any(
            "req-err",
            Box::new(|| Err(ProcessError::internal("expected failure"))),
        );

        let outcome = dispatcher.submit(req).await.unwrap();
        assert!(!outcome.success);
        let error = outcome.error.unwrap_or_default();
        assert!(error.contains("expected failure"));
    }

    #[test]
    fn test_standalone_dispatcher_without_runtime_returns_error() {
        let dispatcher = StandaloneDispatcher::new();
        let req = JobRequest::any("req-sync", Box::new(|| Ok(json!({"sync": true}))));

        let outcome = dispatcher.submit(req).blocking_recv().unwrap();
        assert!(!outcome.success);
        let error = outcome.error.unwrap_or_default();
        assert!(error.contains("No active Tokio runtime"));
    }
}
