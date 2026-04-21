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

use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// A boxed async-compatible task that runs on the DCC main thread.
///
/// Returns a JSON string result (or an error string).
pub type DccTaskFn = Box<dyn FnOnce() -> String + Send + 'static>;

/// A pending DCC task with its result channel.
struct DccTask {
    func: DccTaskFn,
    result_tx: oneshot::Sender<String>,
}

/// Handle owned by the HTTP server to submit tasks to the DCC main thread.
#[derive(Clone)]
pub struct DccExecutorHandle {
    tx: mpsc::Sender<DccTask>,
}

impl DccExecutorHandle {
    /// Submit a task to the DCC main thread and await its result.
    ///
    /// Returns `Err` if the DCC executor has been shut down.
    pub async fn execute(&self, func: DccTaskFn) -> Result<String, crate::error::HttpError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.tx
            .send(DccTask { func, result_tx })
            .await
            .map_err(|_| crate::error::HttpError::ExecutorClosed)?;

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
        tokio::spawn(async move {
            let task = DccTask {
                func: wrapped,
                result_tx,
            };
            // Race `cancel_token.cancelled()` against `tx.send(task)`.
            // Select would require moving `task` into the send branch; a
            // two-step await is simpler and equally correct because the
            // mpsc send is the only branch that owns the task.
            tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    // The wrapper owns its own `result_tx`; dropping the
                    // DccTask here drops that sender and the receiver
                    // observes `RecvError`. Caller selects on
                    // `cancel_token.cancelled()` to translate this into a
                    // proper CANCELLED outcome.
                    drop(task);
                }
                res = tx.reserve() => {
                    match res {
                        Ok(permit) => permit.send(task),
                        Err(_) => {
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
    pub fn new(queue_depth: usize) -> Self {
        let (tx, rx) = mpsc::channel(queue_depth);
        Self {
            rx,
            handle: DccExecutorHandle { tx },
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
        while let Ok(task) = self.rx.try_recv() {
            let result = (task.func)();
            let _ = task.result_tx.send(result);
            count += 1;
        }
        count
    }

    /// Process at most `max` tasks. Useful to bound latency per tick.
    pub fn poll_pending_bounded(&mut self, max: usize) -> usize {
        let mut count = 0;
        while count < max {
            if let Ok(task) = self.rx.try_recv() {
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
        let join = tokio::spawn(async move {
            while let Some(task) = rx.recv().await {
                let result = (task.func)();
                let _ = task.result_tx.send(result);
            }
        });
        (DccExecutorHandle { tx }, Arc::new(join))
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
}
