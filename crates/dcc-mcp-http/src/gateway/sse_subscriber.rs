//! Backend SSE subscription + multiplexing (#320).
//!
//! This module lets the gateway multiplex notifications emitted by each
//! backend DCC server (`notifications/progress`, `$/dcc.jobUpdated`,
//! `$/dcc.workflowUpdated`) back to the *originating* client sessions.
//!
//! # Architecture
//!
//! ```text
//!   client_session_A ──────┐
//!                           \     ┌─────────── gateway ─────────────┐
//!   client_session_B ────────┼──>│ SubscriberManager               │
//!                           /    │   backends: url → BackendSub     │
//!   client_session_C ──────┘    │   job_routes: jobId → session     │
//!                               │   progress_routes: tok → session  │
//!                               │   inflight: url → {sessions}      │
//!                               │   client_sinks: session → tx       │
//!                               └─────┬───────────────┬────────────┘
//!                                     │ GET /mcp (backend)           │
//!                                     ▼                              ▼
//!                                backend-1 SSE               backend-2 SSE
//! ```
//!
//! ## Correlation
//!
//! * `notifications/progress` carries `params.progressToken` — resolve against
//!   `progress_token_routes` (set at outbound `tools/call` time).
//! * `$/dcc.jobUpdated` / `$/dcc.workflowUpdated` carries `params.job_id` —
//!   resolve against `job_routes` (set from `_meta.dcc.jobId` on the reply).
//!
//! If a notification arrives before either correlation is known it is
//! buffered for up to 30 s (or 256 events, whichever comes first) and
//! replayed once the mapping appears; otherwise dropped with a `warn!`.
//!
//! ## Reconnect
//!
//! Each [`BackendSubscriber`] owns an exponential-backoff retry loop
//! (start 100 ms → max 10 s, 25 % jitter). When a broken stream is
//! restored the subscriber emits a synthetic `$/dcc.gatewayReconnect`
//! notification to every client that had an in-flight job on that
//! backend (tracked in `backend_inflight`).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::{DashMap, DashSet};
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::protocol::format_sse_event;

/// How long a notification with an unknown target may sit in the pending
/// buffer before being dropped.
pub(crate) const PENDING_BUFFER_TTL: Duration = Duration::from_secs(30);

/// Maximum number of notifications with unknown target buffered per backend.
pub(crate) const PENDING_BUFFER_CAP: usize = 256;

/// Initial reconnect delay after the backend SSE stream dies.
pub(crate) const RECONNECT_INITIAL: Duration = Duration::from_millis(100);

/// Ceiling on the reconnect delay.
pub(crate) const RECONNECT_MAX: Duration = Duration::from_secs(10);

/// Jitter multiplier applied to each reconnect delay (±25 %).
pub(crate) const RECONNECT_JITTER: f32 = 0.25;

/// Request timeout used when opening the backend SSE stream.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Identifier for a client-side MCP session.
pub type ClientSessionId = String;

/// A notification buffered while its target mapping is still unknown.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    inserted_at: Instant,
    value: Value,
}

/// Per-backend subscription state.
///
/// Public fields are `pub(crate)` so the gateway's `start_gateway_tasks`
/// can spawn / abort the reconnect loop directly.
pub struct BackendSubscriber {
    /// Absolute URL of the backend MCP endpoint (`http://host:port/mcp`).
    #[allow(dead_code)]
    pub(crate) url: String,
    /// Reconnect loop JoinHandle. `None` when the subscriber was never
    /// started or has been aborted.
    pub(crate) task: Option<JoinHandle<()>>,
    /// Shared state with the reconnect task.
    pub(crate) shared: Arc<BackendShared>,
}

impl BackendSubscriber {
    /// Abort the reconnect loop. Idempotent.
    pub fn abort(&mut self) {
        if let Some(h) = self.task.take() {
            h.abort();
        }
    }
}

impl Drop for BackendSubscriber {
    fn drop(&mut self) {
        self.abort();
    }
}

/// Shared state for a single backend's reconnect loop.
pub(crate) struct BackendShared {
    /// Backend URL for logging.
    pub(crate) url: String,
    /// Per-backend bounded buffer of notifications whose target session
    /// could not yet be resolved.
    pub(crate) pending: Mutex<VecDeque<Pending>>,
    /// Number of consecutive reconnect attempts (reset on a successful
    /// open of the SSE stream).
    pub(crate) reconnect_attempts: Mutex<u32>,
}

impl BackendShared {
    fn new(url: String) -> Self {
        Self {
            url,
            pending: Mutex::new(VecDeque::with_capacity(PENDING_BUFFER_CAP)),
            reconnect_attempts: Mutex::new(0),
        }
    }
}

/// Central multiplexer for backend SSE streams.
///
/// A single instance is owned by [`crate::gateway::state::GatewayState`] and
/// shared across every gateway handler. `ensure_subscribed` is the
/// single entry point callers need — the first request for a given
/// backend URL spawns a long-lived reconnect task; subsequent requests
/// are cheap DashMap lookups.
#[derive(Clone)]
pub struct SubscriberManager {
    inner: Arc<SubscriberManagerInner>,
}

struct SubscriberManagerInner {
    backends: DashMap<String, BackendSubscriber>,
    /// `job_id` → owning client session.
    job_routes: DashMap<String, ClientSessionId>,
    /// `progressToken` (serialised JSON) → owning client session.
    progress_token_routes: DashMap<String, ClientSessionId>,
    /// Backend URL → set of client sessions with in-flight jobs on that
    /// backend. Used for `$/dcc.gatewayReconnect` fan-out.
    backend_inflight: DashMap<String, DashSet<ClientSessionId>>,
    /// Client session → broadcast::Sender used by the GET /mcp handler.
    client_sinks: DashMap<ClientSessionId, broadcast::Sender<String>>,
    /// Shared HTTP client with connection pooling.
    http_client: reqwest::Client,
}

impl Default for SubscriberManager {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

impl SubscriberManager {
    pub fn new(http_client: reqwest::Client) -> Self {
        Self {
            inner: Arc::new(SubscriberManagerInner {
                backends: DashMap::new(),
                job_routes: DashMap::new(),
                progress_token_routes: DashMap::new(),
                backend_inflight: DashMap::new(),
                client_sinks: DashMap::new(),
                http_client,
            }),
        }
    }

    // ── Client-side API ────────────────────────────────────────────────

    /// Register (or replace) the broadcast::Sender used to deliver SSE
    /// events to `session_id`. Returns a fresh receiver that the
    /// GET /mcp handler can forward onto its axum SSE stream.
    pub fn register_client(&self, session_id: &str) -> broadcast::Receiver<String> {
        let (tx, rx) = broadcast::channel::<String>(128);
        self.inner.client_sinks.insert(session_id.to_string(), tx);
        rx
    }

    /// Remove a client session and drop its sink. Any future
    /// notifications destined for this session are dropped silently.
    pub fn forget_client(&self, session_id: &str) {
        self.inner.client_sinks.remove(session_id);
        // Scrub the backend_inflight index so a later reconnect on some
        // backend does not try to notify this long-gone session.
        for entry in self.inner.backend_inflight.iter() {
            entry.value().remove(session_id);
        }
        // Scrub routing tables to avoid memory growth. We don't scan
        // `progress_token_routes` keys eagerly (tokens are short-lived)
        // but removing job_routes bound to this session is cheap.
        self.inner
            .job_routes
            .retain(|_, sid| sid.as_str() != session_id);
        self.inner
            .progress_token_routes
            .retain(|_, sid| sid.as_str() != session_id);
    }

    // ── Correlation updates ────────────────────────────────────────────

    /// Associate a `progressToken` seen on the outbound `tools/call`
    /// with the initiating client session.
    pub fn bind_progress_token(&self, token: &Value, session_id: &str) {
        let key = progress_token_key(token);
        self.inner
            .progress_token_routes
            .insert(key, session_id.to_string());
    }

    /// Associate a `job_id` extracted from a backend reply with its
    /// owning client session. Also registers the session as having an
    /// in-flight job on `backend_url` so that a later reconnect on that
    /// backend can emit `$/dcc.gatewayReconnect`.
    pub fn bind_job(&self, job_id: &str, session_id: &str, backend_url: &str) {
        self.inner
            .job_routes
            .insert(job_id.to_string(), session_id.to_string());
        self.inner
            .backend_inflight
            .entry(backend_url.to_string())
            .or_default()
            .insert(session_id.to_string());
        self.flush_pending_for_backend(backend_url);
    }

    /// Forget a `job_id` once the gateway has observed a terminal event
    /// (caller's responsibility; the subscriber loop does not clean up
    /// automatically because terminal detection needs JSON-RPC
    /// semantics).
    #[allow(dead_code)]
    pub fn forget_job(&self, job_id: &str) {
        self.inner.job_routes.remove(job_id);
    }

    // ── Backend lifecycle ──────────────────────────────────────────────

    /// Ensure a reconnecting SSE subscriber exists for `backend_url`.
    /// Idempotent — a second call is a cheap DashMap lookup.
    pub fn ensure_subscribed(&self, backend_url: &str) {
        if self.inner.backends.contains_key(backend_url) {
            return;
        }
        let shared = Arc::new(BackendShared::new(backend_url.to_string()));
        let mgr = self.clone();
        let url = backend_url.to_string();
        let shared_clone = shared.clone();
        let task = tokio::spawn(async move {
            mgr.run_backend_loop(url, shared_clone).await;
        });
        self.inner.backends.insert(
            backend_url.to_string(),
            BackendSubscriber {
                url: backend_url.to_string(),
                task: Some(task),
                shared,
            },
        );
    }

    // ── Introspection helpers (for tests) ──────────────────────────────

    #[cfg(test)]
    pub(crate) fn route_for_job(&self, job_id: &str) -> Option<String> {
        self.inner.job_routes.get(job_id).map(|e| e.value().clone())
    }

    #[cfg(test)]
    pub(crate) fn route_for_progress_token(&self, token: &Value) -> Option<String> {
        self.inner
            .progress_token_routes
            .get(&progress_token_key(token))
            .map(|e| e.value().clone())
    }

    #[cfg(test)]
    pub(crate) fn pending_count(&self, backend_url: &str) -> usize {
        self.inner
            .backends
            .get(backend_url)
            .map(|b| b.shared.pending.lock().len())
            .unwrap_or(0)
    }

    // ── Delivery ───────────────────────────────────────────────────────

    /// Deliver an MCP notification JSON to the right client session, or
    /// buffer it if we cannot resolve the target yet.
    fn deliver(&self, value: Value, backend_shared: &BackendShared) {
        let session = resolve_target(&self.inner, &value);
        match session {
            Some(sid) => {
                if let Some(sender) = self.inner.client_sinks.get(&sid) {
                    let event = format_sse_event(&value, None);
                    // receiver_count() == 0 is fine: push_event in
                    // SessionManager has the same semantics.
                    let _ = sender.send(event);
                } else {
                    tracing::debug!(
                        session = %sid,
                        backend = %backend_shared.url,
                        "gateway SSE: target session has no live sink — dropping"
                    );
                }
            }
            None => self.buffer_pending(backend_shared, value),
        }
    }

    fn buffer_pending(&self, shared: &BackendShared, value: Value) {
        let mut buf = shared.pending.lock();
        // Expire stale entries first.
        let now = Instant::now();
        while buf
            .front()
            .map(|p| now.duration_since(p.inserted_at) >= PENDING_BUFFER_TTL)
            .unwrap_or(false)
        {
            buf.pop_front();
        }
        if buf.len() >= PENDING_BUFFER_CAP {
            let dropped = buf.pop_front();
            tracing::warn!(
                backend = %shared.url,
                buffered = buf.len() + 1,
                dropped_method = %dropped
                    .as_ref()
                    .and_then(|p| p.value.get("method"))
                    .and_then(|m| m.as_str())
                    .unwrap_or(""),
                "gateway SSE pending buffer full — dropping oldest"
            );
        }
        buf.push_back(Pending {
            inserted_at: now,
            value,
        });
    }

    /// Re-scan the pending buffer after a new routing mapping appeared.
    fn flush_pending_for_backend(&self, backend_url: &str) {
        let Some(backend) = self.inner.backends.get(backend_url) else {
            return;
        };
        let shared = backend.shared.clone();
        drop(backend); // release DashMap shard lock before taking inner lock

        let drained: Vec<Pending> = {
            let mut buf = shared.pending.lock();
            let now = Instant::now();
            buf.retain(|p| now.duration_since(p.inserted_at) < PENDING_BUFFER_TTL);
            std::mem::take(&mut *buf).into_iter().collect()
        };
        for p in drained {
            let session = resolve_target(&self.inner, &p.value);
            match session {
                Some(sid) => {
                    if let Some(sender) = self.inner.client_sinks.get(&sid) {
                        let event = format_sse_event(&p.value, None);
                        let _ = sender.send(event);
                    }
                }
                None => {
                    // Still unresolved — re-queue.
                    shared.pending.lock().push_back(p);
                }
            }
        }
    }

    /// Fan-out a synthetic `$/dcc.gatewayReconnect` notification to every
    /// client that had an in-flight job on `backend_url`.
    fn emit_gateway_reconnect(&self, backend_url: &str) {
        let Some(sessions) = self.inner.backend_inflight.get(backend_url) else {
            return;
        };
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/$/dcc.gatewayReconnect",
            "params": {
                "backend_url": backend_url,
            },
        });
        let event = format_sse_event(&notification, None);
        for sid in sessions.iter() {
            if let Some(sender) = self.inner.client_sinks.get(sid.key()) {
                let _ = sender.send(event.clone());
            }
        }
    }

    // ── Backend reconnect loop ─────────────────────────────────────────

    async fn run_backend_loop(self, url: String, shared: Arc<BackendShared>) {
        let mut attempt: u32 = 0;
        loop {
            match self.open_stream(&url).await {
                Ok(resp) => {
                    if attempt > 0 {
                        tracing::info!(
                            backend = %url,
                            attempts = attempt,
                            "gateway SSE: backend reconnected — emitting gatewayReconnect"
                        );
                        self.emit_gateway_reconnect(&url);
                    }
                    attempt = 0;
                    *shared.reconnect_attempts.lock() = 0;
                    // Pump until the stream closes / errors out.
                    self.pump_stream(resp, &shared).await;
                    tracing::info!(backend = %url, "gateway SSE: stream closed — reconnecting");
                }
                Err(e) => {
                    tracing::debug!(
                        backend = %url,
                        attempt,
                        error = %e,
                        "gateway SSE: connect failed"
                    );
                }
            }
            attempt = attempt.saturating_add(1);
            *shared.reconnect_attempts.lock() = attempt;
            let delay = backoff_delay(attempt);
            tokio::time::sleep(delay).await;
        }
    }

    async fn open_stream(&self, url: &str) -> reqwest::Result<reqwest::Response> {
        self.inner
            .http_client
            .get(url)
            .timeout(CONNECT_TIMEOUT)
            .header("accept", "text/event-stream")
            .send()
            .await
            .and_then(|r| r.error_for_status())
    }

    async fn pump_stream(&self, resp: reqwest::Response, shared: &BackendShared) {
        let mut stream = resp.bytes_stream();
        let mut scratch: Vec<u8> = Vec::with_capacity(4096);
        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    tracing::debug!(backend = %shared.url, error = %e, "gateway SSE: stream error");
                    break;
                }
            };
            scratch.extend_from_slice(&bytes);
            // SSE records terminate with "\n\n"; drain complete records
            // from the head of the scratch buffer.
            while let Some(pos) = find_record_end(&scratch) {
                let record = scratch.drain(..pos).collect::<Vec<u8>>();
                // Discard the trailing delimiter.
                let _ = scratch.drain(..record_delim_len(&scratch));
                if let Some(value) = parse_sse_record(&record) {
                    self.deliver(value, shared);
                }
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Serialise a `progressToken` (may be number or string) into a stable
/// map key.
pub(crate) fn progress_token_key(token: &Value) -> String {
    match token {
        Value::String(s) => format!("s:{s}"),
        Value::Number(n) => format!("n:{n}"),
        other => format!("j:{other}"),
    }
}

/// Exponential backoff with ±25 % jitter.
pub(crate) fn backoff_delay(attempt: u32) -> Duration {
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
fn find_record_end(buf: &[u8]) -> Option<usize> {
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
fn record_delim_len(buf: &[u8]) -> usize {
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
fn parse_sse_record(record: &[u8]) -> Option<Value> {
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

/// Determine which client session should receive `value`.
fn resolve_target(inner: &SubscriberManagerInner, value: &Value) -> Option<String> {
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
            inner.job_routes.get(job_id).map(|e| e.value().clone())
        }
        _ => None,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_delay_starts_near_initial() {
        let d = backoff_delay(1);
        // first attempt — base 100 ms ± 25 %
        assert!(d >= Duration::from_millis(75));
        assert!(d <= Duration::from_millis(125));
    }

    #[test]
    fn backoff_delay_grows_exponentially_and_caps() {
        // large attempt — must not exceed RECONNECT_MAX + 25 %
        let cap_with_jitter = (RECONNECT_MAX.as_millis() as f32 * 1.25) as u128;
        for attempt in 1..30u32 {
            let d = backoff_delay(attempt).as_millis();
            assert!(
                d <= cap_with_jitter,
                "attempt={attempt} delay={d}ms exceeds cap {cap_with_jitter}ms"
            );
        }
        // At attempt=20 we are definitely saturated near the cap.
        let d = backoff_delay(20).as_millis();
        let floor = (RECONNECT_MAX.as_millis() as f32 * 0.75) as u128;
        assert!(d >= floor, "saturated backoff={d}ms below floor {floor}ms");
    }

    #[test]
    fn progress_token_key_distinguishes_string_and_number_tokens() {
        let s = progress_token_key(&Value::String("abc".into()));
        let n = progress_token_key(&serde_json::json!(42));
        let n_str = progress_token_key(&Value::String("42".into()));
        assert_ne!(s, n);
        assert_ne!(n, n_str);
    }

    #[test]
    fn parse_sse_record_extracts_json_from_data_field() {
        let rec = b"data: {\"method\":\"notifications/progress\",\"params\":{\"progress\":1}}";
        let v = parse_sse_record(rec).expect("valid record");
        assert_eq!(v["method"], "notifications/progress");
    }

    #[test]
    fn parse_sse_record_handles_multiline_data_and_id_field() {
        // Two `data:` lines must be concatenated with '\n' per
        // WHATWG SSE spec. We check both that the parse does not
        // panic on a multi-line record and that non-data lines
        // (`id:`, `event:`) are skipped.
        let rec = b"id: 7\nevent: message\ndata: {\"a\":1,\ndata: \"b\":2}";
        let v = parse_sse_record(rec).expect("multi-line data: joins cleanly");
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], 2);
    }

    #[test]
    fn parse_sse_record_returns_none_for_record_without_data_field() {
        let rec = b"event: endpoint\n";
        assert!(parse_sse_record(rec).is_none());
    }

    #[test]
    fn resolve_target_prefers_progress_token_for_progress_notifications() {
        let inner = SubscriberManagerInner {
            backends: DashMap::new(),
            job_routes: DashMap::new(),
            progress_token_routes: DashMap::new(),
            backend_inflight: DashMap::new(),
            client_sinks: DashMap::new(),
            http_client: reqwest::Client::new(),
        };
        inner.progress_token_routes.insert(
            progress_token_key(&Value::String("tok".into())),
            "sessA".into(),
        );
        let note = serde_json::json!({
            "method": "notifications/progress",
            "params": {"progressToken": "tok", "progress": 5, "total": 10}
        });
        assert_eq!(resolve_target(&inner, &note).as_deref(), Some("sessA"));
    }

    #[test]
    fn resolve_target_uses_job_id_for_job_updated() {
        let inner = SubscriberManagerInner {
            backends: DashMap::new(),
            job_routes: DashMap::new(),
            progress_token_routes: DashMap::new(),
            backend_inflight: DashMap::new(),
            client_sinks: DashMap::new(),
            http_client: reqwest::Client::new(),
        };
        inner.job_routes.insert("jid-42".into(), "sessB".into());
        let note = serde_json::json!({
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "jid-42", "status": "running"}
        });
        assert_eq!(resolve_target(&inner, &note).as_deref(), Some("sessB"));
    }

    #[test]
    fn resolve_target_returns_none_when_unknown() {
        let inner = SubscriberManagerInner {
            backends: DashMap::new(),
            job_routes: DashMap::new(),
            progress_token_routes: DashMap::new(),
            backend_inflight: DashMap::new(),
            client_sinks: DashMap::new(),
            http_client: reqwest::Client::new(),
        };
        let note = serde_json::json!({
            "method": "notifications/progress",
            "params": {"progressToken": "no-such-token"}
        });
        assert!(resolve_target(&inner, &note).is_none());
    }

    #[tokio::test]
    async fn manager_buffers_then_flushes_after_job_binding() {
        // Stand up a manager, register a client, hand-feed a notification
        // whose job_id mapping is not yet known, then bind the mapping
        // and assert the buffered event is delivered.
        let mgr = SubscriberManager::default();
        let mut rx = mgr.register_client("sess1");
        let backend = "http://127.0.0.1:0/mcp".to_string();
        // Fake a backend entry so buffer operations resolve.
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.inner.backends.insert(
            backend.clone(),
            BackendSubscriber {
                url: backend.clone(),
                task: None,
                shared: shared.clone(),
            },
        );

        let note = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/$/dcc.jobUpdated",
            "params": {"job_id": "job-1", "status": "running"}
        });
        mgr.deliver(note.clone(), &shared);
        assert_eq!(mgr.pending_count(&backend), 1, "buffered while unresolved");
        assert!(rx.try_recv().is_err(), "nothing delivered yet");

        mgr.bind_job("job-1", "sess1", &backend);
        // After bind, the flush is triggered synchronously.
        assert_eq!(mgr.pending_count(&backend), 0, "buffer drained");
        let event = rx
            .try_recv()
            .expect("event should have been flushed to sink");
        assert!(event.contains("notifications/$/dcc.jobUpdated"));
    }

    #[tokio::test]
    async fn manager_emits_gateway_reconnect_to_inflight_sessions() {
        let mgr = SubscriberManager::default();
        let mut rx = mgr.register_client("sess1");
        let backend = "http://127.0.0.1:0/mcp".to_string();
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.inner.backends.insert(
            backend.clone(),
            BackendSubscriber {
                url: backend.clone(),
                task: None,
                shared,
            },
        );
        mgr.bind_job("job-x", "sess1", &backend);

        mgr.emit_gateway_reconnect(&backend);

        let event = rx.try_recv().expect("gatewayReconnect should be delivered");
        assert!(event.contains("notifications/$/dcc.gatewayReconnect"));
        assert!(event.contains(&backend));
    }

    #[tokio::test]
    async fn manager_drops_events_for_forgotten_client() {
        let mgr = SubscriberManager::default();
        let _rx = mgr.register_client("sess1");
        mgr.forget_client("sess1");

        let backend = "http://127.0.0.1:0/mcp".to_string();
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.inner.backends.insert(
            backend.clone(),
            BackendSubscriber {
                url: backend.clone(),
                task: None,
                shared: shared.clone(),
            },
        );
        mgr.bind_job("job-gone", "sess1", &backend);
        let note = serde_json::json!({
            "jsonrpc":"2.0",
            "method":"notifications/$/dcc.jobUpdated",
            "params":{"job_id":"job-gone","status":"running"}
        });
        // Must not panic; simply drops.
        mgr.deliver(note, &shared);
    }

    #[test]
    fn pending_buffer_evicts_oldest_when_full() {
        let mgr = SubscriberManager::default();
        let backend = "http://127.0.0.1:0/mcp".to_string();
        let shared = Arc::new(BackendShared::new(backend.clone()));
        mgr.inner.backends.insert(
            backend.clone(),
            BackendSubscriber {
                url: backend.clone(),
                task: None,
                shared: shared.clone(),
            },
        );
        for i in 0..(PENDING_BUFFER_CAP + 5) {
            let note = serde_json::json!({
                "method":"notifications/$/dcc.jobUpdated",
                "params":{"job_id": format!("j{i}"), "status":"running"}
            });
            mgr.deliver(note, &shared);
        }
        assert_eq!(
            mgr.pending_count(&backend),
            PENDING_BUFFER_CAP,
            "buffer is bounded"
        );
    }
}
