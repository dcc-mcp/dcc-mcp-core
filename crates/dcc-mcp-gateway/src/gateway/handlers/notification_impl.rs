use super::*;

pub(crate) async fn handle_notification(
    gs: &GatewayState,
    req: &JsonRpcRequest,
    _session_id: &str,
) {
    match req.method.as_str() {
        "notifications/initialized" => {}
        "notifications/cancelled" => {
            let request_id = req
                .params
                .as_ref()
                .and_then(|params| params.get("requestId"))
                .cloned();
            if let Some(request_id) = request_id {
                cascade_cancel(gs, request_id).await;
            }
        }
        other => {
            tracing::debug!(method = other, "Gateway received unknown MCP notification");
        }
    }
}

async fn cascade_cancel(gs: &GatewayState, request_id: Value) {
    let request_id_str = serde_json::to_string(&request_id).unwrap_or_default();
    let mut forwarded_to: std::collections::HashSet<String> = std::collections::HashSet::new();

    let job_id = gs.subscriber.job_id_for_request(&request_id_str);
    let route = job_id
        .as_deref()
        .and_then(|job_id| gs.subscriber.job_route(job_id));

    if let Some(route) = route.clone() {
        forward_cancel(gs, &route.backend_id, &request_id, &route.tool).await;
        forwarded_to.insert(route.backend_id.clone());

        if let Some(parent) = route.parent_job_id.as_deref() {
            for (child_job_id, child_route) in gs.subscriber.children_of(parent) {
                if forwarded_to.insert(child_route.backend_id.clone()) {
                    tracing::debug!(
                        parent = parent,
                        child = %child_job_id,
                        backend = %child_route.backend_id,
                        "gateway: cascading cancel to child job"
                    );
                    forward_cancel(gs, &child_route.backend_id, &request_id, &child_route.tool)
                        .await;
                }
            }
        }
    }

    if let Some(job_id) = job_id.as_deref() {
        for (child_job_id, child_route) in gs.subscriber.children_of(job_id) {
            if forwarded_to.insert(child_route.backend_id.clone()) {
                tracing::debug!(
                    parent = job_id,
                    child = %child_job_id,
                    backend = %child_route.backend_id,
                    "gateway: cascading cancel from parent"
                );
                forward_cancel(gs, &child_route.backend_id, &request_id, &child_route.tool).await;
            }
        }
    }

    if forwarded_to.is_empty() {
        let pending = gs.pending_calls.read().await;
        if let Some(call) = pending.get(&request_id_str)
            && !call.backend_url.is_empty()
        {
            forward_cancel(gs, &call.backend_url, &request_id, "").await;
        }
    }
}

/// POST `notifications/cancelled { requestId }` to `backend_url`.
async fn forward_cancel(gs: &GatewayState, backend_url: &str, request_id: &Value, tool: &str) {
    if backend_url.is_empty() {
        return;
    }

    let body = json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": {"requestId": request_id.clone()}
    });
    if let Err(err) = gs
        .http_client
        .post(backend_url)
        .header("content-type", "application/json")
        .body(body.to_string())
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        tracing::debug!(
            backend = backend_url,
            tool = tool,
            error = %err,
            "gateway: notifications/cancelled forward failed"
        );
    }
}
