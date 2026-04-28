//! Pluggable JSON-RPC method router (#492).
//!
//! Replaces the hand-coded `match` table in [`crate::handler::dispatch`]
//! with a [`MethodRouter`] keyed on `req.method`. Built-in methods are
//! registered at server startup via [`MethodRouter::with_builtins`].
//! Downstream crates and embedders can extend the router at runtime
//! through [`MethodRouter::register`].
//!
//! ## Why a trait?
//!
//! - **Open/closed**: adding a new MCP method no longer requires editing
//!   the dispatch.
//! - **Capability gating lives with the handler**, not the router. Each
//!   built-in wrapper checks `state.enable_resources` /
//!   `state.enable_prompts` itself and surfaces `method_not_found` when
//!   the capability is disabled — preserving the previous wire behaviour.
//! - **Object-safe + dyn-compatible**: the trait returns a boxed future
//!   so `Arc<dyn MethodHandler>` works on stable Rust without depending
//!   on `async-trait`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use dashmap::DashMap;
use serde_json::json;

use super::state::AppState;
use crate::error::HttpError;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Boxed future returned by [`MethodHandler::handle`].
pub type HandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<JsonRpcResponse, HttpError>> + Send + 'a>>;

/// Pluggable handler for a single JSON-RPC method.
///
/// Implementations live alongside the dispatch logic for built-in
/// methods; downstream crates can implement this trait for custom
/// methods and register them via [`MethodRouter::register`].
pub trait MethodHandler: Send + Sync {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a>;
}

/// Convenience: any `Fn`-shaped closure with the right signature is a
/// `MethodHandler` automatically. Useful for one-off methods that do not
/// warrant a dedicated type.
impl<F> MethodHandler for F
where
    F: for<'a> Fn(&'a AppState, &'a JsonRpcRequest, Option<&'a str>) -> HandlerFuture<'a>
        + Send
        + Sync,
{
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        (self)(state, req, session_id)
    }
}

/// Registry of [`MethodHandler`] instances keyed by JSON-RPC method
/// name. Cloning is cheap — the underlying map is `Arc<DashMap>`.
#[derive(Clone, Default)]
pub struct MethodRouter {
    handlers: Arc<DashMap<String, Arc<dyn MethodHandler>>>,
}

impl MethodRouter {
    /// Create an empty router. Most callers want
    /// [`MethodRouter::with_builtins`] instead.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a router pre-populated with every built-in MCP method.
    pub fn with_builtins() -> Self {
        let router = Self::new();
        super::builtins::register_builtins(&router);
        router
    }

    /// Register (or replace) a handler for `method`.
    pub fn register(&self, method: impl Into<String>, handler: Arc<dyn MethodHandler>) {
        self.handlers.insert(method.into(), handler);
    }

    /// Remove and return the handler for `method`, if any.
    pub fn unregister(&self, method: &str) -> Option<Arc<dyn MethodHandler>> {
        self.handlers.remove(method).map(|(_, v)| v)
    }

    /// Look up the handler for `method`.
    pub fn get(&self, method: &str) -> Option<Arc<dyn MethodHandler>> {
        self.handlers.get(method).map(|e| e.value().clone())
    }

    /// Snapshot of registered method names. Mostly for diagnostics.
    pub fn methods(&self) -> Vec<String> {
        self.handlers.iter().map(|e| e.key().clone()).collect()
    }

    /// Dispatch `req` to the registered handler, falling back to
    /// `method_not_found` when no handler exists.
    pub async fn dispatch(
        &self,
        state: &AppState,
        req: &JsonRpcRequest,
        session_id: Option<&str>,
    ) -> Result<JsonRpcResponse, HttpError> {
        if let Some(handler) = self.get(&req.method) {
            handler.handle(state, req, session_id).await
        } else {
            Ok(JsonRpcResponse::method_not_found(
                req.id.clone(),
                &req.method,
            ))
        }
    }
}

// ── Tiny no-op handlers used by built-ins ─────────────────────────────

/// Reply with `{}` — used for `notifications/initialized` and `ping`.
pub(crate) struct EmptyAckHandler;

impl MethodHandler for EmptyAckHandler {
    fn handle<'a>(
        &'a self,
        _state: &'a AppState,
        req: &'a JsonRpcRequest,
        _session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        let id = req.id.clone();
        Box::pin(async move { Ok(JsonRpcResponse::success(id, json!({}))) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Echo handler used by the extension-point tests.
    struct EchoHandler;
    impl MethodHandler for EchoHandler {
        fn handle<'a>(
            &'a self,
            _state: &'a AppState,
            req: &'a JsonRpcRequest,
            _session_id: Option<&'a str>,
        ) -> HandlerFuture<'a> {
            let id = req.id.clone();
            let params = req.params.clone().unwrap_or(json!(null));
            Box::pin(async move { Ok(JsonRpcResponse::success(id, json!({"echoed": params}))) })
        }
    }

    #[test]
    fn router_with_builtins_registers_known_methods() {
        let router = MethodRouter::with_builtins();
        let methods = router.methods();
        for m in [
            "initialize",
            "notifications/initialized",
            "ping",
            "tools/list",
            "tools/call",
            "resources/list",
            "resources/read",
            "resources/subscribe",
            "resources/unsubscribe",
            "prompts/list",
            "prompts/get",
            "elicitation/create",
        ] {
            assert!(methods.iter().any(|x| x == m), "missing builtin: {m}");
        }
    }

    #[test]
    fn register_and_unregister_round_trip() {
        let router = MethodRouter::new();
        assert!(router.get("custom/echo").is_none());
        router.register("custom/echo", Arc::new(EchoHandler));
        assert!(router.get("custom/echo").is_some());
        let removed = router.unregister("custom/echo");
        assert!(removed.is_some());
        assert!(router.get("custom/echo").is_none());
    }
}
