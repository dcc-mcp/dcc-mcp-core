//! PyO3 bindings for [`crate::DccDispatcher`] and its two built-in
//! implementations.
//!
//! Exposes:
//!
//! * `DispatchError` — Python exception class.
//! * `PyTickOutcome` — frozen dataclass-like wrapper over
//!   [`crate::TickOutcome`].
//! * `PyPostHandle` — blocking-wait future returned by
//!   `PyQueueDispatcher.post` / `PyBlockingDispatcher.post`.
//! * `PyQueueDispatcher` — default interactive-mode dispatcher.
//! * `PyBlockingDispatcher` — headless-mode dispatcher with
//!   `tick_blocking(max_jobs, timeout_ms)`.
//!
//! ## Contract
//!
//! * `post(callable)` accepts any zero-arg Python callable. The
//!   callable is invoked on whichever thread calls `tick()` / drains
//!   `tick_blocking`; that is, the DCC's main thread in production.
//!   This is the whole point of the dispatcher.
//! * The callable's return value is shipped back to `PyPostHandle.wait`
//!   as a Python object. Python exceptions raised by the callable are
//!   caught and turned into [`DispatchError`] on `wait`.
//! * `wait(timeout=None)` blocks the *calling* thread (the tokio
//!   request handler or the test thread, not the tick thread).
//!   `timeout` is in seconds; `None` waits forever. A timeout raises
//!   [`DispatchError`] with a message that starts with "timeout".
//! * Rust panics inside the callable are caught by the library's
//!   thread-local panic hook (installed once per process by
//!   [`crate::install_panic_hook_once`]) and surface to `wait` as
//!   [`DispatchError`] starting with "panic".

use std::sync::Arc;
use std::time::Duration;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyTuple};
use tokio::runtime::Runtime;

use crate::{
    BlockingDispatcher, DccDispatcher, DispatchError, PostHandle, QueueDispatcher, TickOutcome,
};

/// Alias for the owned-GIL-free Python object handle used across this
/// module. pyo3 0.28 dropped the `PyObject` alias; we reintroduce a
/// local one to keep signatures readable.
type PyObj = Py<PyAny>;

// ── Shared tokio runtime ────────────────────────────────────────────
//
// `PostHandle` is a `tokio::sync::oneshot::Receiver` future. The
// Python side must drive it from non-async code, so we need a
// dedicated tokio runtime to block on futures when callers invoke
// `wait()`. The runtime is created on first use and shared across all
// dispatcher instances; a single multi-threaded runtime is fine since
// everything the Python binding does is short-lived I/O coordination.
fn shared_runtime() -> &'static Runtime {
    use std::sync::OnceLock;
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("dcc-mcp-host-py")
            .worker_threads(2)
            .build()
            .expect("failed to build tokio runtime for dcc-mcp-host python bindings")
    })
}

// ── DispatchError → Python exception ───────────────────────────────

pyo3::create_exception!(
    dcc_mcp_host,
    DispatchErrorPy,
    pyo3::exceptions::PyException,
    "Raised when a posted job fails: dispatcher shut down, job panicked, \
     Python callable raised, or the wait timed out."
);

fn dispatch_error_to_py(err: DispatchError) -> PyErr {
    match err {
        DispatchError::Shutdown => DispatchErrorPy::new_err("shutdown: dispatcher is shut down"),
        DispatchError::ResultDropped => {
            DispatchErrorPy::new_err("dropped: job result channel was dropped before delivery")
        }
        DispatchError::Panic(msg) => DispatchErrorPy::new_err(format!("panic: {msg}")),
        DispatchError::QueueOverloaded {
            depth,
            capacity,
            retry_after_secs,
        } => DispatchErrorPy::new_err(format!(
            "queue-overloaded: depth={depth}/{capacity}; retry in {retry_after_secs}s"
        )),
    }
}

// ── TickOutcome ─────────────────────────────────────────────────────

/// Python-visible equivalent of [`crate::TickOutcome`].
///
/// Frozen-field struct: callers only read these numbers to decide
/// polling cadence.
#[pyclass(
    name = "TickOutcome",
    frozen,
    skip_from_py_object,
    module = "dcc_mcp_core._core"
)]
#[derive(Debug, Clone, Copy)]
pub struct PyTickOutcome {
    /// Number of jobs that were drained and executed in this tick.
    #[pyo3(get)]
    pub jobs_executed: usize,
    /// Number of jobs that panicked (already surfaced as
    /// `DispatchError` to their callers).
    #[pyo3(get)]
    pub jobs_panicked: usize,
    /// `True` when the queue still had more jobs than `max_jobs`
    /// allowed. The caller should tick again soon.
    #[pyo3(get)]
    pub more_pending: bool,
}

#[pymethods]
impl PyTickOutcome {
    fn __repr__(&self) -> String {
        format!(
            "TickOutcome(jobs_executed={}, jobs_panicked={}, more_pending={})",
            self.jobs_executed, self.jobs_panicked, self.more_pending,
        )
    }
}

impl From<TickOutcome> for PyTickOutcome {
    fn from(t: TickOutcome) -> Self {
        Self {
            jobs_executed: t.jobs_executed,
            jobs_panicked: t.jobs_panicked,
            more_pending: t.more_pending,
        }
    }
}

// ── PostHandle → Python future-ish ──────────────────────────────────

/// Handle to a posted Python callable.
///
/// Returned by `post()`. Block on the result via [`PyPostHandle::wait`].
#[pyclass(name = "PostHandle", module = "dcc_mcp_core._core")]
pub struct PyPostHandle {
    // `Option` so `wait` can take ownership of the inner future on first
    // call and subsequent calls surface a clear error.
    inner: parking_lot::Mutex<Option<PostHandle<PyResult<PyObj>>>>,
}

#[pymethods]
impl PyPostHandle {
    /// Block the calling thread until the job completes.
    ///
    /// * `timeout` — seconds, or `None` to wait forever.
    ///
    /// Returns the Python object that the posted callable returned.
    /// Raises `DispatchError` on shutdown, panic, timeout, or
    /// propagates the original Python exception when the callable
    /// raised.
    #[pyo3(signature = (timeout=None))]
    fn wait<'py>(&self, py: Python<'py>, timeout: Option<f64>) -> PyResult<PyObj> {
        let handle = self.inner.lock().take().ok_or_else(|| {
            PyRuntimeError::new_err("PostHandle.wait() called twice: result already consumed")
        })?;

        let rt = shared_runtime();
        // Release the GIL while waiting on the oneshot — the tick may
        // be running on a Python thread that needs the GIL to execute
        // the callable. pyo3 0.28 renamed `allow_threads` to `detach`.
        let outcome: Result<Result<PyResult<PyObj>, DispatchError>, ()> =
            py.detach(|| match timeout {
                None => Ok(rt.block_on(handle)),
                Some(secs) if secs <= 0.0 => rt.block_on(async {
                    match tokio::time::timeout(Duration::from_millis(0), handle).await {
                        Ok(v) => Ok(v),
                        Err(_) => Err(()),
                    }
                }),
                Some(secs) => rt.block_on(async {
                    match tokio::time::timeout(Duration::from_secs_f64(secs), handle).await {
                        Ok(v) => Ok(v),
                        Err(_) => Err(()),
                    }
                }),
            });

        match outcome {
            Ok(Ok(Ok(obj))) => Ok(obj),
            Ok(Ok(Err(py_err))) => Err(py_err),
            Ok(Err(dispatch_err)) => Err(dispatch_error_to_py(dispatch_err)),
            Err(()) => Err(DispatchErrorPy::new_err(
                "timeout: wait() exceeded the requested duration",
            )),
        }
    }

    fn __repr__(&self) -> &'static str {
        "PostHandle(pending)"
    }
}

// ── Helper: build a Runnable from a Python callable ─────────────────
//
// Each `post()` call ships a boxed closure that calls the Python
// callable on the tick thread. The closure runs inside `Python::attach`
// because bpy / maya.cmds / hou need the GIL. Exceptions become
// `PyResult::Err`; successful returns become `PyResult::Ok(PyObj)`.

fn make_py_job(callable: PyObj) -> impl FnOnce() -> PyResult<PyObj> + Send + 'static {
    move || {
        Python::attach(|py| {
            let bound = callable.bind(py);
            let args = PyTuple::empty(py);
            bound.call1(args).map(|r| r.unbind())
        })
    }
}

// ── PyQueueDispatcher ───────────────────────────────────────────────

/// Python wrapper over [`crate::QueueDispatcher`].
///
/// Default dispatcher for interactive DCC modes. `tick()` is
/// non-blocking and must be called from the DCC's main thread.
#[pyclass(name = "QueueDispatcher", module = "dcc_mcp_core._core")]
pub struct PyQueueDispatcher {
    inner: Arc<QueueDispatcher>,
}

impl PyQueueDispatcher {
    /// Hand out the shared Rust dispatcher so another crate (e.g.
    /// `dcc-mcp-http::host_bridge`) can route tasks into it without
    /// going through Python.
    ///
    /// Exposed at crate level only — this is an internal seam, not a
    /// Python-visible method. SRP: the class stays focused on the
    /// Python surface; integration crates reach in by name.
    pub fn arc_inner(&self) -> Arc<dyn DccDispatcher> {
        self.inner.clone() as Arc<dyn DccDispatcher>
    }
}

#[pymethods]
impl PyQueueDispatcher {
    /// Construct a fresh dispatcher with an empty queue.
    #[new]
    fn new() -> Self {
        Self {
            inner: Arc::new(QueueDispatcher::new()),
        }
    }

    /// Enqueue a zero-arg Python callable for main-thread execution.
    ///
    /// Returns a [`PostHandle`] whose `wait()` yields the callable's
    /// return value — or raises `DispatchError` on failure.
    fn post(&self, callable: PyObj) -> PyPostHandle {
        let handle = self.inner.post(make_py_job(callable));
        PyPostHandle {
            inner: parking_lot::Mutex::new(Some(handle)),
        }
    }

    /// Drain at most `max_jobs` entries on the calling thread.
    #[pyo3(signature = (max_jobs=16))]
    fn tick(&self, max_jobs: usize) -> PyTickOutcome {
        self.inner.tick(max_jobs).into()
    }

    /// `True` when at least one job is waiting.
    fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }

    /// Approximate queue depth.
    fn pending(&self) -> usize {
        self.inner.pending()
    }

    /// Reject new posts and cancel anything still queued.
    fn shutdown(&self) {
        self.inner.shutdown();
    }

    /// `True` once `shutdown()` has been called.
    fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }

    fn __repr__(&self) -> String {
        format!(
            "QueueDispatcher(pending={}, shutdown={})",
            self.inner.pending(),
            self.inner.is_shutdown(),
        )
    }
}

// ── PyBlockingDispatcher ────────────────────────────────────────────

/// Python wrapper over [`crate::BlockingDispatcher`].
///
/// Dispatcher for headless DCC modes (`blender --background`,
/// `mayapy`, `hython`) where the host does not run an idle callback.
/// Supports `tick_blocking(max_jobs, timeout_ms)` in addition to the
/// non-blocking `tick`.
#[pyclass(name = "BlockingDispatcher", module = "dcc_mcp_core._core")]
pub struct PyBlockingDispatcher {
    inner: Arc<BlockingDispatcher>,
}

impl PyBlockingDispatcher {
    /// Hand out the shared Rust dispatcher. See [`PyQueueDispatcher::arc_inner`].
    pub fn arc_inner(&self) -> Arc<dyn DccDispatcher> {
        self.inner.clone() as Arc<dyn DccDispatcher>
    }
}

#[pymethods]
impl PyBlockingDispatcher {
    /// Construct a fresh headless dispatcher.
    #[new]
    fn new() -> Self {
        Self {
            inner: Arc::new(BlockingDispatcher::new()),
        }
    }

    /// Enqueue a zero-arg Python callable for main-thread execution.
    fn post(&self, callable: PyObj) -> PyPostHandle {
        let handle = self.inner.post(make_py_job(callable));
        PyPostHandle {
            inner: parking_lot::Mutex::new(Some(handle)),
        }
    }

    /// Non-blocking drain (same semantics as `QueueDispatcher.tick`).
    #[pyo3(signature = (max_jobs=16))]
    fn tick(&self, max_jobs: usize) -> PyTickOutcome {
        self.inner.tick(max_jobs).into()
    }

    /// Block up to `timeout_ms` waiting for the first job, then drain
    /// up to `max_jobs` on the calling thread.
    ///
    /// Returns an empty [`TickOutcome`] when the timeout elapsed
    /// without any job arriving.
    #[pyo3(signature = (max_jobs=16, timeout_ms=100))]
    fn tick_blocking(&self, py: Python<'_>, max_jobs: usize, timeout_ms: u64) -> PyTickOutcome {
        let inner = self.inner.clone();
        let rt = shared_runtime();
        // Release the GIL while blocking on the mpsc receive.
        py.detach(|| {
            rt.block_on(async move {
                inner
                    .tick_blocking(max_jobs, Duration::from_millis(timeout_ms))
                    .await
            })
        })
        .into()
    }

    /// `True` when at least one job is waiting.
    fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }

    /// Approximate queue depth.
    fn pending(&self) -> usize {
        self.inner.pending()
    }

    /// Reject new posts and cancel anything still queued.
    fn shutdown(&self) {
        self.inner.shutdown();
    }

    /// `True` once `shutdown()` has been called.
    fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }

    fn __repr__(&self) -> String {
        format!(
            "BlockingDispatcher(pending={}, shutdown={})",
            self.inner.pending(),
            self.inner.is_shutdown(),
        )
    }
}

// ── Module registration helper ──────────────────────────────────────

/// Register every Python-facing name from this module onto *m*.
///
/// Called once from the top-level `_core` PyO3 module initialiser.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("DispatchError", m.py().get_type::<DispatchErrorPy>())?;
    m.add_class::<PyTickOutcome>()?;
    m.add_class::<PyPostHandle>()?;
    m.add_class::<PyQueueDispatcher>()?;
    m.add_class::<PyBlockingDispatcher>()?;
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn with_py<F, R>(f: F) -> R
    where
        F: for<'py> FnOnce(Python<'py>) -> R,
    {
        // The `auto-initialize` pyo3 feature is enabled in dev-dependencies
        // so `Python::attach` boots an embedded interpreter on first use.
        Python::attach(f)
    }

    /// Round-trip: post a Python lambda that returns 42, tick from a
    /// worker thread, assert wait() returns 42.
    #[test]
    fn py_round_trip_returns_int() {
        with_py(|py| {
            let d = PyQueueDispatcher::new();
            let lambda = py.eval(c"lambda: 42", None, None).unwrap().unbind();
            let handle = d.post(lambda);
            // Drive the tick from another thread so `wait` actually
            // has to block on the oneshot. (Calling `tick` on the
            // same thread before `wait` would work too but tests the
            // fast path — we want the realistic path.)
            let inner = d.inner.clone();
            let ticker = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(10));
                Python::attach(|_| inner.tick(16));
            });
            let got = d.wait_via(&handle, py, None).unwrap();
            ticker.join().unwrap();
            let as_int: i64 = got.extract(py).unwrap();
            assert_eq!(as_int, 42);
        });
    }

    /// Python exception inside the callable surfaces as
    /// `DispatchError` on `wait`.
    #[test]
    fn py_exception_surfaces_as_dispatch_error() {
        with_py(|py| {
            let d = PyQueueDispatcher::new();
            let lambda = py
                .eval(
                    c"lambda: (_ for _ in ()).throw(ValueError('boom'))",
                    None,
                    None,
                )
                .unwrap()
                .unbind();
            let handle = d.post(lambda);
            let inner = d.inner.clone();
            let ticker = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(10));
                Python::attach(|_| inner.tick(16));
            });
            let err = d.wait_via(&handle, py, None).unwrap_err();
            ticker.join().unwrap();
            // Python exceptions from the callable do NOT become
            // DispatchError — they flow through as the real exception
            // so callers can `except ValueError`. We preserve that.
            assert!(err.is_instance_of::<pyo3::exceptions::PyValueError>(py));
        });
    }

    /// `wait(timeout=<small>)` raises `DispatchError("timeout: …")`
    /// when the job has not been drained yet.
    #[test]
    fn py_wait_timeout_raises_dispatch_error() {
        with_py(|py| {
            let d = PyQueueDispatcher::new();
            let lambda = py.eval(c"lambda: 0", None, None).unwrap().unbind();
            let handle = d.post(lambda);
            // Never tick. Wait with a tiny timeout.
            let err = d.wait_via(&handle, py, Some(0.02)).unwrap_err();
            assert!(err.is_instance_of::<DispatchErrorPy>(py));
            let msg = err.value(py).to_string();
            assert!(msg.contains("timeout"), "unexpected: {msg}");
        });
    }

    /// `shutdown` turns a pending post into a `DispatchError("shutdown")`.
    #[test]
    fn py_shutdown_cancels_pending() {
        with_py(|py| {
            let d = PyQueueDispatcher::new();
            let lambda = py.eval(c"lambda: 1", None, None).unwrap().unbind();
            let handle = d.post(lambda);
            d.shutdown();
            assert!(d.is_shutdown());
            let err = d.wait_via(&handle, py, None).unwrap_err();
            assert!(err.is_instance_of::<DispatchErrorPy>(py));
            let msg = err.value(py).to_string();
            assert!(msg.contains("shutdown"), "unexpected: {msg}");
        });
    }

    /// Helper for the tests above so they don't reimplement the
    /// wait/extract dance.
    impl PyQueueDispatcher {
        fn wait_via(
            &self,
            handle: &PyPostHandle,
            py: Python<'_>,
            timeout: Option<f64>,
        ) -> PyResult<PyObj> {
            handle.wait(py, timeout)
        }
    }
}
