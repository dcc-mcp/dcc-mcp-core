use super::*;

/// Returned by [`GatewayRunner::start`]. Dropping this handle aborts the
/// heartbeat and stale-cleanup background tasks.
///
/// # Task retention (issue #303 fix)
///
/// In earlier versions only the `AbortHandle` for the gateway's combined
/// supervisor task was stored here, and the supervisor's `JoinHandle` was
/// dropped at the end of `start_gateway_tasks`. Dropping a `JoinHandle`
/// *detaches* the task — in theory that is fine, but under PyO3-embedded
/// hosts on Windows the detached gateway accept loop can be starved of
/// scheduling time by the parent runtime (cf. issue #303, Run A symptom:
/// `bind()` succeeded, clients see `TIMEOUT`). Keeping the `JoinHandle`
/// alive here pins the task to its original runtime via
/// [`Runtime::enter`]-style ownership so it cannot be silently reclaimed,
/// giving downstream callers a handle they can actually `await`.
///
/// For the `ServerSpawnMode::Dedicated` path the listener runs on an OS
/// thread with its own `current_thread` runtime; [`Self::gateway_thread`]
/// holds its join handle so the Drop impl can block briefly for cleanup.
pub struct GatewayHandle {
    /// `true` if this instance won the gateway port at startup.
    pub is_gateway: bool,
    /// The `ServiceKey` this instance was registered under.
    pub service_key: ServiceKey,
    pub(crate) heartbeat_abort: Option<AbortHandle>,
    /// Combined gateway-HTTP + cleanup abort handle (set on the winner path).
    pub(crate) gateway_abort: Option<AbortHandle>,
    /// JoinHandle of the combined supervisor task, kept alive so the task
    /// is not detached (issue #303).
    #[allow(dead_code)]
    pub(crate) gateway_supervisor: Option<tokio::task::JoinHandle<()>>,
    /// OS thread running the dedicated-mode gateway accept loop.
    /// Only populated when `ServerSpawnMode::Dedicated` is used.
    pub(crate) gateway_thread: Option<std::thread::JoinHandle<()>>,
    /// Background challenger-loop abort handle (set when we entered challenger mode).
    pub(crate) challenger_abort: Option<AbortHandle>,
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        if let Some(h) = self.heartbeat_abort.take() {
            h.abort();
        }
        if let Some(h) = self.gateway_abort.take() {
            h.abort();
        }
        if let Some(h) = self.challenger_abort.take() {
            h.abort();
        }
        // Drop supervisor JoinHandle after aborting — this detaches the
        // underlying task cleanly. The AbortHandle above has already
        // cancelled its work; joining is optional.
        drop(self.gateway_supervisor.take());

        // Dedicated-mode OS thread: we *do not* join here to avoid
        // blocking Drop indefinitely if shutdown is in flight. The thread
        // observes the same yield signal and exits on its own.
        if let Some(h) = self.gateway_thread.take() {
            // Best-effort: detach; the thread is daemon-like and cleans
            // itself up once its yield signal fires.
            drop(h);
        }
    }
}

/// Result of [`GatewayRunner::run_election`].
///
/// Packages the election outcome together with the supervisor join
/// handle and optional OS-thread handle that must be kept alive for
/// the lifetime of the gateway role (issue #303).
#[allow(dead_code)]
pub(crate) struct ElectionOutcome {
    pub(crate) is_gateway: bool,
    pub(crate) gateway_abort: Option<AbortHandle>,
    pub(crate) challenger_abort: Option<AbortHandle>,
    pub(crate) gateway_supervisor: Option<tokio::task::JoinHandle<()>>,
    pub(crate) gateway_thread: Option<std::thread::JoinHandle<()>>,
}
