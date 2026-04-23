use super::types::Pending;
use super::*;

/// Per-backend subscription state.
///
/// Public fields are `pub(crate)` so the gateway's `start_gateway_tasks`
/// can spawn / abort the reconnect loop directly.
pub(crate) struct BackendSubscriber {
    /// Absolute URL of the backend MCP endpoint (`http://host:port/mcp`).
    #[allow(dead_code)]
    pub(crate) url: String,
    /// Reconnect loop JoinHandle. `None` when the subscriber was never
    /// started or has been aborted.
    pub(crate) task: Option<JoinHandle<()>>,
    /// Shared state with the reconnect task.
    pub(crate) shared: Arc<BackendShared>,
}

impl BackendSubscriber {
    /// Abort the reconnect loop. Idempotent.
    pub fn abort(&mut self) {
        if let Some(h) = self.task.take() {
            h.abort();
        }
    }
}

impl Drop for BackendSubscriber {
    fn drop(&mut self) {
        self.abort();
    }
}

/// Shared state for a single backend's reconnect loop.
pub(crate) struct BackendShared {
    /// Backend URL for logging.
    pub(crate) url: String,
    /// Per-backend bounded buffer of notifications whose target session
    /// could not yet be resolved.
    pub(crate) pending: Mutex<VecDeque<Pending>>,
    /// Number of consecutive reconnect attempts (reset on a successful
    /// open of the SSE stream).
    pub(crate) reconnect_attempts: Mutex<u32>,
}

impl BackendShared {
    pub(crate) fn new(url: String) -> Self {
        Self {
            url,
            pending: Mutex::new(VecDeque::with_capacity(PENDING_BUFFER_CAP)),
            reconnect_attempts: Mutex::new(0),
        }
    }
}
