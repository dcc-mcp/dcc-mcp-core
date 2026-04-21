//! Executor tool-call abstractions.
//!
//! The workflow crate is transport-agnostic: it delegates actual tool
//! invocation to the [`ToolCaller`] (for local `tool` steps) and
//! [`RemoteCaller`] (for `tool_remote` steps) traits. Production wiring is
//! provided by the server:
//!
//! - `dcc-mcp-actions` adapter → [`ActionDispatcherCaller`]
//! - `dcc-mcp-http` gateway passthrough → an http-crate-side impl
//!
//! Each caller returns the raw tool output as JSON. The executor extracts
//! `file_refs` via [`crate::context::StepOutput::from_value`].

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use dcc_mcp_actions::dispatcher::ActionDispatcher;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// Return type of [`ToolCaller::call`] — a boxed future yielding
/// `Result<Value, String>`.
pub type CallFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>;

/// Adapter for calling local MCP tools by name.
///
/// The workflow executor calls this for every `StepKind::Tool`. `cancel`
/// is the per-step cancellation token; implementations that run blocking
/// work should honour it cooperatively.
pub trait ToolCaller: Send + Sync {
    /// Invoke `tool_name` with the given JSON `args`. Must be safe to call
    /// from any async worker.
    fn call<'a>(
        &'a self,
        tool_name: &'a str,
        args: Value,
        cancel: CancellationToken,
    ) -> CallFuture<'a>;
}

/// Adapter for calling remote MCP tools (`tool_remote` steps) on a specific
/// DCC target via the gateway.
pub trait RemoteCaller: Send + Sync {
    /// Invoke `tool_name` on the `dcc` target with `args`.
    fn call<'a>(
        &'a self,
        dcc: &'a str,
        tool_name: &'a str,
        args: Value,
        cancel: CancellationToken,
    ) -> CallFuture<'a>;
}

/// Shared, thread-safe tool caller alias.
pub type SharedToolCaller = Arc<dyn ToolCaller>;
/// Shared, thread-safe remote caller alias.
pub type SharedRemoteCaller = Arc<dyn RemoteCaller>;

// ── Dispatcher adapter ───────────────────────────────────────────────────

/// Bridge a synchronous [`ActionDispatcher`] into the async [`ToolCaller`]
/// trait.
///
/// Dispatches are offloaded via [`tokio::task::spawn_blocking`] so they do
/// not stall the async worker. The cancellation token aborts the *await*
/// before the handler sees it, but cannot interrupt synchronous Rust code
/// mid-call — cooperative checkpoints inside the handler (issue #329) are
/// the right mechanism for fine-grained cancel inside long tools.
#[derive(Clone)]
pub struct ActionDispatcherCaller {
    dispatcher: ActionDispatcher,
}

impl ActionDispatcherCaller {
    /// Wrap a dispatcher.
    pub fn new(dispatcher: ActionDispatcher) -> Self {
        Self { dispatcher }
    }
}

impl ToolCaller for ActionDispatcherCaller {
    fn call<'a>(
        &'a self,
        tool_name: &'a str,
        args: Value,
        cancel: CancellationToken,
    ) -> CallFuture<'a> {
        let dispatcher = self.dispatcher.clone();
        let name = tool_name.to_string();
        Box::pin(async move {
            let dispatch = tokio::task::spawn_blocking(move || dispatcher.dispatch(&name, args));
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err("cancelled".to_string()),
                res = dispatch => match res {
                    Err(e) => Err(format!("dispatch join error: {e}")),
                    Ok(Err(e)) => Err(e.to_string()),
                    Ok(Ok(d)) => Ok(d.output),
                },
            }
        })
    }
}

/// No-op remote caller — returns an error for every call. Used when a
/// workflow has no `tool_remote` steps and the server does not wire in a
/// gateway client.
#[derive(Debug, Default, Clone)]
pub struct NullRemoteCaller;

impl RemoteCaller for NullRemoteCaller {
    fn call<'a>(
        &'a self,
        dcc: &'a str,
        tool_name: &'a str,
        _args: Value,
        _cancel: CancellationToken,
    ) -> CallFuture<'a> {
        let msg = format!(
            "tool_remote step cannot be executed: no RemoteCaller wired for dcc={dcc:?}, tool={tool_name:?}"
        );
        Box::pin(async move { Err(msg) })
    }
}

// ── Test helpers ─────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use parking_lot::Mutex;
    use std::collections::HashMap;

    /// In-memory [`ToolCaller`] backed by a map of `name → Fn(args) → Result<Value>`.
    type Handler = Arc<dyn Fn(Value) -> Result<Value, String> + Send + Sync>;

    #[derive(Default)]
    pub struct MockToolCaller {
        handlers: Mutex<HashMap<String, Handler>>,
        /// History of calls in arrival order.
        pub calls: Mutex<Vec<(String, Value)>>,
    }

    impl MockToolCaller {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn add<F>(&self, name: &str, f: F)
        where
            F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
        {
            self.handlers.lock().insert(name.to_string(), Arc::new(f));
        }
        pub fn call_count(&self, name: &str) -> usize {
            self.calls.lock().iter().filter(|(n, _)| n == name).count()
        }
    }

    impl ToolCaller for MockToolCaller {
        fn call<'a>(
            &'a self,
            tool_name: &'a str,
            args: Value,
            _cancel: CancellationToken,
        ) -> CallFuture<'a> {
            self.calls
                .lock()
                .push((tool_name.to_string(), args.clone()));
            let handler = self.handlers.lock().get(tool_name).cloned();
            Box::pin(async move {
                match handler {
                    None => Err(format!("no handler registered for {tool_name:?}")),
                    Some(f) => f(args),
                }
            })
        }
    }

    /// In-memory [`RemoteCaller`] backed by a map.
    #[derive(Default)]
    pub struct MockRemoteCaller {
        handlers: Mutex<HashMap<(String, String), Handler>>,
        pub calls: Mutex<Vec<(String, String, Value)>>,
    }

    impl MockRemoteCaller {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn add<F>(&self, dcc: &str, tool: &str, f: F)
        where
            F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
        {
            self.handlers
                .lock()
                .insert((dcc.to_string(), tool.to_string()), Arc::new(f));
        }
    }

    impl RemoteCaller for MockRemoteCaller {
        fn call<'a>(
            &'a self,
            dcc: &'a str,
            tool_name: &'a str,
            args: Value,
            _cancel: CancellationToken,
        ) -> CallFuture<'a> {
            self.calls
                .lock()
                .push((dcc.to_string(), tool_name.to_string(), args.clone()));
            let handler = self
                .handlers
                .lock()
                .get(&(dcc.to_string(), tool_name.to_string()))
                .cloned();
            Box::pin(async move {
                match handler {
                    None => Err(format!(
                        "no remote handler registered for dcc={dcc:?}, tool={tool_name:?}"
                    )),
                    Some(f) => f(args),
                }
            })
        }
    }
}
