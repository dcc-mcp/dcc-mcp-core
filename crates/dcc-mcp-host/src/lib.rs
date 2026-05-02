//! # dcc-mcp-host
//!
//! Cross-DCC main-thread dispatcher primitives.
//!
//! Almost every DCC Python API (Blender `bpy`, Maya `cmds`, Houdini `hou`,
//! 3ds Max MAXScript) must be invoked from the host application's
//! **main / UI thread** — calling from a worker thread typically crashes
//! or corrupts state. `dcc-mcp-host` provides the minimum trait + queue
//! abstraction so a Tokio-driven MCP HTTP server can safely route
//! `tools/call` into the DCC's main thread regardless of which DCC is
//! hosting us.
//!
//! ## Model
//!
//! 1. A tokio worker receives an incoming request and calls
//!    [`DccDispatcher::post`] with a boxed FnOnce closure.
//! 2. The dispatcher enqueues the job and returns a future that
//!    resolves once the job has executed.
//! 3. On the DCC's main thread, a native hook (Blender
//!    `bpy.app.timers.register`, Maya `executeDeferred`, Houdini
//!    `hou.ui.addEventLoopCallback`, 3ds Max `.NET Timer.onTick`,
//!    Unreal `AsyncTask(ENamedThreads::GameThread, …)`) repeatedly
//!    invokes [`DccDispatcher::tick`]. Each `tick` drains up to
//!    `max_jobs` entries from the queue, executing them **on the
//!    caller's thread** — which by construction is the DCC main
//!    thread.
//! 4. The result / panic / `DispatchError` flows back through a
//!    one-shot channel to the awaiting tokio future.
//!
//! ## Two concrete dispatchers
//!
//! * [`QueueDispatcher`] — default. Holds a standard mpsc queue and is
//!   driven by the host's native idle callback. Targets interactive
//!   mode (Blender GUI, Maya GUI, Houdini GUI, 3ds Max editor).
//! * [`BlockingDispatcher`] — wraps the same queue but exposes
//!   [`BlockingDispatcher::tick_blocking`] that sleeps on the
//!   receiver until a job arrives or the timeout expires. Targets
//!   headless mode (`blender --background`, `mayapy`, `hython`) where
//!   the host's idle callbacks are not firing.
//!
//! ## Blocking semantics
//!
//! `tick()` is synchronous and never blocks: it drains whatever is
//! currently in the queue and returns. `tick_blocking(timeout)` blocks
//! the caller for at most `timeout` waiting for the first job, then
//! drains.
//!
//! ## Ordering
//!
//! Jobs are executed in submission order (FIFO). Panics inside a job
//! are caught and surfaced to the caller's future as
//! [`DispatchError::Panic`] so they never poison the dispatcher.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

#[cfg(feature = "python-bindings")]
pub mod python;

/// Error surfaced to the awaiting tokio future when a posted job cannot
/// run to completion.
#[derive(Debug, Error)]
pub enum DispatchError {
    /// The dispatcher was already shut down when the job was posted, or
    /// it shut down before the tick loop could drain the job.
    #[error("dispatcher is shut down")]
    Shutdown,
    /// The result one-shot was dropped before the caller could observe
    /// the value. Usually means the caller cancelled the awaiting
    /// future before tick ran.
    #[error("job result channel was dropped before delivery")]
    ResultDropped,
    /// The job panicked inside `tick`. The payload is the captured
    /// panic message rendered with `format!("{payload:?}")`.
    #[error("job panicked on the main thread: {0}")]
    Panic(String),
}

/// Outcome of a single [`DccDispatcher::tick`] call.
///
/// Consumers use `jobs_executed` to decide whether to keep polling
/// aggressively or back off (e.g. switch the Blender timer from
/// `TIMER_INTERVAL_ACTIVE` to `TIMER_INTERVAL_IDLE`).
#[derive(Debug, Clone, Copy, Default)]
pub struct TickOutcome {
    /// Number of jobs that were drained and executed in this tick.
    pub jobs_executed: usize,
    /// Number of jobs that panicked (already surfaced as
    /// [`DispatchError::Panic`] to their callers).
    pub jobs_panicked: usize,
    /// `true` when the queue still had more jobs than `max_jobs` allowed.
    /// The caller should tick again soon.
    pub more_pending: bool,
}

/// Abstract interface for posting and draining main-thread jobs.
///
/// Every DCC adapter in this workspace wires its native idle primitive
/// to call [`DccDispatcher::tick`]. The MCP HTTP server layer calls
/// [`DccDispatcher::post`] (via the [`DccDispatcherExt`] convenience
/// extension) from its tokio workers.
///
/// # Dyn compatibility
///
/// This trait is intentionally dyn-compatible (no generic methods in
/// the main surface) so consumers can hold `Arc<dyn DccDispatcher>`.
/// The ergonomic generic `post<F, R>(job)` lives on
/// [`DccDispatcherExt`], which is blanket-implemented for every
/// `DccDispatcher` and available whenever that trait is in scope.
pub trait DccDispatcher: Send + Sync + 'static {
    /// Enqueue a type-erased job for main-thread execution and return
    /// a future that resolves with the job's boxed result.
    ///
    /// This is the dyn-safe primitive: every other `post*` method
    /// (including [`DccDispatcherExt::post`]) funnels through here.
    /// Callers who just want to schedule a closure and await a typed
    /// result should use the generic
    /// [`DccDispatcherExt::post`] instead — that wraps this method
    /// and handles the down-cast.
    fn post_boxed(&self, job: BoxedJob) -> PostHandle<BoxedResult>;

    /// Drain at most `max_jobs` entries from the queue and run them
    /// **on the calling thread**.
    ///
    /// Implementations must be callable only from the host's main
    /// thread; passing a value from a worker thread is a logic bug
    /// (it will work but defeats the point of the dispatcher). The
    /// returned [`TickOutcome`] tells the caller how much work was
    /// done so it can adapt its polling interval.
    fn tick(&self, max_jobs: usize) -> TickOutcome;

    /// `true` when at least one job is waiting in the queue.
    fn has_pending(&self) -> bool;

    /// Approximate queue depth. Useful for metrics and adaptive
    /// throttling; not a strict guarantee under concurrent posts.
    fn pending(&self) -> usize;

    /// Signal to all current and future posters that no further work
    /// will be accepted. Pending jobs that have not yet been drained
    /// are dropped and their callers receive
    /// [`DispatchError::Shutdown`].
    fn shutdown(&self);

    /// `true` once [`DccDispatcher::shutdown`] has been called.
    fn is_shutdown(&self) -> bool;
}

/// Type-erased closure the dispatcher stores in its queue.
pub type BoxedJob = Box<dyn FnOnce() -> BoxedResult + Send + 'static>;

/// Type-erased return value shipped back via [`PostHandle`]. The
/// generic [`DccDispatcherExt::post`] downcasts this transparently.
pub type BoxedResult = Box<dyn std::any::Any + Send + 'static>;

/// Convenience extension giving every [`DccDispatcher`] the ergonomic
/// generic `post<F, R>(job)` API. Blanket-implemented — callers don't
/// need to implement this trait themselves.
///
/// Keeping this out of the core trait is what makes
/// [`DccDispatcher`] dyn-compatible.
pub trait DccDispatcherExt: DccDispatcher {
    /// Enqueue a `FnOnce() -> R` for main-thread execution and return
    /// a future that resolves to `R` directly (no boxing visible at
    /// the call site).
    fn post<F, R>(&self, job: F) -> PostHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let boxed_job: BoxedJob = Box::new(move || {
            let r = job();
            Box::new(r) as BoxedResult
        });
        let raw = self.post_boxed(boxed_job);
        PostHandle::downcasting::<R>(raw)
    }
}

impl<T: DccDispatcher + ?Sized> DccDispatcherExt for T {}

/// Handle to a posted job. Awaiting it yields the job's return value
/// once the main thread ticks, or [`DispatchError`] on failure.
pub struct PostHandle<R> {
    inner: oneshot::Receiver<Result<R, DispatchError>>,
}

impl<R> PostHandle<R> {
    fn new(rx: oneshot::Receiver<Result<R, DispatchError>>) -> Self {
        Self { inner: rx }
    }
}

impl PostHandle<BoxedResult> {
    /// Convert a type-erased [`PostHandle<BoxedResult>`] (as produced
    /// by [`DccDispatcher::post_boxed`]) into a typed
    /// [`PostHandle<R>`] by attaching an adapter task that performs
    /// the runtime downcast.
    ///
    /// Used by [`DccDispatcherExt::post`] so users holding
    /// `Arc<dyn DccDispatcher>` still get the ergonomic typed
    /// return. The conversion is infallible in practice — the
    /// extension trait is the only way to feed data in, and it
    /// guarantees the boxed payload is `R` — but we still surface a
    /// [`DispatchError::ResultDropped`] if the adapter channel is
    /// torn down.
    fn downcasting<R>(self) -> PostHandle<R>
    where
        R: Send + 'static,
    {
        let (tx, rx) = oneshot::channel::<Result<R, DispatchError>>();
        // Spawn an adapter task only if we're inside a tokio runtime.
        // When called from inside `DccDispatcherExt::post`, the caller
        // is typically on a tokio worker (HTTP hot path) or inside a
        // `tokio::runtime::Handle::block_on` (Python bindings), so a
        // runtime is almost always present. In the rare non-runtime
        // case we fall back to `std::thread::spawn` so the handle
        // still resolves correctly.
        let inner = self.inner;
        let forward = async move {
            let outcome = match inner.await {
                Ok(Ok(boxed)) => match boxed.downcast::<R>() {
                    Ok(concrete) => Ok(*concrete),
                    Err(_) => Err(DispatchError::Panic(
                        "post_boxed payload downcast failed — internal bug".to_string(),
                    )),
                },
                Ok(Err(err)) => Err(err),
                Err(_) => Err(DispatchError::ResultDropped),
            };
            let _ = tx.send(outcome);
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(forward);
            }
            Err(_) => {
                std::thread::spawn(move || {
                    // Minimal current-thread runtime for the adapter.
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_time()
                        .build()
                        .expect("failed to build fallback runtime for PostHandle downcast");
                    rt.block_on(forward);
                });
            }
        }
        PostHandle::new(rx)
    }
}

impl<R> Future for PostHandle<R> {
    type Output = Result<R, DispatchError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::task::Poll;
        match std::pin::Pin::new(&mut self.inner).poll(cx) {
            Poll::Pending => Poll::Pending,
            // The sender closed without delivering — either shutdown
            // dropped the queue or the tick thread dropped the sender.
            // Surface a stable error so callers don't get an anonymous
            // `RecvError`.
            Poll::Ready(Err(_)) => Poll::Ready(Err(DispatchError::ResultDropped)),
            Poll::Ready(Ok(v)) => Poll::Ready(v),
        }
    }
}

// ── Internal job envelope ───────────────────────────────────────────

/// A type-erased job that can be invoked on the main thread.
///
/// Each submitted `FnOnce() -> R` closure is wrapped by `QueueDispatcher::post`
/// into this trait object that owns its own result channel, so the
/// dispatcher can drain heterogeneous return types through a single
/// queue.
trait Runnable: Send {
    /// Invoke the wrapped closure. Any panic is caught and reported
    /// to the originating [`PostHandle`].
    fn run(self: Box<Self>) -> bool;
    /// Report shutdown to the awaiting caller without executing the job.
    fn cancel(self: Box<Self>);
}

struct Job<F, R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    func: Option<F>,
    result_tx: Option<oneshot::Sender<Result<R, DispatchError>>>,
}

impl<F, R> Runnable for Job<F, R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    fn run(mut self: Box<Self>) -> bool {
        let Some(func) = self.func.take() else {
            return false;
        };

        install_panic_hook_once();

        // AssertUnwindSafe: we own the FnOnce and never use it again
        // after the panic; any captured state that needed unwind
        // safety is the caller's responsibility.
        let outcome = catch_unwind(AssertUnwindSafe(func));

        let Some(tx) = self.result_tx.take() else {
            // No one is listening — drop the result silently.
            return outcome.is_err();
        };
        match outcome {
            Ok(value) => {
                let _ = tx.send(Ok(value));
                false
            }
            Err(_) => {
                let msg = LAST_PANIC
                    .with(|slot| slot.borrow_mut().take())
                    .unwrap_or_else(|| "<panic without captured message>".to_string());
                let _ = tx.send(Err(DispatchError::Panic(msg)));
                true
            }
        }
    }

    fn cancel(mut self: Box<Self>) {
        if let Some(tx) = self.result_tx.take() {
            let _ = tx.send(Err(DispatchError::Shutdown));
        }
    }
}

// Captures the message of the *most recent* panic on the current
// thread. `Runnable::run` reads and clears this slot inside its
// `catch_unwind` error branch.
std::thread_local! {
    static LAST_PANIC: std::cell::RefCell<Option<String>>
        = const { std::cell::RefCell::new(None) };
}

/// Install a process-wide panic hook that records each panic message
/// in the thread-local [`LAST_PANIC`] slot. Called once by the first
/// `Runnable::run`. We chain to the previous hook so the default
/// "thread '<name>' panicked at …" line still prints to stderr —
/// that's useful operator signal and matches user expectations.
fn install_panic_hook_once() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let payload = info.payload();
            let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = payload.downcast_ref::<Box<str>>() {
                s.to_string()
            } else {
                // Unknown payload shape — use the default formatter
                // (which internally understands `PanicMessage` etc.)
                // by formatting `info` as Display.
                format!("{info}")
            };
            let located = match info.location() {
                Some(loc) => format!("{msg} at {}:{}:{}", loc.file(), loc.line(), loc.column()),
                None => msg,
            };
            LAST_PANIC.with(|slot| *slot.borrow_mut() = Some(located));
            prev(info);
        }));
    });
}

// ── Shared queue state ──────────────────────────────────────────────

struct Shared {
    /// Sender half. Cloned into every [`QueueDispatcher::post`] call.
    tx: mpsc::UnboundedSender<Box<dyn Runnable>>,
    /// Receiver half. Wrapped in a tokio mutex so both the sync
    /// `tick()` path (via [`tokio::sync::Mutex::try_lock`]) and the
    /// async `drain_awaiting` path (via `.lock().await`) can share
    /// exclusive drain access without the
    /// `clippy::await_holding_lock` footgun that `parking_lot` brings.
    rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Box<dyn Runnable>>>,
    pending: AtomicUsize,
    shutdown: AtomicBool,
}

impl Shared {
    fn new() -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel::<Box<dyn Runnable>>();
        Arc::new(Self {
            tx,
            rx: tokio::sync::Mutex::new(rx),
            pending: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
        })
    }

    fn enqueue(&self, job: Box<dyn Runnable>) -> Result<(), Box<dyn Runnable>> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(job);
        }
        match self.tx.send(job) {
            Ok(()) => {
                self.pending.fetch_add(1, Ordering::Release);
                Ok(())
            }
            Err(mpsc::error::SendError(job)) => Err(job),
        }
    }

    /// Drain up to `max_jobs` currently-ready items from the queue.
    /// Returns `(drained, more_pending)`.
    ///
    /// Uses `try_lock` because `tick()` is synchronous — if the async
    /// `drain_awaiting` branch is already holding the receiver, the
    /// drain is deferred to the next call rather than blocking the
    /// DCC main thread.
    fn drain_ready(&self, max_jobs: usize) -> (Vec<Box<dyn Runnable>>, bool) {
        let mut out = Vec::with_capacity(max_jobs.min(16));
        let Ok(mut rx) = self.rx.try_lock() else {
            // Somebody else is in `drain_awaiting`; let them continue.
            // We'll catch up on the next tick.
            return (out, true);
        };
        for _ in 0..max_jobs {
            match rx.try_recv() {
                Ok(job) => out.push(job),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
        let more = match rx.try_recv() {
            Ok(job) => {
                // We peeked by popping — put it back at the front of
                // our batch so ordering is preserved.
                out.push(job);
                true
            }
            Err(_) => false,
        };
        // The `more` peek may have made the batch one bigger than
        // `max_jobs`. That's acceptable — we promise *at most*
        // `max_jobs + 1` on the boundary, and callers use `max_jobs`
        // as a soft cap for fairness rather than a hard limit.
        self.pending.fetch_sub(out.len(), Ordering::Release);
        (out, more)
    }

    /// Block until a job is available or `timeout` elapses; then drain
    /// up to `max_jobs`.
    async fn drain_awaiting(
        &self,
        max_jobs: usize,
        timeout: Duration,
    ) -> (Vec<Box<dyn Runnable>>, bool) {
        let mut rx = self.rx.lock().await;
        // Wait for the first job with a bounded timeout.
        let first = match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(job)) => Some(job),
            Ok(None) | Err(_) => None,
        };
        let Some(first) = first else {
            return (Vec::new(), false);
        };
        let mut out = Vec::with_capacity(max_jobs.min(16));
        out.push(first);
        // Drain any extra items that happen to be ready without blocking.
        for _ in 1..max_jobs {
            match rx.try_recv() {
                Ok(job) => out.push(job),
                Err(_) => break,
            }
        }
        let more = match rx.try_recv() {
            Ok(job) => {
                out.push(job);
                true
            }
            Err(_) => false,
        };
        self.pending.fetch_sub(out.len(), Ordering::Release);
        (out, more)
    }

    fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        // Drain and cancel everything still queued so posters don't
        // block indefinitely awaiting their result channels.
        //
        // We avoid `blocking_lock` because shutdown can legitimately
        // be called from within a tokio runtime (e.g. an HTTP handler
        // that tears down the server mid-request). Instead, spin with
        // `try_lock`: the receiver is only held during a drain batch,
        // so contention is brief and bounded.
        let mut attempts = 0;
        let mut rx = loop {
            match self.rx.try_lock() {
                Ok(guard) => break guard,
                Err(_) => {
                    attempts += 1;
                    if attempts > 1000 {
                        // The lock is stuck — the active drain will
                        // eventually observe `shutdown` and stop
                        // enqueuing, and all currently-ticked jobs
                        // will see `shutdown` too. Not being able to
                        // cancel an in-flight drain is acceptable
                        // because those jobs will run (or have run)
                        // and their results will flow back normally.
                        tracing::warn!(
                            "dcc-mcp-host: shutdown gave up waiting for drain lock after 1000 spins"
                        );
                        return;
                    }
                    std::thread::yield_now();
                }
            }
        };
        while let Ok(job) = rx.try_recv() {
            job.cancel();
            self.pending.fetch_sub(1, Ordering::Release);
        }
    }
}

// ── QueueDispatcher: interactive-mode implementation ────────────────

/// Default dispatcher for DCCs that run an idle callback
/// (Blender `bpy.app.timers`, Houdini `hou.ui.addEventLoopCallback`,
/// etc.).
///
/// `post` is cheap and thread-safe. `tick` is non-blocking and must
/// be called from the DCC's main thread.
#[derive(Clone)]
pub struct QueueDispatcher {
    shared: Arc<Shared>,
}

impl Default for QueueDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueDispatcher {
    /// Construct a fresh dispatcher with an empty queue.
    pub fn new() -> Self {
        Self {
            shared: Shared::new(),
        }
    }

    /// Inherent generic `post<F, R>` — the fast path when the caller
    /// holds a concrete `QueueDispatcher`.
    ///
    /// Avoids the double-box the dyn-safe
    /// [`DccDispatcher::post_boxed`] path incurs: the closure and its
    /// return type are preserved statically until the tick thread
    /// runs them. The ergonomic
    /// [`DccDispatcherExt::post`] method provides the same signature
    /// when the caller holds an `Arc<dyn DccDispatcher>`.
    pub fn post<F, R>(&self, job: F) -> PostHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = oneshot::channel::<Result<R, DispatchError>>();
        let boxed: Box<dyn Runnable> = Box::new(Job::<F, R> {
            func: Some(job),
            result_tx: Some(tx),
        });
        if let Err(rejected) = self.shared.enqueue(boxed) {
            rejected.cancel();
        }
        PostHandle::new(rx)
    }
}

impl DccDispatcher for QueueDispatcher {
    fn post_boxed(&self, job: BoxedJob) -> PostHandle<BoxedResult> {
        // Dyn-safe primitive: the generic closure shape has already
        // been erased by the caller, so we just stash the boxed job
        // in the same `Runnable` plumbing the inherent `post` uses.
        self.post(job)
    }

    fn tick(&self, max_jobs: usize) -> TickOutcome {
        if max_jobs == 0 {
            return TickOutcome::default();
        }
        let (batch, more) = self.shared.drain_ready(max_jobs);
        let mut outcome = TickOutcome {
            jobs_executed: 0,
            jobs_panicked: 0,
            more_pending: more,
        };
        for job in batch {
            let panicked = job.run();
            outcome.jobs_executed += 1;
            if panicked {
                outcome.jobs_panicked += 1;
            }
        }
        outcome
    }

    fn has_pending(&self) -> bool {
        self.shared.pending.load(Ordering::Acquire) > 0
    }

    fn pending(&self) -> usize {
        self.shared.pending.load(Ordering::Acquire)
    }

    fn shutdown(&self) {
        self.shared.shutdown();
    }

    fn is_shutdown(&self) -> bool {
        self.shared.shutdown.load(Ordering::Acquire)
    }
}

// ── BlockingDispatcher: headless-mode wrapper ───────────────────────

/// Dispatcher for headless DCCs (`blender --background`, `mayapy`,
/// `hython`) where the host does not run an idle callback.
///
/// Wraps a [`QueueDispatcher`] and exposes
/// [`BlockingDispatcher::tick_blocking`] which sleeps on the
/// underlying mpsc receiver with a bounded timeout. Use this from a
/// tight loop in the host's main thread.
#[derive(Clone)]
pub struct BlockingDispatcher {
    inner: QueueDispatcher,
}

impl Default for BlockingDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockingDispatcher {
    /// Construct a fresh headless dispatcher.
    pub fn new() -> Self {
        Self {
            inner: QueueDispatcher::new(),
        }
    }

    /// Inherent generic `post<F, R>` — see
    /// [`QueueDispatcher::post`] for why we keep this outside the
    /// trait.
    pub fn post<F, R>(&self, job: F) -> PostHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.post(job)
    }

    /// Drain up to `max_jobs` from the queue, blocking up to `timeout`
    /// waiting for the first job if none are immediately available.
    ///
    /// Returns once either a drain has happened or the timeout
    /// elapses. Callers loop on this method in the DCC's main thread.
    pub async fn tick_blocking(&self, max_jobs: usize, timeout: Duration) -> TickOutcome {
        if max_jobs == 0 {
            return TickOutcome::default();
        }
        // Fast path — any items already present are returned without
        // waiting.
        if self.inner.shared.pending.load(Ordering::Acquire) > 0 {
            return self.inner.tick(max_jobs);
        }
        let (batch, more) = self.inner.shared.drain_awaiting(max_jobs, timeout).await;
        let mut outcome = TickOutcome {
            jobs_executed: 0,
            jobs_panicked: 0,
            more_pending: more,
        };
        for job in batch {
            let panicked = job.run();
            outcome.jobs_executed += 1;
            if panicked {
                outcome.jobs_panicked += 1;
            }
        }
        outcome
    }
}

impl DccDispatcher for BlockingDispatcher {
    fn post_boxed(&self, job: BoxedJob) -> PostHandle<BoxedResult> {
        self.inner.post_boxed(job)
    }

    fn tick(&self, max_jobs: usize) -> TickOutcome {
        self.inner.tick(max_jobs)
    }

    fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }

    fn pending(&self) -> usize {
        self.inner.pending()
    }

    fn shutdown(&self) {
        self.inner.shutdown()
    }

    fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A bare post-then-tick round-trip on a QueueDispatcher returns
    /// the job's value to the awaiting future.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_then_tick_returns_job_result() {
        let d = Arc::new(QueueDispatcher::new());
        let handle = d.post(|| 42_u32);
        // Simulate the "main thread" by running tick on a blocking task
        // so `handle.await` below sees the result asynchronously.
        let d_tick = d.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            d_tick.tick(16);
        });
        let got = handle.await.expect("job succeeds");
        assert_eq!(got, 42);
        assert!(!d.has_pending());
    }

    /// Jobs run on the tick caller's thread — proves the main-thread
    /// contract. Captures the thread id inside the job and asserts
    /// it matches the tick thread, not the post thread.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn jobs_run_on_tick_thread() {
        let d = Arc::new(QueueDispatcher::new());
        let tick_thread = std::thread::current().id();
        let job_thread = Arc::new(parking_lot::Mutex::new(None));
        let jt = job_thread.clone();
        let handle = d.post(move || {
            *jt.lock() = Some(std::thread::current().id());
        });
        // Drive tick from a separate OS thread so we can prove the job
        // runs on *that* thread, not the post caller's.
        let d_tick = d.clone();
        let expected = std::thread::spawn(move || {
            let id = std::thread::current().id();
            // Wait briefly for the post to arrive before ticking.
            std::thread::sleep(Duration::from_millis(5));
            d_tick.tick(16);
            id
        });
        handle.await.expect("job succeeds");
        let tick_owner = expected.join().unwrap();
        assert_ne!(
            tick_thread, tick_owner,
            "sanity: test runner thread != spawned"
        );
        assert_eq!(*job_thread.lock(), Some(tick_owner));
    }

    /// Ordering: jobs execute in FIFO submission order within a single
    /// tick.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn jobs_execute_in_fifo_order() {
        let d = Arc::new(QueueDispatcher::new());
        let log = Arc::new(parking_lot::Mutex::new(Vec::<u32>::new()));
        let mut handles = Vec::new();
        for i in 0..10 {
            let log = log.clone();
            handles.push(d.post(move || {
                log.lock().push(i);
            }));
        }
        let d_tick = d.clone();
        std::thread::spawn(move || d_tick.tick(64)).join().unwrap();
        for h in handles {
            h.await.expect("job succeeds");
        }
        assert_eq!(*log.lock(), (0..10).collect::<Vec<_>>());
    }

    /// Concurrent posters stress-test: 32 tokio workers each post 100
    /// jobs; all 3200 must flow through without loss or deadlock and
    /// the total observed value matches the sum 0..3200.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_posters_do_not_deadlock_or_lose_jobs() {
        let d = Arc::new(QueueDispatcher::new());
        let total = Arc::new(AtomicU64::new(0));
        let mut handles = Vec::new();
        const WORKERS: u64 = 32;
        const PER_WORKER: u64 = 100;
        for w in 0..WORKERS {
            let d = d.clone();
            let total = total.clone();
            handles.push(tokio::spawn(async move {
                let base = w * PER_WORKER;
                let mut hs = Vec::new();
                for i in 0..PER_WORKER {
                    let total = total.clone();
                    let value = base + i;
                    hs.push(d.post(move || {
                        total.fetch_add(value, Ordering::Relaxed);
                    }));
                }
                for h in hs {
                    h.await.expect("job succeeds");
                }
            }));
        }
        // Run tick from a dedicated "main thread" until everything is
        // drained and posters have returned.
        let d_tick = d.clone();
        let ticker = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                let out = d_tick.tick(128);
                if !out.more_pending && !d_tick.has_pending() {
                    // Spin a couple of extra times to let any in-flight
                    // posters settle; break once two consecutive idle
                    // ticks happen.
                    std::thread::sleep(Duration::from_millis(1));
                    let again = d_tick.tick(128);
                    if again.jobs_executed == 0 && !d_tick.has_pending() {
                        break;
                    }
                }
                if std::time::Instant::now() > deadline {
                    panic!("ticker deadline exceeded — queue drained incompletely");
                }
            }
        });
        for h in handles {
            h.await.expect("poster task succeeds");
        }
        ticker.join().unwrap();
        let expected: u64 = (0..WORKERS * PER_WORKER).sum();
        assert_eq!(total.load(Ordering::Relaxed), expected);
    }

    /// A panic inside a job becomes `DispatchError::Panic` on the
    /// caller's future, and subsequent jobs still run.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panics_are_caught_and_surfaced_as_dispatch_error() {
        let d = Arc::new(QueueDispatcher::new());
        let boom: PostHandle<()> = d.post(|| panic!("boom on main thread"));
        let ok = d.post(|| "still alive");
        let d_tick = d.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            d_tick.tick(16);
        });
        let err = boom.await.unwrap_err();
        match err {
            DispatchError::Panic(msg) => {
                assert!(msg.contains("boom"), "panic payload unexpected: {msg:?}")
            }
            other => panic!("expected Panic, got {other:?}"),
        }
        assert_eq!(ok.await.expect("second job runs"), "still alive");
    }

    /// Shutdown cancels pending jobs and rejects new posts.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_cancels_pending_and_rejects_new_posts() {
        let d = Arc::new(QueueDispatcher::new());
        let pending: PostHandle<i32> = d.post(|| 1);
        d.shutdown();
        assert!(d.is_shutdown());
        // Pending job now resolves to Shutdown without running.
        let err = pending.await.unwrap_err();
        assert!(matches!(err, DispatchError::Shutdown));
        // New posts also reject immediately.
        let rejected: PostHandle<i32> = d.post(|| 2);
        let err = rejected.await.unwrap_err();
        assert!(matches!(err, DispatchError::Shutdown));
    }

    /// `tick(max_jobs)` respects the fairness cap and reports
    /// `more_pending=true` so the caller knows to tick again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tick_respects_max_jobs_and_reports_more_pending() {
        let d = Arc::new(QueueDispatcher::new());
        let mut handles = Vec::new();
        for _ in 0..10 {
            handles.push(d.post(|| 1_u32));
        }
        // Cap at 3 — expect 3 executed (plus at most one peek job)
        // and more_pending=true.
        let outcome = d.tick(3);
        assert!(outcome.jobs_executed >= 3 && outcome.jobs_executed <= 4);
        assert!(outcome.more_pending);
        // Drain the rest so awaits resolve cleanly.
        while d.has_pending() {
            d.tick(64);
        }
        for h in handles {
            h.await.expect("job drained");
        }
    }

    /// `BlockingDispatcher::tick_blocking` sleeps until a post arrives,
    /// then drains and returns.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_dispatcher_wakes_on_post() {
        let d = Arc::new(BlockingDispatcher::new());
        let d_post = d.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = d_post.post(|| ()).await;
        });
        let started = std::time::Instant::now();
        let outcome = d.tick_blocking(16, Duration::from_millis(500)).await;
        let elapsed = started.elapsed();
        assert_eq!(outcome.jobs_executed, 1);
        assert!(
            elapsed < Duration::from_millis(400),
            "woke too late: {elapsed:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(15),
            "woke too early: {elapsed:?}"
        );
    }

    /// `BlockingDispatcher::tick_blocking` returns an empty outcome
    /// after the timeout when no job arrives.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_dispatcher_timeout_returns_empty() {
        let d = BlockingDispatcher::new();
        let started = std::time::Instant::now();
        let outcome = d.tick_blocking(16, Duration::from_millis(30)).await;
        let elapsed = started.elapsed();
        assert_eq!(outcome.jobs_executed, 0);
        assert!(!outcome.more_pending);
        assert!(elapsed >= Duration::from_millis(25));
    }

    /// `post` is safe to call from any thread (poster need not be the
    /// tick thread or the tokio runtime thread).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_from_non_tokio_thread() {
        let d = Arc::new(QueueDispatcher::new());
        let d_post = d.clone();
        let handle = std::thread::spawn(move || d_post.post(|| 7_u32));
        let post_handle = handle.join().unwrap();
        let d_tick = d.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            d_tick.tick(16);
        });
        assert_eq!(post_handle.await.unwrap(), 7);
    }

    /// Sanity: after shutdown the queue reports empty and `pending()`
    /// returns 0 even under a prior load.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_resets_pending_to_zero() {
        let d = Arc::new(QueueDispatcher::new());
        let mut handles = Vec::new();
        for _ in 0..5 {
            handles.push(d.post(|| ()));
        }
        assert!(d.pending() > 0);
        d.shutdown();
        for h in handles {
            // All should resolve to Shutdown.
            assert!(matches!(h.await, Err(DispatchError::Shutdown)));
        }
        assert_eq!(d.pending(), 0);
    }
}
