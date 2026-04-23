use super::*;

/// Common envelope-to-text extraction used by both the sync and wait-
/// for-terminal paths. Keeps the gateway's response shape a single
/// `CallToolResult` rather than a nested envelope.
pub(crate) fn envelope_to_text_result(result: &Value) -> (String, bool) {
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(result).unwrap_or_default());
    (text, is_error)
}

/// Detect whether the outbound `tools/call` has signalled async
/// dispatch opt-in (#318 + #321). Any of the three signals listed in
/// the backend handler (`handler.rs::should_dispatch_async`) triggers
/// the longer gateway timeout — we do NOT need to consult the tool's
/// `ActionMeta` here because the backend will do so itself; if none of
/// these signals are present the call will always be synchronous and
/// the short timeout is correct.
pub(crate) fn meta_signals_async_dispatch(meta: Option<&Value>) -> bool {
    let Some(m) = meta else {
        return false;
    };
    let async_flag = m
        .get("dcc")
        .and_then(|d| d.get("async"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_progress_token = m.get("progressToken").is_some();
    async_flag || has_progress_token
}

/// Detect the `_meta.dcc.wait_for_terminal = true` opt-in (#321).
pub(crate) fn meta_wants_wait_for_terminal(meta: Option<&Value>) -> bool {
    meta.and_then(|m| m.get("dcc"))
        .and_then(|d| d.get("wait_for_terminal"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Remove gateway-only bookkeeping keys from a `_meta` value before we
/// forward it to the backend (`wait_for_terminal` is useless to the
/// backend — keep the wire protocol clean).
pub(crate) fn strip_gateway_meta_flags(mut meta: Value) -> Value {
    if let Some(dcc) = meta.get_mut("dcc").and_then(Value::as_object_mut) {
        dcc.remove("wait_for_terminal");
    }
    meta
}

/// Extract the `job_id` from a backend `tools/call` result envelope, if
/// the backend enqueued an async job. Returns `None` when the tool ran
/// synchronously.
pub(crate) fn extract_job_id(result: &Value) -> Option<String> {
    if let Some(s) = result
        .get("structuredContent")
        .and_then(|c| c.get("job_id"))
        .and_then(Value::as_str)
    {
        return Some(s.to_owned());
    }
    if let Some(s) = result
        .get("_meta")
        .and_then(|m| m.get("dcc"))
        .and_then(|d| d.get("jobId"))
        .and_then(Value::as_str)
    {
        return Some(s.to_owned());
    }
    None
}

pub(crate) async fn live_backends(gs: &GatewayState) -> Vec<ServiceEntry> {
    let reg = gs.registry.read().await;
    gs.live_instances(&reg)
        .into_iter()
        .filter(|e| e.dcc_type != super::GATEWAY_SENTINEL_DCC_TYPE)
        .collect()
}

pub(crate) async fn targets_for_fanout(gs: &GatewayState, dcc_filter: Option<&str>) -> Vec<ServiceEntry> {
    live_backends(gs)
        .await
        .into_iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
        .collect()
}

pub(crate) async fn find_instance_by_prefix(gs: &GatewayState, prefix: &str) -> Option<ServiceEntry> {
    live_backends(gs)
        .await
        .into_iter()
        .find(|e| instance_short(&e.instance_id) == prefix)
}

pub(crate) async fn resolve_target(
    gs: &GatewayState,
    instance_id: Option<&str>,
    dcc_filter: Option<&str>,
) -> Result<ServiceEntry, String> {
    let candidates = live_backends(gs).await;

    // Exact or prefix match on instance_id.
    if let Some(iid) = instance_id {
        if let Some(e) = candidates.iter().find(|e| {
            let full = e.instance_id.to_string();
            full == iid || full.starts_with(iid) || instance_short(&e.instance_id) == iid
        }) {
            return Ok(e.clone());
        }
        return Err(format!("No live instance matches instance_id='{iid}'"));
    }

    // DCC-filtered auto-select when unambiguous.
    let filtered: Vec<&ServiceEntry> = candidates
        .iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
        .collect();

    match filtered.len() {
        0 => Err(match dcc_filter {
            Some(d) => format!("No live '{d}' instance."),
            None => "No live DCC instances.".to_string(),
        }),
        1 => Ok(filtered[0].clone()),
        _ => Err(format!(
            "Ambiguous target — {} instances live. Pass `instance_id` (or use `dcc` filter if only one of that type).",
            filtered.len()
        )),
    }
}

pub(crate) fn to_text_result(res: Result<String, String>) -> (String, bool) {
    match res {
        Ok(text) => (text, false),
        Err(msg) => (msg, true),
    }
}

pub(crate) fn inject_instance_metadata(value: &mut Value, iid: &Uuid, dcc_type: &str) {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("_instance_id".to_string(), Value::String(iid.to_string()));
        obj.insert(
            "_instance_short".to_string(),
            Value::String(instance_short(iid)),
        );
        obj.insert("_dcc_type".to_string(), Value::String(dcc_type.to_string()));
    }
}
