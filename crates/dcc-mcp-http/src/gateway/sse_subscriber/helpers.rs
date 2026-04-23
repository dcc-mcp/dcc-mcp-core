use super::*;

/// Serialise a `progressToken` (may be number or string) into a stable
/// map key.
pub(super) fn progress_token_key(token: &Value) -> String {
    match token {
        Value::String(s) => format!("s:{s}"),
        Value::Number(n) => format!("n:{n}"),
        other => format!("j:{other}"),
    }
}

/// Exponential backoff with ±25 % jitter.
pub(super) fn backoff_delay(attempt: u32) -> Duration {
    let base = RECONNECT_INITIAL.as_millis() as u64;
    // doubling, capped.
    let shift = attempt.saturating_sub(1).min(12); // 2^12 headroom
    let mut delay_ms = base.saturating_mul(1u64 << shift);
    let cap = RECONNECT_MAX.as_millis() as u64;
    if delay_ms > cap {
        delay_ms = cap;
    }
    // Pseudo-random jitter derived from attempt & current nanos so
    // multiple backends don't synchronise retries.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let entropy = (nanos.rotate_left(attempt % 64)) % 1024;
    let jitter_span = (delay_ms as f32 * RECONNECT_JITTER) as i64;
    let jitter = if jitter_span > 0 {
        (entropy as i64 % (jitter_span * 2 + 1)) - jitter_span
    } else {
        0
    };
    let final_ms = (delay_ms as i64).saturating_add(jitter).max(0) as u64;
    Duration::from_millis(final_ms)
}

/// Return the byte offset of the end of the next complete SSE record
/// (double-newline) in `buf`, relative to the buffer start.
pub(super) fn find_record_end(buf: &[u8]) -> Option<usize> {
    // Accept both "\n\n" and "\r\n\r\n" as record terminators.
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some(i);
        }
        if i + 3 < buf.len() && &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

/// Length of the terminator `find_record_end` matched. Called after a
/// successful `find_record_end` returns `Some`.
pub(super) fn record_delim_len(buf: &[u8]) -> usize {
    if buf.starts_with(b"\r\n\r\n") {
        4
    } else {
        // Standard case: the drained record already took everything up
        // to the first delimiter byte. What's left in the buffer starts
        // with the terminator itself.
        if buf.starts_with(b"\n\n") { 2 } else { 1 }
    }
}

/// Parse a single SSE record (without trailing blank line) into a JSON
/// value, returning `None` if the record has no `data:` field or the
/// payload is not valid JSON.
pub(super) fn parse_sse_record(record: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(record).ok()?;
    let mut data_lines: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            // MDN / WHATWG: the single leading space is optional.
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let joined = data_lines.join("\n");
    serde_json::from_str::<Value>(&joined).ok()
}

/// Extract the `job_id` from a `$/dcc.jobUpdated` / `workflowUpdated`
/// notification envelope. Used by the per-job broadcast bus (#321) so
/// wait-for-terminal POST handlers can block on terminal events without
/// needing their own SSE subscription.
pub(super) fn job_id_for_job_notification(value: &Value) -> Option<String> {
    let method = value.get("method").and_then(|m| m.as_str())?;
    if !matches!(
        method,
        "notifications/$/dcc.jobUpdated" | "notifications/$/dcc.workflowUpdated"
    ) {
        return None;
    }
    value
        .get("params")
        .and_then(|p| p.get("job_id"))
        .and_then(|j| j.as_str())
        .map(str::to_owned)
}

/// Extract the `job_id` from a `$/dcc.jobUpdated` / `workflowUpdated`
/// notification only when it carries a terminal status (issue #322
/// auto-eviction). Terminal statuses follow #318: `completed`,
/// `failed`, `cancelled`, `interrupted`.
pub(super) fn terminal_job_id(value: &Value) -> Option<String> {
    let method = value.get("method").and_then(|m| m.as_str())?;
    if !matches!(
        method,
        "notifications/$/dcc.jobUpdated" | "notifications/$/dcc.workflowUpdated"
    ) {
        return None;
    }
    let params = value.get("params")?;
    let status = params.get("status").and_then(|s| s.as_str())?;
    if !matches!(status, "completed" | "failed" | "cancelled" | "interrupted") {
        return None;
    }
    params
        .get("job_id")
        .and_then(|j| j.as_str())
        .map(str::to_owned)
}

/// Determine which client session should receive `value`.
pub(super) fn resolve_target(inner: &SubscriberManagerInner, value: &Value) -> Option<String> {
    let method = value.get("method").and_then(|m| m.as_str())?;
    let params = value.get("params");

    match method {
        "notifications/progress" => {
            let token = params.and_then(|p| p.get("progressToken"))?;
            inner
                .progress_token_routes
                .get(&progress_token_key(token))
                .map(|e| e.value().clone())
        }
        "notifications/$/dcc.jobUpdated" | "notifications/$/dcc.workflowUpdated" => {
            let job_id = params
                .and_then(|p| p.get("job_id"))
                .and_then(|j| j.as_str())?;
            inner
                .job_routes
                .get(job_id)
                .map(|e| e.value().client_session_id.clone())
        }
        _ => None,
    }
}
