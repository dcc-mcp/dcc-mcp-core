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
    /// Stable `Mcp-Session-Id` the gateway uses for every outbound
    /// request (SSE GET + forwarded JSON-RPC POSTs) to this backend.
    ///
    /// The backend's `resources/subscribe` handler requires a session
    /// header so it can route `notifications/resources/updated` back
    /// onto *that* session's SSE stream (#732). The gateway's subscriber
    /// loop is the reader of that stream, so we bind the subscribe POST
    /// to the same session — giving the backend a single sink to push
    /// resource updates to.
    ///
    /// Wrapped in a `Mutex` because the id is not known until the first
    /// `initialize` handshake against the backend completes (see
    /// [`SubscriberManager::open_stream`] / the reconnect loop). Writers
    /// overwrite the slot on every reconnect so a backend restart that
    /// loses its session table gets a fresh id without restarting the
    /// gateway. Readers (the handler that forwards `resources/subscribe`)
    /// block until the first id is minted; in practice this is a
    /// microsecond-scale wait since `ensure_subscribed` is called
    /// eagerly by the gateway supervisor long before any client
    /// subscription arrives.
    pub(crate) session_id: Mutex<Option<String>>,
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
            session_id: Mutex::new(None),
            pending: Mutex::new(VecDeque::with_capacity(PENDING_BUFFER_CAP)),
            reconnect_attempts: Mutex::new(0),
        }
    }
}
