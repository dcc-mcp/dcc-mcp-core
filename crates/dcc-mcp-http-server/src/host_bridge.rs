//! Bridge from [`dcc_mcp_host::DccDispatcher`] to
//! [`crate::executor::DccExecutorHandle`].
//!
//! # Why this exists
//!
//! The HTTP server already has a main-thread executor
//! ([`DccExecutorHandle`], driven by
//! [`crate::executor::DeferredExecutor::poll_pending`]). The portable,
//! cross-DCC dispatcher trait lives in `dcc-mcp-host`. Both solve the
//! same "tokio worker → DCC main thread" problem, with different
//! contracts and ownership stories. Rather than unify them into a
//! single trait — which would drag the server queue's `DccTaskFn`
//! string-return convention into the host-runtime abstraction — we
//! keep both and provide a one-directional adapter here.
//!
//! # Responsibility (SRP)
//!
//! This module is pure glue. It takes an [`Arc<dyn DccDispatcher>`]
//! and returns a [`DccExecutorHandle`] that forwards each type-erased
//! runnable through `dispatcher.post(...)`. Typed result channels are
//! captured by the runnable itself, so the bridge never serializes them.
//! It does not own the dispatcher, does not
//! own the executor, and does not touch `AppState` or the request
//! hot path.
//!
//! # SOLID
//!
//! - **SRP**: one tiny module, one job — forwarding.
//! - **OCP**: `sync_impl.rs::run_on_main_thread` consumes any
//!   `DccExecutorHandle` already; this adapter adds zero branches in
//!   the hot path.
//! - **LSP**: `QueueDispatcher`, `BlockingDispatcher`, and any future
//!   `BlenderHost` / `MayaHost` implementing `DccDispatcher` are all
//!   valid inputs.
//! - **DIP**: depends on the [`DccDispatcher`] trait, not on a
//!   concrete type.

use std::sync::Arc;

use dcc_mcp_host::{DccDispatcher, DccDispatcherExt};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::executor::{DccExecutorHandle, DccTask};

/// Historical default bridge queue depth. Kept only as the fallback
/// used by [`dispatcher_to_executor_handle`] when the caller has no
/// `McpHttpConfig` in hand. Prefer
/// [`dispatcher_to_executor_handle_with_capacity`] so operators can
/// tune the depth via the HTTP config `bridge_queue_depth` /
/// `MCP_QUEUE_BRIDGE_CAP` (issue #715).
pub const DEFAULT_BRIDGE_QUEUE_DEPTH: usize = 16;

/// Convert any [`Arc<dyn DccDispatcher>`] into a [`DccExecutorHandle`]
/// the HTTP server can plug straight into its `with_executor` hook.
///
/// Uses [`DEFAULT_BRIDGE_QUEUE_DEPTH`] so existing call-sites see
/// identical behaviour to pre-#715. New code paths should prefer
/// [`dispatcher_to_executor_handle_with_capacity`] so operators can
/// tune the depth via the HTTP config `bridge_queue_depth`.
///
/// A single background tokio task (spawned on `runtime`) drains the
/// synthesized handle's mpsc and forwards each runnable into
/// `dispatcher.post(...)`.
#[must_use]
pub fn dispatcher_to_executor_handle(
    dispatcher: Arc<dyn DccDispatcher>,
    runtime: &Handle,
) -> DccExecutorHandle {
    dispatcher_to_executor_handle_with_capacity(dispatcher, runtime, DEFAULT_BRIDGE_QUEUE_DEPTH)
}

/// Like [`dispatcher_to_executor_handle`] but accepts an explicit
/// queue capacity (issue #715).
///
/// `capacity == 0` degrades gracefully to [`DEFAULT_BRIDGE_QUEUE_DEPTH`]
/// so misconfigured env-vars cannot silently disable backpressure.
#[must_use]
pub fn dispatcher_to_executor_handle_with_capacity(
    dispatcher: Arc<dyn DccDispatcher>,
    runtime: &Handle,
    capacity: usize,
) -> DccExecutorHandle {
    let depth = if capacity == 0 {
        DEFAULT_BRIDGE_QUEUE_DEPTH
    } else {
        capacity
    };
    let (tx, mut rx) = mpsc::channel::<DccTask>(depth);

    runtime.spawn(async move {
        while let Some(DccTask { func }) = rx.recv().await {
            if let Err(error) = dispatcher.post(func).await {
                tracing::warn!(%error, "host dispatcher rejected HTTP executor task");
            }
        }
    });

    DccExecutorHandle::from_sender(tx, depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    use dcc_mcp_host::QueueDispatcher;
    use std::time::Duration;

    /// Round-trip: submit a task through the synthesized handle, tick
    /// the dispatcher from a "main thread" helper, assert the oneshot
    /// returns the string the closure produced.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn round_trip_forwards_string_result() {
        let dispatcher: Arc<dyn DccDispatcher> = Arc::new(QueueDispatcher::new());
        let handle = dispatcher_to_executor_handle(dispatcher.clone(), &Handle::current());

        let dispatcher_tick = dispatcher.clone();
        let ticker = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                let outcome = dispatcher_tick.tick(16);
                if outcome.jobs_executed > 0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("ticker saw no jobs within deadline");
        });

        let got = handle
            .execute(Box::new(|| "hello".to_string()))
            .await
            .unwrap();
        assert_eq!(got, "hello");
        ticker.join().unwrap();
    }

    /// Panics close the typed task result channel.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panic_closes_typed_result_channel() {
        let dispatcher: Arc<dyn DccDispatcher> = Arc::new(QueueDispatcher::new());
        let handle = dispatcher_to_executor_handle(dispatcher.clone(), &Handle::current());

        let dispatcher_tick = dispatcher.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            dispatcher_tick.tick(16);
        });

        let error = handle
            .execute(Box::new(|| panic!("boom in closure")))
            .await
            .unwrap_err();
        assert!(matches!(error, crate::executor::ExecutorError::Closed));
    }

    /// After dispatcher shutdown, typed task result channels close.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_closes_typed_result_channel() {
        let dispatcher: Arc<dyn DccDispatcher> = Arc::new(QueueDispatcher::new());
        let handle = dispatcher_to_executor_handle(dispatcher.clone(), &Handle::current());

        dispatcher.shutdown();

        let error = handle
            .execute(Box::new(|| "never runs".to_string()))
            .await
            .unwrap_err();
        assert!(matches!(error, crate::executor::ExecutorError::Closed));
    }
}
