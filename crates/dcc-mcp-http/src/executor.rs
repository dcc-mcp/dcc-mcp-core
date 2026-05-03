//! DCC main-thread executor — the thread-safety bridge.
//!
//! DCC software (Maya, Blender, Houdini, 3ds Max) requires that all
//! scene-modifying operations execute on the **application's main thread**.
//! HTTP requests arrive on Tokio worker threads, so we must hand tasks off
//! and await their results.
//!
//! # How it works
//!
//! ```text
//!  Tokio worker thread             DCC main thread
//!  ────────────────────            ─────────────────
//!  DeferredExecutor::execute()     poll_pending()  ← called by DCC event loop
//!        │                               │
//!        │── DccTask ──► mpsc::channel ──┤
//!        │                               │ run task fn
//!        │◄── result channel ────────────┘
//! ```
//!
//! For non-DCC environments (testing, pure Python), a simple in-process
//! executor runs tasks directly on the calling thread.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Shared observability state for a [`DccExecutorHandle`] and its
/// backing channel (issue #715).
///
/// Cloned (via `Arc`) alongside the handle so every sender and the
/// owning [`DeferredExecutor`] see the same counters and submit-time
/// ledger. The hot-path writers (submit / deliver) only take short
/// `parking_lot` mutexes for two bounded `VecDeque`s; everything else
/// is atomic.
#[derive(Debug)]
pub(crate) struct ExecutorStats {
    /// Configured channel capacity (`send_timeout` blocks when full).
    pub capacity: usize,
    /// How long `execute` will block on a full channel before
    /// surfacing [`crate::error::HttpError::QueueOverloaded`].
    pub send_timeout: Duration,
    pub total_enqueued: AtomicU64,
    pub total_dequeued: AtomicU64,
    pub total_rejected: AtomicU64,
    /// Submit timestamps of currently-queued jobs, oldest first. Used
    /// to surface `oldest_submit_age`.
    pub submit_times: parking_lot::Mutex<VecDeque<Instant>>,
    /// Wait-time samples for completed jobs (bounded ring of 256).
    pub wait_samples: parking_lot::Mutex<VecDeque<u64>>,
}

impl ExecutorStats {
    pub(crate) fn new(capacity: usize, send_timeout: Duration) -> Arc<Self> {
        Arc::new(Self {
            capacity,
            send_timeout,
            total_enqueued: AtomicU64::new(0),
            total_dequeued: AtomicU64::new(0),
            total_rejected: AtomicU64::new(0),
            submit_times: parking_lot::Mutex::new(VecDeque::new()),
            wait_samples: parking_lot::Mutex::new(VecDeque::with_capacity(256)),
        })
    }

    pub(crate) fn record_submit(&self, at: Instant) {
        self.total_enqueued.fetch_add(1, Ordering::Release);
        self.submit_times.lock().push_back(at);
    }

    pub(crate) fn record_dequeue(&self, now: Instant) {
        let submitted_at = self.submit_times.lock().pop_front();
        if let Some(submitted) = submitted_at {
            let wait_ms = now.saturating_duration_since(submitted).as_millis() as u64;
            let mut ring = self.wait_samples.lock();
            if ring.len() == 256 {
                ring.pop_front();
            }
            ring.push_back(wait_ms);
        }
        self.total_dequeued.fetch_add(1, Ordering::Release);
    }

    pub(crate) fn record_reject(&self) {
        self.total_rejected.fetch_add(1, Ordering::Release);
    }

    /// Approximate current queue depth — `enqueued - dequeued`,
    /// saturating at zero.
    pub(crate) fn pending(&self) -> usize {
        let enq = self.total_enqueued.load(Ordering::Acquire);
        let deq = self.total_dequeued.load(Ordering::Acquire);
        enq.saturating_sub(deq) as usize
    }

    pub(crate) fn oldest_wait(&self) -> Option<Duration> {
        self.submit_times.lock().front().map(|t| t.elapsed())
    }

    pub(crate) fn percentiles(&self) -> (Option<u64>, Option<u64>, Option<u64>) {
        let ring = self.wait_samples.lock();
        if ring.is_empty() {
            return (None, None, None);
        }
        let mut sorted: Vec<u64> = ring.iter().copied().collect();
        drop(ring);
        sorted.sort_unstable();
        let pick = |q: f64| -> u64 {
            let n = sorted.len();
            let idx = ((q * n as f64).ceil() as usize)
                .saturating_sub(1)
                .min(n - 1);
            sorted[idx]
        };
        (Some(pick(0.50)), Some(pick(0.95)), Some(pick(0.99)))
    }
}

/// Public observability snapshot for the DCC main-thread executor
/// queue (issue #715). Field names are the stable wire shape
/// consumed by `diagnostics__process_status.queue.executor_*`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutorQueueStats {
    pub pending: usize,
    pub capacity: usize,
    pub total_enqueued: u64,
    pub total_dequeued: u64,
    pub total_rejected: u64,
    pub oldest_wait_ms: Option<u64>,
    pub wait_p50_ms: Option<u64>,
    pub wait_p95_ms: Option<u64>,
    pub wait_p99_ms: Option<u64>,
}

/// A boxed async-compatible task that runs on the DCC main thread.
///
/// Returns a JSON string result (or an error string).
pub type DccTaskFn = Box<dyn FnOnce() -> String + Send + 'static>;

/// A pending DCC task with its result channel.
///
/// `pub(crate)` so [`crate::host_bridge`] can construct the mpsc
/// channel that backs a bridged [`DccExecutorHandle`]. External
/// crates still cannot see this type — they use the public
/// [`DccExecutorHandle::execute`] API.
pub(crate) struct DccTask {
    pub(crate) func: DccTaskFn,
    pub(crate) result_tx: oneshot::Sender<String>,
}

/// Handle owned by the HTTP server to submit tasks to the DCC main thread.
#[derive(Clone)]
pub struct DccExecutorHandle {
    tx: mpsc::Sender<DccTask>,
    stats: Arc<ExecutorStats>,
}

impl DccExecutorHandle {
    /// Build a `DccExecutorHandle` from an externally-owned sender.
    ///
    /// Used by [`crate::host_bridge::dispatcher_to_executor_handle`]
    /// to bridge a portable [`dcc_mcp_host::DccDispatcher`] into the
    /// HTTP server's main-thread executor. `pub(crate)` keeps the
    /// module-private `tx` field invariant for normal callers while
    /// giving the bridge a single, documented seam.
    pub(crate) fn from_sender(tx: mpsc::Sender<DccTask>, capacity: usize) -> Self {
        Self {
            tx,
            stats: ExecutorStats::new(capacity, Duration::from_millis(2_000)),
        }
    }

    /// Current approximate queue depth (issue #715). Safe to call
    /// from any thread.
    pub fn pending(&self) -> usize {
        self.stats.pending()
    }

    /// Configured channel capacity (issue #715).
    pub fn capacity(&self) -> usize {
        self.stats.capacity
    }

    /// Wait-time of the oldest queued task (issue #715). `None` when
    /// the queue is empty.
    pub fn oldest_submit_age(&self) -> Option<Duration> {
        self.stats.oldest_wait()
    }

    /// Observability snapshot for diagnostics (issue #715).
    pub fn queue_stats(&self) -> ExecutorQueueStats {
        let (p50, p95, p99) = self.stats.percentiles();
        ExecutorQueueStats {
            pending: self.stats.pending(),
            capacity: self.stats.capacity,
            total_enqueued: self.stats.total_enqueued.load(Ordering::Acquire),
            total_dequeued: self.stats.total_dequeued.load(Ordering::Acquire),
            total_rejected: self.stats.total_rejected.load(Ordering::Acquire),
            oldest_wait_ms: self.stats.oldest_wait().map(|d| d.as_millis() as u64),
            wait_p50_ms: p50,
            wait_p95_ms: p95,
            wait_p99_ms: p99,
        }
    }
}

impl DccExecutorHandle {
    /// Submit a task to the DCC main thread and await its result.
    ///
    /// Backpressure semantics (issue #715): when the channel is at
    /// capacity, the caller blocks for up to the handle's configured
    /// send-timeout (default 2 s) waiting for the main thread to
    /// drain. If it still does not drain, the call returns
    /// [`crate::error::HttpError::QueueOverloaded`] — callers can
    /// distinguish this from [`crate::error::HttpError::ExecutorClosed`]
    /// and decide whether to retry or fail over.
    pub async fn execute(&self, func: DccTaskFn) -> Result<String, crate::error::HttpError> {
        let (result_tx, result_rx) = oneshot::channel();
        let submit_attempted_at = Instant::now();
        let timeout = self.stats.send_timeout;
        let send_res = if timeout.is_zero() {
            // Opt-out of backpressure: caller asked for no bound.
            self.tx
                .send(DccTask { func, result_tx })
                .await
                .map_err(|_| ())
        } else {
            match tokio::time::timeout(timeout, self.tx.send(DccTask { func, result_tx })).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err(()), // channel closed
                Err(_) => {
                    // Timed out waiting on a full channel. Canonical
                    // overload signal.
                    self.stats.record_reject();
                    return Err(crate::error::HttpError::QueueOverloaded {
                        depth: self.stats.pending(),
                        capacity: self.stats.capacity,
                        retry_after_secs: 1,
                    });
                }
            }
        };
        match send_res {
            Ok(()) => {
                self.stats.record_submit(submit_attempted_at);
            }
            Err(()) => {
                self.stats.record_reject();
                return Err(crate::error::HttpError::ExecutorClosed);
            }
        }

        result_rx
            .await
            .map_err(|_| crate::error::HttpError::ExecutorClosed)
    }

    /// Submit a cancellation-aware task to the DCC main thread (issue #332).
    ///
    /// The returned `oneshot::Receiver` resolves with `Ok(json_string)` once
    /// the task has run, or with an `Err(RecvError)` if the executor is
    /// dropped. The behaviour differs from [`Self::execute`] in three ways:
    ///
    /// 1. The submitted closure is wrapped with a **pre-execution
    ///    cancellation checkpoint** — if `cancel_token.is_cancelled()` when
    ///    the pump finally picks the request up, the user closure is NOT
    ///    invoked and the wrapper immediately surfaces
    ///    `{"__dispatch_error": "CANCELLED"}` to the receiver.
    /// 2. A **soft-fence tracing warning** is emitted when the wrapper
    ///    detects common main-thread pitfalls (see
    ///    [`warn_on_forbidden_patterns`]). Enforcement is out of scope —
    ///    skill authors are expected to fix the warning.
    /// 3. Callers can drive cancellation cooperatively by selecting on the
    ///    returned receiver alongside `cancel_token.cancelled()`.
    ///
    /// `tool_name` is purely for logging; pass the fully-qualified MCP tool
    /// name (`skill__action`).
    pub fn submit_deferred(
        &self,
        tool_name: &str,
        cancel_token: CancellationToken,
        func: DccTaskFn,
    ) -> oneshot::Receiver<String> {
        let (result_tx, result_rx) = oneshot::channel();
        let name_for_task = tool_name.to_string();
        let ct_for_task = cancel_token.clone();
        let wrapped: DccTaskFn = Box::new(move || {
            // Pre-execution checkpoint: drop the call if it was cancelled
            // while queued. Cheap, happens on main thread. Keeps the
            // wrapper interface uniform with `execute`.
            if ct_for_task.is_cancelled() {
                tracing::debug!(
                    tool = %name_for_task,
                    "deferred tool skipped — job cancelled before pump reached it"
                );
                return serde_json::to_string(&serde_json::json!({
                    "__dispatch_error": "CANCELLED"
                }))
                .unwrap_or_else(|_| "{\"__dispatch_error\":\"CANCELLED\"}".to_string());
            }
            let start = std::time::Instant::now();
            let out = (func)();
            let elapsed_ms = start.elapsed().as_millis();
            // Soft-fence: anything that runs > 1 frame @ 60 FPS on the main
            // thread will visibly stutter the DCC UI. We never panic on
            // this — just warn the author.
            if elapsed_ms > 50 {
                tracing::warn!(
                    tool = %name_for_task,
                    elapsed_ms,
                    "deferred tool spent > 50 ms on the DCC main thread — consider chunking \
                     (see docs/guide/dcc-thread-safety.md)"
                );
            }
            out
        });

        // Submit via a detached Tokio task so the caller doesn't need an
        // `.await`; cancelling while the mpsc is backed up drops the
        // request entirely without ever surfacing it to the pump.
        let tx = self.tx.clone();
        let stats = self.stats.clone();
        tokio::spawn(async move {
            let task = DccTask {
                func: wrapped,
                result_tx,
            };
            // Race `cancel_token.cancelled()` against `tx.send(task)`.
            tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    drop(task);
                }
                res = tx.reserve() => {
                    match res {
                        Ok(permit) => {
                            stats.record_submit(Instant::now());
                            permit.send(task);
                        }
                        Err(_) => {
                            stats.record_reject();
                            tracing::warn!(
                                "submit_deferred: DeferredExecutor mpsc closed"
                            );
                            drop(task);
                        }
                    }
                }
            }
        });
        result_rx
    }

    /// Yield a frame back to the DCC event loop (issue #332).
    ///
    /// Submits a no-op closure to the main-thread queue and awaits its
    /// completion. Long-running chunked jobs should call this between
    /// chunks so the DCC gets a chance to redraw the UI.
    ///
    /// ```text
    ///   for chunk in chunks:
    ///       do_scene_work(chunk)
    ///       handle.yield_frame().await          # UI redraws here
    /// ```
    ///
    /// Returns `Err` if the executor has been shut down.
    pub async fn yield_frame(&self) -> Result<(), crate::error::HttpError> {
        self.execute(Box::new(String::new)).await.map(|_| ())
    }
}

/// The DCC main-thread executor.
///
/// Call [`DeferredExecutor::poll_pending()`] from your DCC event loop
/// (e.g., Maya's `maya.utils.executeDeferred` callback, Blender's timer, etc.).
pub struct DeferredExecutor {
    rx: mpsc::Receiver<DccTask>,
    handle: DccExecutorHandle,
}

impl DeferredExecutor {
    /// Create a new executor with a bounded queue depth.
    ///
    /// The default send-timeout for backpressure is 2 s — use
    /// [`Self::with_send_timeout`] to override it.
    pub fn new(queue_depth: usize) -> Self {
        Self::with_send_timeout(queue_depth, Duration::from_millis(2_000))
    }

    /// Create a new executor with a bounded queue depth and a custom
    /// send-timeout for the backpressure path (issue #715).
    pub fn with_send_timeout(queue_depth: usize, send_timeout: Duration) -> Self {
        let (tx, rx) = mpsc::channel(queue_depth);
        let stats = ExecutorStats::new(queue_depth, send_timeout);
        Self {
            rx,
            handle: DccExecutorHandle { tx, stats },
        }
    }

    /// Get a cloneable handle for submitting tasks from Tokio workers.
    pub fn handle(&self) -> DccExecutorHandle {
        self.handle.clone()
    }

    /// Process **all currently queued** tasks synchronously on the calling thread.
    ///
    /// Call this from your DCC event loop. Returns the number of tasks processed.
    pub fn poll_pending(&mut self) -> usize {
        let mut count = 0;
        let stats = self.handle.stats.clone();
        while let Ok(task) = self.rx.try_recv() {
            stats.record_dequeue(Instant::now());
            let result = (task.func)();
            let _ = task.result_tx.send(result);
            count += 1;
        }
        count
    }

    /// Process at most `max` tasks. Useful to bound latency per tick.
    pub fn poll_pending_bounded(&mut self, max: usize) -> usize {
        let mut count = 0;
        let stats = self.handle.stats.clone();
        while count < max {
            if let Ok(task) = self.rx.try_recv() {
                stats.record_dequeue(Instant::now());
                let result = (task.func)();
                let _ = task.result_tx.send(result);
                count += 1;
            } else {
                break;
            }
        }
        count
    }
}

/// An in-process executor that runs tasks immediately on the calling thread.
///
/// Used for testing and non-DCC environments where no thread dispatch is needed.
#[derive(Clone)]
pub struct InProcessExecutor;

impl InProcessExecutor {
    /// Execute the task immediately on the current thread.
    pub fn execute(&self, func: DccTaskFn) -> String {
        func()
    }

    /// Wrap as a [`DccExecutorHandle`] backed by a dedicated Tokio task.
    pub fn into_handle(self) -> (DccExecutorHandle, Arc<tokio::task::JoinHandle<()>>) {
        let (tx, mut rx) = mpsc::channel::<DccTask>(256);
        let stats = ExecutorStats::new(256, Duration::from_millis(2_000));
        let drain_stats = stats.clone();
        let join = tokio::spawn(async move {
            while let Some(task) = rx.recv().await {
                drain_stats.record_dequeue(Instant::now());
                let result = (task.func)();
                let _ = task.result_tx.send(result);
            }
        });
        (DccExecutorHandle { tx, stats }, Arc::new(join))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    /// Drive `poll_pending_bounded(max)` on the calling thread until either
    /// `max_iterations` ticks have elapsed or `should_stop` returns true.
    /// Mirrors what a DCC idle pump would do.
    async fn pump_until<F: Fn() -> bool>(
        exec: &mut DeferredExecutor,
        should_stop: F,
        max_iterations: usize,
    ) {
        for _ in 0..max_iterations {
            exec.poll_pending_bounded(8);
            if should_stop() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn submit_deferred_runs_closure_on_pumped_thread() {
        let mut exec = DeferredExecutor::new(16);
        let handle = exec.handle();
        let ct = CancellationToken::new();

        let invoked = Arc::new(AtomicBool::new(false));
        let invoked_clone = invoked.clone();
        let rx = handle.submit_deferred(
            "test.tool",
            ct.clone(),
            Box::new(move || {
                invoked_clone.store(true, Ordering::SeqCst);
                "\"ok\"".to_string()
            }),
        );

        let done = Arc::new(AtomicBool::new(false));
        let done_clone = done.clone();
        tokio::spawn(async move {
            let out = rx.await.expect("receiver");
            assert_eq!(out, "\"ok\"");
            done_clone.store(true, Ordering::SeqCst);
        });

        pump_until(&mut exec, || done.load(Ordering::SeqCst), 40).await;
        assert!(invoked.load(Ordering::SeqCst), "closure ran");
        assert!(done.load(Ordering::SeqCst), "oneshot resolved");
    }

    #[tokio::test]
    async fn submit_deferred_skips_closure_when_cancelled_before_pump() {
        let mut exec = DeferredExecutor::new(16);
        let handle = exec.handle();
        let ct = CancellationToken::new();

        // Cancel BEFORE the pump ever runs — the wrapper's pre-check must
        // short-circuit and the user closure must NOT be invoked.
        ct.cancel();

        let user_closure_ran = Arc::new(AtomicBool::new(false));
        let flag = user_closure_ran.clone();
        let rx = handle.submit_deferred(
            "test.tool",
            ct.clone(),
            Box::new(move || {
                flag.store(true, Ordering::SeqCst);
                "\"should-not-run\"".to_string()
            }),
        );

        // Give the submitter task a chance to observe the cancellation and
        // decide between (a) not enqueuing at all or (b) enqueuing a wrapper
        // that short-circuits. Either outcome is correct — what matters is
        // that the user closure never runs.
        tokio::time::sleep(Duration::from_millis(20)).await;
        exec.poll_pending_bounded(8);

        // The oneshot either resolves with CANCELLED or is dropped.
        let res = tokio::time::timeout(Duration::from_millis(100), rx).await;
        assert!(
            !user_closure_ran.load(Ordering::SeqCst),
            "user closure must not run after cancel-before-pump"
        );
        match res {
            Ok(Ok(json_str)) => {
                assert!(
                    json_str.contains("CANCELLED"),
                    "wrapper must surface CANCELLED, got {json_str}"
                );
            }
            Ok(Err(_)) | Err(_) => {
                // Sender dropped — caller would observe this as a cancelled
                // oneshot and translate to CANCELLED externally.
            }
        }
    }

    #[tokio::test]
    async fn submit_deferred_runs_on_pump_thread_not_tokio_worker() {
        // Issue #332 acceptance — main-affined jobs must land on the thread
        // that drives the pump, never on a Tokio worker.
        let mut exec = DeferredExecutor::new(16);
        let handle = exec.handle();
        let ct = CancellationToken::new();

        let pump_thread_id = std::thread::current().id();
        let captured = Arc::new(parking_lot::Mutex::new(None::<std::thread::ThreadId>));
        let captured_clone = captured.clone();
        let rx = handle.submit_deferred(
            "test.main_only",
            ct.clone(),
            Box::new(move || {
                *captured_clone.lock() = Some(std::thread::current().id());
                "\"ok\"".to_string()
            }),
        );

        // Pump on THIS (test) thread; Tokio workers never touch the queue.
        let (done_tx, mut done_rx) = oneshot::channel();
        tokio::spawn(async move {
            let out = rx.await.expect("oneshot");
            let _ = done_tx.send(out);
        });
        let mut out = None;
        for _ in 0..50 {
            exec.poll_pending_bounded(8);
            if let Ok(o) = done_rx.try_recv() {
                out = Some(o);
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert!(out.is_some(), "submit_deferred returned");
        let observed = captured.lock().expect("closure recorded its thread id");
        assert_eq!(
            observed, pump_thread_id,
            "main-affined closure must execute on the pump thread"
        );
    }

    #[tokio::test]
    async fn yield_frame_returns_after_pump_tick() {
        let mut exec = DeferredExecutor::new(16);
        let handle = exec.handle();

        let tick_count = Arc::new(AtomicUsize::new(0));
        let yielded = Arc::new(AtomicBool::new(false));
        let yielded_clone = yielded.clone();

        tokio::spawn(async move {
            handle.yield_frame().await.expect("yield");
            yielded_clone.store(true, Ordering::SeqCst);
        });

        // Simulate the DCC event loop.
        for _ in 0..40 {
            let n = exec.poll_pending_bounded(8);
            tick_count.fetch_add(n, Ordering::SeqCst);
            if yielded.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert!(yielded.load(Ordering::SeqCst), "yield_frame completed");
        assert!(
            tick_count.load(Ordering::SeqCst) >= 1,
            "pump processed at least one task"
        );
    }

    /// Issue #715: saturating the executor channel without pumping
    /// surfaces a structured `QueueOverloaded` after the send-timeout
    /// — not a silent hang, and not `ExecutorClosed`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn execute_returns_queue_overloaded_on_saturation() {
        // Tiny channel + short send-timeout so the test stays fast.
        let _exec = DeferredExecutor::with_send_timeout(2, Duration::from_millis(50));
        let handle = _exec.handle();

        // Fill the channel: neither of these awaits resolves because
        // no one pumps. We don't await them here; we just let them
        // occupy the two slots.
        let h1 = handle.clone();
        let h2 = handle.clone();
        let _t1 = tokio::spawn(async move {
            let _ = h1.execute(Box::new(|| "one".to_string())).await;
        });
        let _t2 = tokio::spawn(async move {
            let _ = h2.execute(Box::new(|| "two".to_string())).await;
        });
        // Let them land in the channel.
        tokio::time::sleep(Duration::from_millis(20)).await;

        // The third submit should hit the send-timeout and bubble
        // QueueOverloaded.
        let err = handle
            .execute(Box::new(|| "three".to_string()))
            .await
            .expect_err("saturation must fail");
        match err {
            crate::error::HttpError::QueueOverloaded {
                depth,
                capacity,
                retry_after_secs,
            } => {
                assert_eq!(capacity, 2, "capacity reflects config");
                assert!(depth >= 1, "depth reported non-zero");
                assert!(retry_after_secs >= 1, "retry hint is non-zero");
            }
            other => panic!("expected QueueOverloaded, got {other:?}"),
        }
        let stats = handle.queue_stats();
        assert!(stats.total_rejected >= 1, "reject counter bumped");
        assert_eq!(stats.capacity, 2, "capacity reported in snapshot");
    }

    /// Issue #715: the queue-stats snapshot tracks enqueued/dequeued
    /// and reports a non-zero oldest-wait-age while jobs are parked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queue_stats_track_enqueue_dequeue_and_age() {
        let mut exec = DeferredExecutor::new(8);
        let handle = exec.handle();

        let h = handle.clone();
        let submitter = tokio::spawn(async move { h.execute(Box::new(|| "r".to_string())).await });
        // Let the submit land.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let pre = handle.queue_stats();
        assert_eq!(pre.total_enqueued, 1, "one submit recorded");
        assert_eq!(pre.total_dequeued, 0, "nothing drained yet");
        assert!(pre.oldest_wait_ms.unwrap_or(0) > 0, "wait-age reported");
        assert_eq!(pre.pending, 1);

        // Pump.
        let drained = exec.poll_pending();
        assert_eq!(drained, 1);
        let res = submitter.await.expect("join").expect("execute");
        assert_eq!(res, "r");

        let post = handle.queue_stats();
        assert_eq!(post.total_dequeued, 1, "drain recorded");
        assert_eq!(post.pending, 0);
        assert!(post.oldest_wait_ms.is_none(), "oldest cleared after drain");
        assert!(post.wait_p50_ms.is_some(), "p50 populated from sample");
    }
}
