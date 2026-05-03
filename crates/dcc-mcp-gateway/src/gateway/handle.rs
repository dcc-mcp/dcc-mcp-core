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
///
/// # Graceful deregistration (issue #718)
///
/// Prior to #718 this handle's `Drop` only aborted background tasks and
/// left the `FileRegistry` rows stamped with a fresh `last_heartbeat`.
/// Peers reading `services.json` kept seeing the now-dead instance as
/// "available" until `stale_timeout_secs` (default 30 s) elapsed. We
/// now carry an `Arc<RwLock<FileRegistry>>` and call
/// `FileRegistry::deregister` for the service key (and, for gateway
/// winners, the `__gateway__` sentinel) in `Drop`, so `services.json`
/// is purged immediately on clean shutdown.
///
/// The deregistration is idempotent — each key is consumed via `take()`
/// so calling Drop more than once is a no-op. The registry's outer lock
/// is the async `tokio::sync::RwLock`; Drop uses `try_read()` to avoid
/// blocking an executor. All callers in this crate only take *read*
/// locks for short synchronous operations, so contention is
/// effectively nil in practice. If `try_read` ever fails we log at
/// `warn!` and fall back to the stale-row cleanup path.
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
    /// Shared `FileRegistry` used to deregister the instance (and the
    /// sentinel, when we are the gateway) on Drop. See issue #718.
    pub(crate) registry: Arc<RwLock<FileRegistry>>,
    /// Pending deregistrations. Populated with the instance key (and the
    /// sentinel key for winners); `Drop` drains the vector so a second
    /// call is a no-op. See issue #718.
    pub(crate) pending_deregister: Vec<ServiceKey>,
}

impl GatewayHandle {
    /// Deregister every pending `ServiceKey` from the `FileRegistry` and
    /// clear the queue. Idempotent and cheap — safe to call from both
    /// async shutdown paths and `Drop`.
    ///
    /// Uses `try_read()` because Drop is synchronous and cannot await.
    /// In this crate the registry lock is only ever held in `read` mode
    /// for brief O(n) DashMap scans, so the fast path virtually always
    /// succeeds. On the rare contention case we log and leave the row —
    /// the existing `stale_timeout_secs` cleanup path still purges it.
    pub fn deregister_all(&mut self) {
        if self.pending_deregister.is_empty() {
            return;
        }
        let keys = std::mem::take(&mut self.pending_deregister);
        match self.registry.try_read() {
            Ok(reg) => {
                for key in keys {
                    if let Err(e) = reg.deregister(&key) {
                        tracing::warn!(
                            error = %e,
                            dcc_type = %key.dcc_type,
                            instance_id = %key.instance_id,
                            "FileRegistry::deregister failed during gateway shutdown"
                        );
                    }
                }
            }
            Err(_) => {
                tracing::warn!(
                    pending = keys.len(),
                    "FileRegistry read lock contended during shutdown — \
                     falling back to stale-timeout cleanup (issue #718)"
                );
            }
        }
    }
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        // Issue #718: deregister BEFORE aborting the heartbeat so that
        // even if the heartbeat were to race us, the row is already
        // gone. `deregister_all` is idempotent via `take()`.
        self.deregister_all();

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
    /// `__gateway__` sentinel key registered by the winner path; carried
    /// back to the `GatewayHandle` so `Drop` can deregister it on clean
    /// shutdown (issue #718).
    pub(crate) sentinel_key: Option<ServiceKey>,
}
