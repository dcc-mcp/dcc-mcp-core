//! Approval gate for `StepKind::Approve`.
//!
//! When an `Approve` step runs, the executor pauses the workflow until one
//! of:
//! - an MCP client sends `notifications/$/dcc.approveResponse { workflow_id,
//!   step_id, approved, reason }`, or
//! - the step's `timeout_secs` elapses (default: indefinite; when set,
//!   timeout is treated as `approved=false, reason="timeout"`).
//!
//! The [`ApprovalGate`] is a process-local registry keyed by
//! `(workflow_id, step_id)` so multiple concurrent workflows do not trip
//! over each other. The HTTP crate bridges inbound `approveResponse` JSON-RPC
//! notifications into [`ApprovalGate::resolve`].

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;

/// Outcome of an approval gate.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApprovalResponse {
    /// Whether the gate was approved. On timeout this is `false`.
    pub approved: bool,
    /// Optional human-readable reason. Populated with `"timeout"` when the
    /// approval deadline elapsed.
    pub reason: Option<String>,
}

impl ApprovalResponse {
    /// Convenience: timeout outcome.
    pub fn timeout() -> Self {
        Self {
            approved: false,
            reason: Some("timeout".to_string()),
        }
    }

    /// Convenience: cancelled outcome.
    pub fn cancelled() -> Self {
        Self {
            approved: false,
            reason: Some("cancelled".to_string()),
        }
    }
}

type ApprovalSenderMap =
    std::collections::HashMap<(Uuid, String), oneshot::Sender<ApprovalResponse>>;

/// Process-local registry of outstanding approval gates.
#[derive(Debug, Default, Clone)]
pub struct ApprovalGate {
    inner: Arc<Mutex<ApprovalSenderMap>>,
}

impl ApprovalGate {
    /// Construct an empty gate registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a pending approval and return the receiver side. Drops the
    /// previous waiter for the same key, if any.
    pub fn wait_handle(
        &self,
        workflow_id: Uuid,
        step_id: &str,
    ) -> oneshot::Receiver<ApprovalResponse> {
        let (tx, rx) = oneshot::channel();
        self.inner
            .lock()
            .insert((workflow_id, step_id.to_string()), tx);
        rx
    }

    /// Resolve the approval for `(workflow_id, step_id)`. Returns `true` if
    /// a waiter was listening.
    pub fn resolve(&self, workflow_id: Uuid, step_id: &str, resp: ApprovalResponse) -> bool {
        let sender = self
            .inner
            .lock()
            .remove(&(workflow_id, step_id.to_string()));
        match sender {
            Some(tx) => tx.send(resp).is_ok(),
            None => false,
        }
    }

    /// Drop any pending waiter for `(workflow_id, step_id)` without
    /// resolving it. Used when a step is cancelled. The receiver will
    /// observe a channel close.
    pub fn discard(&self, workflow_id: Uuid, step_id: &str) {
        self.inner
            .lock()
            .remove(&(workflow_id, step_id.to_string()));
    }

    /// Number of outstanding pending approvals. Used in tests.
    pub fn pending_count(&self) -> usize {
        self.inner.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_delivers_response() {
        let gate = ApprovalGate::new();
        let wid = Uuid::new_v4();
        let mut rx = gate.wait_handle(wid, "gate");
        assert_eq!(gate.pending_count(), 1);

        let r = gate.resolve(
            wid,
            "gate",
            ApprovalResponse {
                approved: true,
                reason: None,
            },
        );
        assert!(r);
        let resp = (&mut rx).await.unwrap();
        assert!(resp.approved);
        assert_eq!(gate.pending_count(), 0);
    }

    #[tokio::test]
    async fn resolve_without_waiter_returns_false() {
        let gate = ApprovalGate::new();
        assert!(!gate.resolve(Uuid::new_v4(), "nope", ApprovalResponse::timeout()));
    }

    #[tokio::test]
    async fn discard_closes_channel() {
        let gate = ApprovalGate::new();
        let wid = Uuid::new_v4();
        let rx = gate.wait_handle(wid, "gate");
        gate.discard(wid, "gate");
        let err = rx.await.unwrap_err();
        // oneshot::error::RecvError is the only variant.
        assert_eq!(err.to_string(), "channel closed");
    }
}
