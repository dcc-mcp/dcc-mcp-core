use super::rest_support::*;
use super::*;

use crate::gateway::admin::trace::{
    AgentContext, MAX_INPUT_BYTES, MAX_OUTPUT_BYTES, TraceContext, TracePayload,
};
use crate::gateway::agent_telemetry::{AgentWorkflowEvent, policy_reason_from_value};
use crate::gateway::capability::{RefreshReason, parse_slug};
use crate::gateway::capability_service::{
    ServiceError, call_service, refresh_all_live_backends, service_error_to_json,
};
use crate::gateway::middleware::{CallContext, CallResult};
use crate::gateway::response_codec::compact_call_batch_payload;
use crate::gateway::search_telemetry::search_id_from_payload;

pub(super) struct RestCallTraceRequest<'a> {
    pub(super) method: &'a str,
    pub(super) slug: &'a str,
    pub(super) arguments: Value,
    pub(super) meta: Option<Value>,
    pub(super) request_body: &'a Value,
    pub(super) trace_context: TraceContext,
}

pub(super) async fn call_service_with_admin_trace(
    gs: &GatewayState,
    headers: &HeaderMap,
    request: RestCallTraceRequest<'_>,
) -> Result<Value, ServiceError> {
    let RestCallTraceRequest {
        method,
        slug,
        arguments,
        meta,
        request_body,
        trace_context,
    } = request;
    let search_id = search_id_from_payload(request_body);

    let mut ctx = CallContext::new(method, trace_context.request_id.clone(), arguments.clone())
        .with_tool_slug(slug)
        .with_transport("rest")
        .with_agent_context(AgentContext::from_request_parts_with_server_network(
            headers,
            Some(request_body),
            meta.as_ref(),
        ))
        .with_trace_context(trace_context);
    if let Some(session_id) = session_id_from_headers(headers) {
        ctx = ctx.with_session_id(session_id);
    }
    // Parse optional upstream LLM billing token counts from header.
    if let Some(raw) = headers
        .get("x-dcc-mcp-llm-usage")
        .and_then(|v| v.to_str().ok())
    {
        ctx.llm_usage = serde_json::from_str(raw).ok();
    }
    if let Some((dcc_type, instance_hint, _)) = parse_slug(slug) {
        ctx = ctx.with_backend(dcc_type, instance_hint);
    }
    if !gs.middleware_chain.is_empty()
        && let Err(err) = gs.middleware_chain.run_before(&mut ctx).await
    {
        let message = err.to_string();
        crate::gateway::metrics::record_gateway_governance_event(
            err.governance_category(),
            err.kind(),
        );
        ctx.input_payload = Some(TracePayload::from_input_value(&ctx.args, MAX_INPUT_BYTES));
        ctx.output_payload = Some(TracePayload::from_str(&message, MAX_OUTPUT_BYTES));
        record_token_accounting(
            &mut ctx,
            gs,
            headers,
            request_body,
            service_error_to_json(&ServiceError::new(err.kind(), message.clone())),
            None,
            false,
        );
        let mut rejected = CallResult::from_tuple(&message, true);
        if let Err(after_err) = gs.middleware_chain.run_after(&ctx, &mut rejected).await {
            tracing::warn!(error = %after_err, "gateway middleware after-call failed for rejected REST call");
        }
        AgentWorkflowEvent::new("gateway.call", "rest")
            .with_trace_context(Some(&ctx.trace_context))
            .with_agent_context(ctx.agent_context.as_ref())
            .with_session_id(ctx.session_id.as_deref())
            .with_route(
                Some(slug),
                None,
                ctx.dcc_type.as_deref(),
                ctx.instance_id.as_deref(),
            )
            .with_outcome(false, Some(err.kind()))
            .with_policy_reason(Some(err.governance_category()))
            .emit();
        return Err(ServiceError::new(err.kind(), message));
    }

    ctx.input_payload = Some(TracePayload::from_input_value(&ctx.args, MAX_INPUT_BYTES));

    let effective_arguments = if gs.middleware_chain.is_empty() {
        arguments
    } else {
        ctx.args.clone()
    };
    emit_rest_traffic_frame(
        gs,
        &ctx,
        headers,
        RestTrafficFrame {
            path: method,
            direction: "inbound",
            leg: "client_to_gateway",
            status: None,
            body: json!({
            "tool_slug": slug,
            "arguments": effective_arguments.clone(),
            "meta": meta.clone(),
            }),
        },
    );
    let dispatch_ns = now_ns();
    let mut result = call_service(
        gs,
        slug,
        effective_arguments.clone(),
        meta.clone(),
        Some(&ctx.trace_context),
        ctx.agent_context.as_ref(),
    )
    .await;
    if matches!(&result, Err(err) if err.kind == "unknown-slug") {
        refresh_all_live_backends(gs, RefreshReason::Periodic).await;
        result = call_service(
            gs,
            slug,
            effective_arguments,
            meta,
            Some(&ctx.trace_context),
            ctx.agent_context.as_ref(),
        )
        .await;
    }

    let response_ns = now_ns();
    record_search_followup(
        gs,
        search_id.as_deref(),
        "call",
        Some(slug),
        None,
        result.is_ok(),
        &ctx.trace_context,
    );
    let (preview_text, is_error, output_value) = match &result {
        Ok(value) => (
            serde_json::to_string(value).unwrap_or_default(),
            false,
            value.clone(),
        ),
        Err(err) => (err.message.clone(), true, service_error_to_json(err)),
    };
    let mut span = ctx
        .trace_context
        .child_span(
            "backend.execute",
            dispatch_ns,
            response_ns.saturating_sub(dispatch_ns),
        )
        .with_attr("tool_slug", slug)
        .with_attr("transport", "rest")
        .with_attr("ok", !is_error);
    if is_error {
        span = span.with_error();
    }
    ctx.push_span(span);
    ctx.output_payload = Some(TracePayload::from_value(&output_value, MAX_OUTPUT_BYTES));
    record_token_accounting(
        &mut ctx,
        gs,
        headers,
        request_body,
        output_value.clone(),
        None,
        false,
    );

    let mut call_result = CallResult::from_tuple(preview_text, is_error);
    if !gs.middleware_chain.is_empty()
        && let Err(err) = gs.middleware_chain.run_after(&ctx, &mut call_result).await
    {
        let selected_hit = selected_search_hit(gs, search_id.as_deref(), Some(slug), None);
        AgentWorkflowEvent::new("gateway.call", "rest")
            .with_trace_context(Some(&ctx.trace_context))
            .with_agent_context(ctx.agent_context.as_ref())
            .with_session_id(ctx.session_id.as_deref())
            .with_search_id(search_id.as_deref())
            .with_ranker_version(Some(crate::gateway::search_telemetry::RANKER_VERSION))
            .with_route(
                Some(slug),
                None,
                ctx.dcc_type.as_deref(),
                ctx.instance_id.as_deref(),
            )
            .with_selected_hit(selected_hit.as_ref())
            .with_outcome(false, Some("middleware-error"))
            .emit();
        return Err(ServiceError::new("middleware-error", err.to_string()));
    }

    let selected_hit = selected_search_hit(gs, search_id.as_deref(), Some(slug), None);
    let error_kind = result.as_ref().err().map(|err| err.kind.as_str());
    let error_payload = result
        .as_ref()
        .err()
        .map(crate::gateway::capability_service::service_error_to_json);
    let policy_reason = error_payload.as_ref().and_then(policy_reason_from_value);
    AgentWorkflowEvent::new("gateway.call", "rest")
        .with_trace_context(Some(&ctx.trace_context))
        .with_agent_context(ctx.agent_context.as_ref())
        .with_session_id(ctx.session_id.as_deref())
        .with_search_id(search_id.as_deref())
        .with_ranker_version(Some(crate::gateway::search_telemetry::RANKER_VERSION))
        .with_route(
            Some(slug),
            None,
            ctx.dcc_type.as_deref(),
            ctx.instance_id.as_deref(),
        )
        .with_selected_hit(selected_hit.as_ref())
        .with_outcome(!is_error, error_kind)
        .with_policy_reason(policy_reason.as_deref())
        .emit();

    emit_rest_traffic_frame(
        gs,
        &ctx,
        headers,
        RestTrafficFrame {
            path: method,
            direction: "outbound",
            leg: "gateway_to_client",
            status: Some(if is_error { 502 } else { 200 }),
            body: output_value,
        },
    );

    result
}

pub(super) async fn call_batch_with_admin_trace(
    gs: &GatewayState,
    headers: &HeaderMap,
    request_body: &Value,
    trace_context: TraceContext,
) -> Result<Value, ServiceError> {
    let mut ctx = CallContext::new(
        "v1/call_batch",
        trace_context.request_id.clone(),
        request_body.clone(),
    )
    .with_tool_slug("call_batch")
    .with_transport("rest")
    .with_agent_context(AgentContext::from_request_parts_with_server_network(
        headers,
        Some(request_body),
        request_body.get("meta"),
    ))
    .with_trace_context(trace_context);
    if let Some(session_id) = session_id_from_headers(headers) {
        ctx = ctx.with_session_id(session_id);
    }
    if !gs.middleware_chain.is_empty()
        && let Err(err) = gs.middleware_chain.run_before(&mut ctx).await
    {
        let message = err.to_string();
        crate::gateway::metrics::record_gateway_governance_event(
            err.governance_category(),
            err.kind(),
        );
        ctx.input_payload = Some(TracePayload::from_input_value(&ctx.args, MAX_INPUT_BYTES));
        ctx.output_payload = Some(TracePayload::from_str(&message, MAX_OUTPUT_BYTES));
        record_token_accounting(
            &mut ctx,
            gs,
            headers,
            request_body,
            service_error_to_json(&ServiceError::new(err.kind(), message.clone())),
            None,
            true,
        );
        let mut rejected = CallResult::from_tuple(&message, true);
        if let Err(after_err) = gs.middleware_chain.run_after(&ctx, &mut rejected).await {
            tracing::warn!(error = %after_err, "gateway middleware after-call failed for rejected REST batch");
        }
        AgentWorkflowEvent::new("gateway.call_batch", "rest")
            .with_trace_context(Some(&ctx.trace_context))
            .with_agent_context(ctx.agent_context.as_ref())
            .with_session_id(ctx.session_id.as_deref())
            .with_route(Some("call_batch"), None, None, None)
            .with_outcome(false, Some(err.kind()))
            .with_policy_reason(Some(err.governance_category()))
            .emit();
        return Err(ServiceError::new(err.kind(), message));
    }

    ctx.input_payload = Some(TracePayload::from_input_value(&ctx.args, MAX_INPUT_BYTES));
    emit_rest_traffic_frame(
        gs,
        &ctx,
        headers,
        RestTrafficFrame {
            path: "v1/call_batch",
            direction: "inbound",
            leg: "client_to_gateway",
            status: None,
            body: ctx.args.clone(),
        },
    );

    let dispatch_ns = now_ns();
    let result = crate::gateway::tools::gateway_call_batch_inner(
        gs,
        &ctx.args,
        request_body.get("meta"),
        Some(&ctx.trace_context),
        ctx.agent_context.as_ref(),
    )
    .await;
    let response_ns = now_ns();
    let (preview_text, is_error, output_value) = match &result {
        Ok(value) => (
            serde_json::to_string(value).unwrap_or_default(),
            false,
            value.clone(),
        ),
        Err(message) => (
            message.clone(),
            true,
            json!({
                "success": false,
                "error": {"kind": "bad-request", "message": message},
            }),
        ),
    };
    let mut span = ctx
        .trace_context
        .child_span(
            "batch.execute",
            dispatch_ns,
            response_ns.saturating_sub(dispatch_ns),
        )
        .with_attr("tool_slug", "call_batch")
        .with_attr("transport", "rest")
        .with_attr("ok", !is_error);
    if is_error {
        span = span.with_error();
    }
    ctx.push_span(span);
    ctx.output_payload = Some(TracePayload::from_value(&output_value, MAX_OUTPUT_BYTES));
    let compact_output = result.as_ref().ok().map(compact_call_batch_payload);
    record_token_accounting(
        &mut ctx,
        gs,
        headers,
        request_body,
        output_value.clone(),
        compact_output,
        true,
    );

    let mut call_result = CallResult::from_tuple(preview_text, is_error);
    let search_id = search_id_from_payload(request_body);
    let batch_size = call_batch_size(request_body);
    let first_slug = first_batch_tool_slug(request_body);
    let selected_hit = selected_search_hit(gs, search_id.as_deref(), first_slug, None);
    if !gs.middleware_chain.is_empty()
        && let Err(err) = gs.middleware_chain.run_after(&ctx, &mut call_result).await
    {
        AgentWorkflowEvent::new("gateway.call_batch", "rest")
            .with_trace_context(Some(&ctx.trace_context))
            .with_agent_context(ctx.agent_context.as_ref())
            .with_session_id(ctx.session_id.as_deref())
            .with_search_id(search_id.as_deref())
            .with_ranker_version(Some(crate::gateway::search_telemetry::RANKER_VERSION))
            .with_route(first_slug, None, None, None)
            .with_selected_hit(selected_hit.as_ref())
            .with_batch_size(batch_size)
            .with_outcome(false, Some("middleware-error"))
            .emit();
        return Err(ServiceError::new(err.kind(), err.to_string()));
    }

    let batch_success = result
        .as_ref()
        .ok()
        .and_then(|value| value.get("success"))
        .and_then(Value::as_bool)
        .unwrap_or(!is_error);
    AgentWorkflowEvent::new("gateway.call_batch", "rest")
        .with_trace_context(Some(&ctx.trace_context))
        .with_agent_context(ctx.agent_context.as_ref())
        .with_session_id(ctx.session_id.as_deref())
        .with_search_id(search_id.as_deref())
        .with_ranker_version(Some(crate::gateway::search_telemetry::RANKER_VERSION))
        .with_route(first_slug, None, None, None)
        .with_selected_hit(selected_hit.as_ref())
        .with_batch_size(batch_size)
        .with_outcome(
            batch_success,
            if batch_success {
                None
            } else if result.is_err() {
                Some("bad-request")
            } else {
                Some("call-failed")
            },
        )
        .with_policy_reason(
            result
                .as_ref()
                .ok()
                .and_then(policy_reason_from_value)
                .as_deref(),
        )
        .emit();

    emit_rest_traffic_frame(
        gs,
        &ctx,
        headers,
        RestTrafficFrame {
            path: "v1/call_batch",
            direction: "outbound",
            leg: "gateway_to_client",
            status: Some(if is_error { 400 } else { 200 }),
            body: output_value,
        },
    );

    result.map_err(|message| ServiceError::new("bad-request", message))
}

fn selected_search_hit(
    gs: &GatewayState,
    search_id: Option<&str>,
    tool_slug: Option<&str>,
    skill_name: Option<&str>,
) -> Option<crate::gateway::search_telemetry::SearchTelemetryHit> {
    search_id.and_then(|search_id| {
        gs.search_telemetry
            .selected_hit(search_id, tool_slug, skill_name)
    })
}

fn first_batch_tool_slug(request_body: &Value) -> Option<&str> {
    request_body
        .get("calls")
        .and_then(Value::as_array)
        .and_then(|calls| calls.first())
        .and_then(|call| call.get("tool_slug"))
        .and_then(Value::as_str)
}

fn call_batch_size(request_body: &Value) -> Option<usize> {
    request_body
        .get("calls")
        .and_then(Value::as_array)
        .map(Vec::len)
}
