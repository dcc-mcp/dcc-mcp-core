//! Structured remediation payloads for thread-affinity failures (#1075).

use dcc_mcp_actions::DispatchExecutionContext;
use dcc_mcp_models::ThreadAffinity;
use serde_json::{Value, json};

/// Machine-readable context attached to `thread-affinity-violation` errors.
#[must_use]
pub fn build_thread_affinity_context(
    action: &str,
    declared: ThreadAffinity,
    actual: ThreadAffinity,
    execution: Option<DispatchExecutionContext>,
) -> Value {
    let host_dispatcher_attached = execution.and_then(|c| c.host_dispatcher_attached);
    let observed_context = describe_observed_context(actual, host_dispatcher_attached);
    json!({
        "action": action,
        "declared_affinity": declared.to_string(),
        "observed_affinity": actual.to_string(),
        "observed_context": observed_context,
        "host_dispatcher_attached": host_dispatcher_attached,
        "remediation": remediation_steps(declared, actual, host_dispatcher_attached),
    })
}

/// Human-readable hint for the `hint` field on [`crate::ServiceError`].
#[must_use]
pub fn thread_affinity_hint(
    declared: ThreadAffinity,
    actual: ThreadAffinity,
    host_dispatcher_attached: Option<bool>,
) -> String {
    match (declared, actual, host_dispatcher_attached) {
        (ThreadAffinity::Main, ThreadAffinity::Any, Some(false)) => {
            "This tool requires the DCC main thread but no host dispatcher is attached. \
             In interactive hosts, load the DCC plugin or pass a UI dispatcher to start_server(); \
             note that dispatcher=true on GET /v1/readyz only confirms adapter readiness, \
             while /v1/call also needs a host QueueDispatcher/BlockingDispatcher attached before server.start()."
                .to_string()
        }
        (ThreadAffinity::Main, ThreadAffinity::Any, Some(true) | None) => {
            "This tool requires the DCC main thread. Ensure calls are routed through the host \
             dispatcher (plugin mode or MayaUiDispatcher + pump), not a raw Tokio worker."
                .to_string()
        }
        _ => {
            "Check the action tools.yaml thread_affinity and enforce_thread_affinity, or marshal \
             through the host main-thread dispatcher."
                .to_string()
        }
    }
}

/// Context for `THREAD_AFFINITY_UNAVAILABLE` handler errors.
#[must_use]
pub fn build_affinity_unavailable_context(action: &str) -> Value {
    json!({
        "action": action,
        "declared_affinity": "main",
        "observed_context": "no_host_dispatcher",
        "host_dispatcher_attached": false,
        "remediation": [
            "Attach a host QueueDispatcher/BlockingDispatcher (or plugin bridge) before server.start().",
            "For Maya GUI, prefer the bundled plugin + gateway (http://127.0.0.1:9765/mcp) instead of bare start_server(port=8765).",
        ],
    })
}

#[must_use]
pub fn affinity_unavailable_hint() -> &'static str {
    "Attach a host main-thread dispatcher before start(), or use the DCC plugin/gateway path that wires one automatically."
}

fn describe_observed_context(
    actual: ThreadAffinity,
    host_dispatcher_attached: Option<bool>,
) -> &'static str {
    match (actual, host_dispatcher_attached) {
        (ThreadAffinity::Main, _) => "main",
        (ThreadAffinity::Any, Some(false)) => "worker_no_dispatcher",
        (ThreadAffinity::Any, Some(true)) => "worker",
        (ThreadAffinity::Any, None) => "worker",
    }
}

fn remediation_steps(
    declared: ThreadAffinity,
    actual: ThreadAffinity,
    host_dispatcher_attached: Option<bool>,
) -> Vec<&'static str> {
    let mut steps = Vec::new();
    if declared == ThreadAffinity::Main && actual == ThreadAffinity::Any {
        steps.push(
            "Route the call through the host main-thread dispatcher (plugin or explicit UI dispatcher).",
        );
        if host_dispatcher_attached == Some(false) {
            steps.push(
                "Wire HostExecutionBridge with a host QueueDispatcher/BlockingDispatcher before starting the HTTP server.",
            );
            steps.push(
                "Do not rely on readiness.dispatcher alone; inspect the /v1/call error context for host_dispatcher_attached=true.",
            );
        }
    }
    if steps.is_empty() {
        steps.push(
            "Verify tools.yaml thread_affinity matches how the adapter executes the handler.",
        );
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_includes_dispatcher_flag() {
        let ctx = build_thread_affinity_context(
            "maya_scene__info",
            ThreadAffinity::Main,
            ThreadAffinity::Any,
            Some(DispatchExecutionContext {
                host_dispatcher_attached: Some(false),
            }),
        );
        assert_eq!(ctx["declared_affinity"], "main");
        assert_eq!(ctx["observed_context"], "worker_no_dispatcher");
        assert_eq!(ctx["host_dispatcher_attached"], false);
        assert!(ctx["remediation"].is_array());
    }

    #[test]
    fn dispatch_error_maps_host_dispatcher_false_on_default_rest_path() {
        use dcc_mcp_actions::{DispatchError, with_execution_context};

        use crate::dispatch_error_to_service_error;

        let err = DispatchError::ThreadAffinityViolation {
            action: "main_only".into(),
            declared: ThreadAffinity::Main,
            actual: ThreadAffinity::Any,
        };
        let svc = with_execution_context(
            DispatchExecutionContext {
                host_dispatcher_attached: Some(false),
            },
            || dispatch_error_to_service_error(err),
        );
        let ctx = svc.context.expect("context");
        assert_eq!(ctx["host_dispatcher_attached"], false);
    }

    #[test]
    fn hint_mentions_dispatcher_when_missing() {
        let hint = thread_affinity_hint(ThreadAffinity::Main, ThreadAffinity::Any, Some(false));
        assert!(hint.contains("dispatcher"));
        assert!(hint.contains("readyz") || hint.contains("gateway"));
    }

    #[test]
    fn dispatch_error_maps_to_service_error_with_context() {
        use dcc_mcp_actions::DispatchError;

        use crate::ServiceErrorKind;
        use crate::dispatch_error_to_service_error;

        let err = DispatchError::ThreadAffinityViolation {
            action: "main_only".into(),
            declared: ThreadAffinity::Main,
            actual: ThreadAffinity::Any,
        };
        let svc = dispatch_error_to_service_error(err);
        assert_eq!(svc.kind, ServiceErrorKind::ThreadAffinityViolation);
        let ctx = svc.context.expect("context");
        assert_eq!(ctx["action"], "main_only");
    }
}
