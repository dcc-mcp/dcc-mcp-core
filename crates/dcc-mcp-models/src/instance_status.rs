//! Service and dispatch status policy shared by gateway-facing applications.

use serde::{Deserialize, Serialize};

/// Status of a discovered DCC service instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    /// Service is available and accepting connections.
    #[default]
    Available,
    /// Service is busy processing a request.
    Busy,
    /// Service is unreachable after a health check failed.
    Unreachable,
    /// Service is shutting down.
    ShuttingDown,
    /// Service is alive while its embedded DCC host is still initialising.
    Booting,
    /// Service was explicitly marked stale and is no longer routable.
    Stale,
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Available => write!(f, "available"),
            Self::Busy => write!(f, "busy"),
            Self::Unreachable => write!(f, "unreachable"),
            Self::ShuttingDown => write!(f, "shutting_down"),
            Self::Booting => write!(f, "booting"),
            Self::Stale => write!(f, "stale"),
        }
    }
}

/// Application-level dispatch readiness for a DCC instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    /// Instance has reported dispatch-ready.
    Ready,
    /// Instance is alive but has not yet reported dispatch-ready.
    Pending,
    /// Instance reported a dispatch failure.
    Failed,
    /// Instance has not reported dispatch status.
    #[default]
    Unknown,
}

impl std::fmt::Display for DispatchStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => write!(f, "ready"),
            Self::Pending => write!(f, "pending"),
            Self::Failed => write!(f, "failed"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Application-facing interpretation of discovery and dispatch state.
///
/// Transport code reports [`ServiceStatus`] and [`DispatchStatus`] facts.
/// This model owns the retry policy and agent-facing recovery guidance derived
/// from those facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceStatus {
    /// Transport-level connection state.
    pub status: ServiceStatus,
    /// Application-level dispatch readiness.
    pub dispatch_status: DispatchStatus,
    /// Whether the current state is safe to retry.
    pub retryable: bool,
    /// Human-readable recommended next step.
    pub recommended_next_action: String,
}

impl InstanceStatus {
    /// Interpret a transport status and dispatch status using the canonical
    /// application policy.
    #[must_use]
    pub fn from_states(status: ServiceStatus, dispatch_status: DispatchStatus) -> Self {
        let (retryable, recommended_next_action) = actionability(status, dispatch_status);
        Self {
            status,
            dispatch_status,
            retryable,
            recommended_next_action: recommended_next_action.to_string(),
        }
    }
}

fn actionability(status: ServiceStatus, dispatch_status: DispatchStatus) -> (bool, &'static str) {
    match (status, dispatch_status) {
        (ServiceStatus::Available, DispatchStatus::Ready) => {
            (true, "Instance is available for dispatch.")
        }
        (ServiceStatus::Available, DispatchStatus::Pending) => {
            (true, "Wait for instance to report dispatch_status=ready.")
        }
        (ServiceStatus::Available, DispatchStatus::Failed) => (
            false,
            "Inspect instance failure stage/reason; the backend may need a restart.",
        ),
        (ServiceStatus::Available, DispatchStatus::Unknown) => (
            true,
            "Dispatch status not yet reported; try a direct MCP call.",
        ),
        (ServiceStatus::Busy, DispatchStatus::Ready) => {
            (true, "Instance is busy; retry after current job completes.")
        }
        (ServiceStatus::Busy, DispatchStatus::Pending) => (
            true,
            "Instance is busy and not yet dispatch-ready; wait and retry.",
        ),
        (ServiceStatus::Busy, DispatchStatus::Failed) => (
            false,
            "Instance is busy but dispatch has failed; inspect failure details.",
        ),
        (ServiceStatus::Busy, DispatchStatus::Unknown) => (true, "Instance is busy; retry later."),
        (ServiceStatus::Booting, _) => (true, "Instance is booting; wait for readiness and retry."),
        (ServiceStatus::Unreachable, _) => (
            false,
            "Instance is unreachable; check logs and restart if needed.",
        ),
        (ServiceStatus::ShuttingDown, _) => {
            (false, "Instance is shutting down; wait for a new instance.")
        }
        (ServiceStatus::Stale, _) => (
            false,
            "Instance is stale; it will be removed by the registry.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_ready_is_dispatchable() {
        let status = InstanceStatus::from_states(ServiceStatus::Available, DispatchStatus::Ready);
        assert!(status.retryable);
        assert_eq!(
            status.recommended_next_action,
            "Instance is available for dispatch."
        );
    }

    #[test]
    fn available_pending_tells_agent_to_wait() {
        let status = InstanceStatus::from_states(ServiceStatus::Available, DispatchStatus::Pending);
        assert!(status.retryable);
        assert!(
            status
                .recommended_next_action
                .contains("dispatch_status=ready")
        );
    }

    #[test]
    fn failed_dispatch_is_not_retryable() {
        let status = InstanceStatus::from_states(ServiceStatus::Available, DispatchStatus::Failed);
        assert!(!status.retryable);
        assert!(status.recommended_next_action.contains("failure stage"));
    }

    #[test]
    fn transport_failures_override_dispatch_state() {
        for service_status in [
            ServiceStatus::Unreachable,
            ServiceStatus::ShuttingDown,
            ServiceStatus::Stale,
        ] {
            let status = InstanceStatus::from_states(service_status, DispatchStatus::Ready);
            assert!(!status.retryable);
        }
    }

    #[test]
    fn status_roundtrips_through_json() {
        let status = InstanceStatus::from_states(ServiceStatus::Busy, DispatchStatus::Ready);
        let json = serde_json::to_string(&status).unwrap();
        let parsed: InstanceStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, ServiceStatus::Busy);
        assert_eq!(parsed.dispatch_status, DispatchStatus::Ready);
        assert!(parsed.retryable);
    }
}
