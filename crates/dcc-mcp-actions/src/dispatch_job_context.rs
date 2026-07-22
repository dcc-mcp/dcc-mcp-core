//! Server-owned job context threaded through synchronous action handlers.
//!
//! Transport layers install this context immediately around
//! [`crate::ToolDispatcher::dispatch`]. In-process DCC executors can then
//! expose the authoritative job id and a read-only cancellation probe without
//! trusting caller-provided metadata or depending on a particular async
//! runtime.

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

/// Read-only context for the asynchronous job that owns one dispatch.
#[derive(Clone)]
pub struct DispatchJobContext {
    job_id: Arc<str>,
    is_cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl DispatchJobContext {
    /// Build a context from a server-owned job id and cancellation probe.
    pub fn new(
        job_id: impl Into<String>,
        is_cancelled: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            job_id: Arc::from(job_id.into()),
            is_cancelled: Arc::new(is_cancelled),
        }
    }

    /// Return the authoritative server-owned job id.
    #[must_use]
    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    /// Return whether the owning job has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        (self.is_cancelled)()
    }
}

impl fmt::Debug for DispatchJobContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DispatchJobContext")
            .field("job_id", &self.job_id)
            .finish_non_exhaustive()
    }
}

thread_local! {
    static JOB_CONTEXT: RefCell<Option<DispatchJobContext>> = const { RefCell::new(None) };
}

/// Run `f` while publishing the owning asynchronous job context.
pub fn with_dispatch_job_context<R>(context: DispatchJobContext, f: impl FnOnce() -> R) -> R {
    JOB_CONTEXT.with(|slot| {
        struct Reset<'a> {
            slot: &'a RefCell<Option<DispatchJobContext>>,
            previous: Option<DispatchJobContext>,
        }

        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.slot.replace(self.previous.take());
            }
        }

        let previous = slot.replace(Some(context));
        let _reset = Reset { slot, previous };
        f()
    })
}

/// Return the current dispatch job context, if one is installed.
#[must_use]
pub fn current_dispatch_job_context() -> Option<DispatchJobContext> {
    JOB_CONTEXT.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn nested_context_restores_previous_job() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let outer = DispatchJobContext::new("outer", {
            let cancelled = Arc::clone(&cancelled);
            move || cancelled.load(Ordering::Acquire)
        });
        let inner = DispatchJobContext::new("inner", || true);

        with_dispatch_job_context(outer, || {
            assert_eq!(
                current_dispatch_job_context()
                    .as_ref()
                    .map(|ctx| ctx.job_id()),
                Some("outer")
            );
            with_dispatch_job_context(inner, || {
                let current = current_dispatch_job_context().expect("inner context");
                assert_eq!(current.job_id(), "inner");
                assert!(current.is_cancelled());
            });
            assert_eq!(
                current_dispatch_job_context()
                    .as_ref()
                    .map(|ctx| ctx.job_id()),
                Some("outer")
            );
            cancelled.store(true, Ordering::Release);
            assert!(
                current_dispatch_job_context()
                    .expect("outer context")
                    .is_cancelled()
            );
        });

        assert!(current_dispatch_job_context().is_none());
    }
}
