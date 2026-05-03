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
//! single trait — which would drag HTTP-specific concerns
//! ([`crate::error::HttpError`], the `DccTaskFn` string-return
//! convention) into the host-runtime abstraction — we keep both and
//! provide a one-directional adapter here.
//!
//! # Responsibility (SRP)
//!
//! This module is pure glue. It takes an [`Arc<dyn DccDispatcher>`]
//! and returns a [`DccExecutorHandle`] that forwards every submitted
//! [`crate::executor::DccTaskFn`] through `dispatcher.post(...)` and
//! relays the resulting `String` back through the `oneshot` the HTTP
//! layer already expects. It does not own the dispatcher, does not
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

use dcc_mcp_host::{DccDispatcher, DccDispatcherExt, DispatchError};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::executor::{DccExecutorHandle, DccTask};

/// Historical default bridge queue depth. Kept only as the fallback
/// used by [`dispatcher_to_executor_handle`] when the caller has no
/// `McpHttpConfig` in hand. Prefer
/// [`dispatcher_to_executor_handle_with_capacity`] so operators can
/// tune the depth via [`crate::McpHttpConfig::bridge_queue_depth`]
/// / `MCP_QUEUE_BRIDGE_CAP` (issue #715).
pub const DEFAULT_BRIDGE_QUEUE_DEPTH: usize = 16;

/// Convert any [`Arc<dyn DccDispatcher>`] into a [`DccExecutorHandle`]
/// the HTTP server can plug straight into
/// [`crate::server::McpHttpServer::with_executor`].
///
/// Uses [`DEFAULT_BRIDGE_QUEUE_DEPTH`] so existing call-sites see
/// identical behaviour to pre-#715. New code paths should prefer
/// [`dispatcher_to_executor_handle_with_capacity`] so operators can
/// tune the depth via [`crate::McpHttpConfig::bridge_queue_depth`].
///
/// A single background tokio task (spawned on `runtime`) drains the
/// synthesized handle's mpsc, forwards each closure into
/// `dispatcher.post(...)`, awaits the post, and ships the resulting
/// `String` through the oneshot the HTTP hot path already awaits on.
///
/// # Error encoding
///
/// On [`DispatchError`] we encode the failure as a
/// `{"__dispatch_error": "..."}` JSON string, matching the convention
/// used by
/// [`crate::handlers::tools_call::sync_impl::run_on_main_thread`].
/// The HTTP layer's `decode_dispatch_output` unwraps this into a
/// user-facing error without introducing a new error type.
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
        while let Some(DccTask { func, result_tx }) = rx.recv().await {
            let post = dispatcher.post(func);
            let payload = match post.await {
                Ok(json_string) => json_string,
                Err(err) => encode_dispatch_error(&err),
            };
            // Ignore send failures: the HTTP caller may have dropped
            // its receiver after a timeout. That's their contract,
            // not ours.
            let _ = result_tx.send(payload);
        }
    });

    DccExecutorHandle::from_sender(tx, depth)
}

/// Encode a [`DispatchError`] as the `{"__dispatch_error": "..."}`
/// JSON string the HTTP hot path understands.
///
/// The tag prefix (`shutdown:` / `dropped:` / `panic:`) mirrors the
/// convention in [`dcc_mcp_host::python::PyPostHandle::wait`] so
/// Python and Rust surfaces report the same failure taxonomy.
fn encode_dispatch_error(err: &DispatchError) -> String {
    let tag = match err {
        DispatchError::Shutdown => "shutdown",
        DispatchError::ResultDropped => "dropped",
        DispatchError::Panic(_) => "panic",
        DispatchError::QueueOverloaded { .. } => "queue-overloaded",
    };
    serde_json::to_string(&serde_json::json!({
        "__dispatch_error": format!("{tag}: {err}"),
    }))
    .unwrap_or_else(|_| r#"{"__dispatch_error":"dispatch failure"}"#.to_string())
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

    /// Panics inside the closure surface as `__dispatch_error` JSON.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panic_surfaces_as_dispatch_error_json() {
        let dispatcher: Arc<dyn DccDispatcher> = Arc::new(QueueDispatcher::new());
        let handle = dispatcher_to_executor_handle(dispatcher.clone(), &Handle::current());

        let dispatcher_tick = dispatcher.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            dispatcher_tick.tick(16);
        });

        let got = handle
            .execute(Box::new(|| panic!("boom in closure")))
            .await
            .unwrap();
        assert!(
            got.contains("__dispatch_error") && got.contains("panic"),
            "expected __dispatch_error json for panic, got: {got}"
        );
    }

    /// After the dispatcher shuts down, submits surface as
    /// `__dispatch_error: shutdown` JSON.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_surfaces_as_dispatch_error_json() {
        let dispatcher: Arc<dyn DccDispatcher> = Arc::new(QueueDispatcher::new());
        let handle = dispatcher_to_executor_handle(dispatcher.clone(), &Handle::current());

        dispatcher.shutdown();

        let got = handle
            .execute(Box::new(|| "never runs".to_string()))
            .await
            .unwrap();
        assert!(
            got.contains("__dispatch_error") && got.contains("shutdown"),
            "expected __dispatch_error json for shutdown, got: {got}"
        );
    }
}
