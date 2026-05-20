//! Optional execution context threaded through synchronous dispatches.
//!
//! HTTP/MCP layers set [`DispatchExecutionContext`] before calling
//! [`crate::ToolDispatcher::dispatch`] so affinity violations can cite
//! whether a host dispatcher was wired (issue #1075).

use std::cell::Cell;

/// Snapshot of the runtime environment for the current dispatch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatchExecutionContext {
    /// `Some(true)` when a host `DccExecutorHandle` / queue dispatcher is
    /// attached; `Some(false)` when the HTTP server has no main-thread bridge;
    /// `None` when the caller did not set context (e.g. unit tests).
    pub host_dispatcher_attached: Option<bool>,
}

thread_local! {
    static EXECUTION_CONTEXT: Cell<Option<DispatchExecutionContext>> = const { Cell::new(None) };
}

/// Run `f` while publishing `ctx` to affinity diagnostics.
pub fn with_execution_context<R>(ctx: DispatchExecutionContext, f: impl FnOnce() -> R) -> R {
    EXECUTION_CONTEXT.with(|cell| {
        let previous = cell.replace(Some(ctx));
        let result = f();
        cell.set(previous);
        result
    })
}

/// Return the context for the current thread, if any.
#[must_use]
pub fn current_execution_context() -> Option<DispatchExecutionContext> {
    EXECUTION_CONTEXT.with(Cell::get)
}
