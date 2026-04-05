//! Convenience wrappers around `tracing` spans for the DCC-MCP ecosystem.
//!
//! These helpers ensure that spans always include the standard DCC-MCP
//! attributes ([`crate::types::span_keys`]) and are named consistently.

use tracing::{Instrument, Span};

use crate::types::span_keys;

/// Create a `tracing` span for a DCC action execution.
///
/// # Arguments
///
/// * `action_name` — name of the action being executed (e.g. `"create_sphere"`)
/// * `dcc_name`   — name of the target DCC application (e.g. `"maya"`)
///
/// # Example
///
/// ```text
/// use dcc_mcp_telemetry::span::action_span;
///
/// let span = action_span("create_sphere", "maya");
/// let _guard = span.enter();
/// // ... execute action ...
/// ```
pub fn action_span(action_name: &str, dcc_name: &str) -> Span {
    tracing::info_span!(
        "dcc_mcp.action",
        { span_keys::ACTION_NAME } = action_name,
        { span_keys::DCC_NAME } = dcc_name,
    )
}

/// Create a `tracing` span for a DCC transport operation.
pub fn transport_span(protocol: &str, dcc_name: &str) -> Span {
    tracing::info_span!(
        "dcc_mcp.transport",
        { span_keys::TRANSPORT_PROTOCOL } = protocol,
        { span_keys::DCC_NAME } = dcc_name,
    )
}

/// Create a `tracing` span for a skill loading operation.
pub fn skill_span(skill_name: &str, dcc_name: &str) -> Span {
    tracing::info_span!(
        "dcc_mcp.skill",
        { span_keys::SKILL_NAME } = skill_name,
        { span_keys::DCC_NAME } = dcc_name,
    )
}

/// Instrument a future with a DCC action span.
///
/// # Example
///
/// ```text
/// use dcc_mcp_telemetry::span::instrument_action;
///
/// let fut = async { Ok::<_, ()>(42) };
/// let instrumented = instrument_action(fut, "create_sphere", "maya");
/// ```
pub fn instrument_action<F>(
    future: F,
    action_name: &str,
    dcc_name: &str,
) -> impl std::future::Future<Output = F::Output>
where
    F: std::future::Future,
{
    future.instrument(action_span(action_name, dcc_name))
}

/// Instrument a future with a DCC transport span.
pub fn instrument_transport<F>(
    future: F,
    protocol: &str,
    dcc_name: &str,
) -> impl std::future::Future<Output = F::Output>
where
    F: std::future::Future,
{
    future.instrument(transport_span(protocol, dcc_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_span_creation {
        use super::*;

        #[test]
        fn action_span_creates_without_panic() {
            let _span = action_span("create_sphere", "maya");
        }

        #[test]
        fn transport_span_creates_without_panic() {
            let _span = transport_span("named_pipe", "maya");
        }

        #[test]
        fn skill_span_creates_without_panic() {
            let _span = skill_span("my_skill", "blender");
        }

        #[test]
        fn action_span_is_valid() {
            let span = action_span("render_frame", "houdini");
            // The span should be valid (not None) even without a subscriber
            // registered — tracing does not panic in this case.
            let _guard = span.entered();
        }

        #[test]
        fn transport_span_can_be_entered() {
            let span = transport_span("tcp", "unreal");
            let _guard = span.entered();
        }
    }

    mod test_instrument_action {
        use super::*;

        #[tokio::test]
        async fn instrument_action_preserves_output() {
            let fut = async { 42u32 };
            let result = instrument_action(fut, "my_action", "maya").await;
            assert_eq!(result, 42);
        }

        #[tokio::test]
        async fn instrument_transport_preserves_output() {
            let fut = async { "hello" };
            let result = instrument_transport(fut, "tcp", "maya").await;
            assert_eq!(result, "hello");
        }
    }
}
