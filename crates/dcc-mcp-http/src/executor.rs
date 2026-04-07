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
