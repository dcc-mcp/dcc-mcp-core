//! Small adapter for recording gateway lifecycle events through AuditMiddleware.

use axum::http::HeaderMap;
use serde_json::Value;

use super::{CallContext, CallResult, MiddlewareChain};
use crate::gateway::admin::trace::{AgentContext, TraceContext};

pub async fn record_gateway_event(
    chain: &MiddlewareChain,
    headers: Option<&HeaderMap>,
    method: &str,
    dcc_type: Option<&str>,
    instance_id: Option<&str>,
    args: Value,
    result: CallResult,
) {
    if chain.is_empty() {
        return;
    }
    let trace_context = headers
        .map(TraceContext::from_headers)
        .unwrap_or_else(|| TraceContext::from_headers(&HeaderMap::new()));
    let agent_context = headers
        .map(|headers| AgentContext::from_request_parts_with_server_network(headers, None, None))
        .unwrap_or(None);
    let mut ctx = CallContext::new(method, trace_context.request_id.clone(), args)
        .with_transport("gateway-event")
        .with_trace_context(trace_context)
        .with_agent_context(agent_context);
    if let Some(dcc_type) = dcc_type {
        if let Some(instance_id) = instance_id {
            ctx = ctx.with_backend(dcc_type, instance_id);
        } else {
            ctx.dcc_type = Some(dcc_type.to_string());
        }
    }
    let mut result = result;
    if let Err(err) = chain.run_after(&ctx, &mut result).await {
        tracing::warn!(error = %err, method, "gateway audit event middleware failed");
    }
}
