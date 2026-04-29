use super::*;
use crate::idempotency::IdempotencyStore;

/// Builder for [`WorkflowExecutor`].
#[derive(Default)]
pub struct WorkflowExecutorBuilder {
    tool_caller: Option<SharedToolCaller>,
    remote_caller: Option<SharedRemoteCaller>,
    notifier: Option<SharedNotifier>,
    artefacts: Option<SharedArtefactStore>,
    idempotency: Option<SharedIdempotencyStore>,
    approval_gate: Option<ApprovalGate>,
    #[cfg(feature = "job-persist-sqlite")]
    storage: Option<Arc<crate::sqlite::WorkflowStorage>>,
    default_approve_timeout: Option<Duration>,
}

impl WorkflowExecutorBuilder {
    /// Set the local tool caller.
    pub fn tool_caller(mut self, caller: SharedToolCaller) -> Self {
        self.tool_caller = Some(caller);
        self
    }

    /// Convenience: wrap an [`dcc_mcp_actions::dispatcher::ActionDispatcher`]
    /// as the local tool caller.
    pub fn dispatcher(mut self, dispatcher: dcc_mcp_actions::dispatcher::ActionDispatcher) -> Self {
        self.tool_caller = Some(Arc::new(ActionDispatcherCaller::new(dispatcher)));
        self
    }

    /// Set the remote / gateway caller (defaults to [`NullRemoteCaller`]).
    pub fn remote_caller(mut self, caller: SharedRemoteCaller) -> Self {
        self.remote_caller = Some(caller);
        self
    }

    /// Set the SSE notifier (defaults to [`NullNotifier`]).
    pub fn notifier(mut self, notifier: SharedNotifier) -> Self {
        self.notifier = Some(notifier);
        self
    }

    /// Set the artefact store.
    pub fn artefacts(mut self, store: SharedArtefactStore) -> Self {
        self.artefacts = Some(store);
        self
    }

    /// Override the default approval timeout (applies when a step omits
    /// `timeout_secs`).
    pub fn default_approve_timeout(mut self, d: Duration) -> Self {
        self.default_approve_timeout = Some(d);
        self
    }

    /// Attach a shared in-memory idempotency cache (defaults to a fresh
    /// one). Convenience for the historical concrete-type API.
    pub fn idempotency(mut self, cache: IdempotencyCache) -> Self {
        self.idempotency = Some(Arc::new(cache));
        self
    }

    /// Attach an arbitrary [`IdempotencyStore`] implementation. Use this
    /// to plug in `crate::sqlite::SqliteIdempotencyStore` for persistent
    /// idempotency that survives server restarts.
    pub fn idempotency_store<S: IdempotencyStore + 'static>(mut self, store: S) -> Self {
        self.idempotency = Some(Arc::new(store));
        self
    }

    /// Attach a pre-wrapped [`SharedIdempotencyStore`].
    pub fn shared_idempotency(mut self, store: SharedIdempotencyStore) -> Self {
        self.idempotency = Some(store);
        self
    }

    /// Attach a shared approval gate registry (defaults to a fresh one).
    pub fn approval_gate(mut self, gate: ApprovalGate) -> Self {
        self.approval_gate = Some(gate);
        self
    }

    /// Attach a SQLite storage backend. When present, every workflow/step
    /// transition is persisted and `recover()` flips non-terminal rows to
    /// `interrupted` on restart.
    #[cfg(feature = "job-persist-sqlite")]
    pub fn storage(mut self, storage: Arc<crate::sqlite::WorkflowStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Finalise. Panics if no tool caller is configured — there's no
    /// sensible default and every workflow has at least one `tool` step.
    pub fn build(self) -> WorkflowExecutor {
        WorkflowExecutor {
            tool_caller: self
                .tool_caller
                .expect("WorkflowExecutor requires a tool_caller"),
            remote_caller: self
                .remote_caller
                .unwrap_or_else(|| Arc::new(NullRemoteCaller)),
            notifier: self.notifier.unwrap_or_else(|| Arc::new(NullNotifier)),
            artefacts: self.artefacts,
            idempotency: self
                .idempotency
                .unwrap_or_else(|| Arc::new(IdempotencyCache::new())),
            approval_gate: self.approval_gate.unwrap_or_default(),
            #[cfg(feature = "job-persist-sqlite")]
            storage: self.storage,
            default_approve_timeout: self.default_approve_timeout,
        }
    }
}
