use super::*;

/// Block the gateway's `tools/call` response until the backend reports
/// a terminal `$/dcc.jobUpdated` for `job_id`, or until the
/// [`GatewayState::wait_terminal_timeout`] elapses.
///
/// Returns the same `(text, is_error)` shape as the synchronous path so
/// the caller's wrapping into `CallToolResult` is identical.
///
/// On timeout we return the **initial `{pending}` envelope annotated
/// with `_meta.dcc.timed_out = true`** and leave the job running on the
/// backend — the caller can keep polling `jobs.get_status` or reconnect
/// SSE to collect the result later.
pub(crate) async fn wait_for_terminal_reply(
    gs: &GatewayState,
    job_id: &str,
    pending_envelope: &mut Value,
    entry: &ServiceEntry,
    timeout: Duration,
) -> (String, bool) {
    // Subscribe BEFORE we return to the caller — the publish happens
    // inside [`SubscriberManager::deliver`] regardless of any
    // client-session binding, so the only race window we need to
    // defend against is between "backend replied {pending}" and "we
    // call `.recv()` below". Binding happened in the caller via
    // `bind_job`, but the bus is independent — create it here.
    let mut rx: broadcast::Receiver<Value> = gs.subscriber.job_event_channel(job_id);

    // Capture the latest-seen job update so that on timeout we can
    // return the richest envelope we observed.
    let mut latest: Option<Value> = None;
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(value)) => {
                let status = value
                    .get("params")
                    .and_then(|p| p.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let is_terminal = TERMINAL_JOB_STATUSES
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(status));
                latest = Some(value);
                if is_terminal {
                    // Retire per-job bus + routing (best-effort — we may
                    // have an in-flight notification still buffered, but
                    // the waiter has consumed the terminal event).
                    gs.subscriber.forget_job_bus(job_id);
                    gs.subscriber.forget_job(job_id);
                    break;
                }
            }
            // Broadcast lag: the backend emitted notifications faster
            // than we could consume them. Keep going; the next call
            // will deliver the most recent events.
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            // Sender was dropped — the subscriber's backend loop tore
            // down. This is NOT a terminal state; surface a clear
            // error so the client knows the job is in limbo on the
            // backend. (#328 will later mark it `interrupted`.)
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                gs.subscriber.forget_job_bus(job_id);
                let payload = json!({
                    "error": {
                        "code": -32000,
                        "message": format!(
                            "backend disconnected during wait_for_terminal \
                             (job {job_id} still running on {})",
                            entry.dcc_type
                        ),
                        "data": {
                            "job_id": job_id,
                            "instance_id": entry.instance_id.to_string(),
                            "dcc_type": entry.dcc_type,
                        }
                    }
                });
                return (
                    serde_json::to_string_pretty(&payload).unwrap_or_default(),
                    true,
                );
            }
            // Per-iteration timeout — fall through to check deadline.
            Err(_) => break,
        }
    }

    // If we have a terminal event, build the final envelope by merging
    // the backend's job-update payload into the pending envelope.
    let envelope = match latest {
        Some(update) => merge_job_update_into_envelope(pending_envelope.clone(), &update, false),
        None => {
            // Timed out before any update arrived. Tag the pending
            // envelope so the client can distinguish "still running"
            // from "completed with empty output".
            gs.subscriber.forget_job_bus(job_id);
            merge_job_update_into_envelope(pending_envelope.clone(), &Value::Null, true)
        }
    };

    let mut final_envelope = envelope;
    inject_instance_metadata(&mut final_envelope, &entry.instance_id, &entry.dcc_type);
    envelope_to_text_result(&final_envelope)
}

/// Compose a terminal-state `CallToolResult` by layering:
/// 1. The backend's original `{pending, job_id}` envelope (preserves
///    `_meta.dcc.jobId`, `parentJobId`).
/// 2. The `$/dcc.jobUpdated` payload's `status`, `result` (if present),
///    and `error` (if present).
/// 3. Gateway flags — `_meta.dcc.timed_out` when we couldn't wait any
///    longer.
///
/// The output is a JSON object shaped like a `CallToolResult` so the
/// caller can reuse [`envelope_to_text_result`].
pub(crate) fn merge_job_update_into_envelope(mut pending: Value, update: &Value, timed_out: bool) -> Value {
    let params = update.get("params");
    let status = params
        .and_then(|p| p.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let error_text = params
        .and_then(|p| p.get("error"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let result_value = params.and_then(|p| p.get("result")).cloned();

    // Build structuredContent payload: reuse the pending object, then
    // overwrite status + result.
    let mut sc = pending
        .get("structuredContent")
        .cloned()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    if !status.is_empty() {
        sc.insert("status".to_string(), Value::String(status.to_string()));
    }
    if let Some(r) = result_value {
        sc.insert("result".to_string(), r);
    }
    if let Some(ref e) = error_text {
        sc.insert("error".to_string(), Value::String(e.clone()));
    }

    // Merge _meta.
    let mut meta = sc
        .get("_meta")
        .cloned()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let mut dcc_meta = meta
        .get("dcc")
        .cloned()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    if !status.is_empty() {
        dcc_meta.insert("status".to_string(), Value::String(status.to_string()));
    }
    if timed_out {
        dcc_meta.insert("timed_out".to_string(), Value::Bool(true));
    }
    meta.insert("dcc".to_string(), Value::Object(dcc_meta));
    sc.insert("_meta".to_string(), Value::Object(meta));

    // Build a human-readable text body so the CallToolResult still
    // has a non-empty `content`.
    let text = if timed_out {
        format!("wait_for_terminal: timeout — job still running (status={status})")
    } else if let Some(err) = error_text.as_deref() {
        format!("Job {}: {err}", status)
    } else {
        format!(
            "Job {status} — {}",
            sc.get("result")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "(no structured result)".to_string())
        )
    };

    let is_error = matches!(status, "failed" | "cancelled" | "interrupted") || timed_out;
    if let Some(obj) = pending.as_object_mut() {
        obj.insert("structuredContent".to_string(), Value::Object(sc));
        obj.insert("isError".to_string(), Value::Bool(is_error));
        obj.insert(
            "content".to_string(),
            json!([{ "type": "text", "text": text }]),
        );
    }
    pending
}
