use super::backend::BackendShared;
use super::helpers::{backoff_delay, find_record_end, parse_sse_record, record_delim_len};
use super::*;

impl SubscriberManager {
    // ── Backend reconnect loop ─────────────────────────────────────────

    pub(super) async fn run_backend_loop(self, url: String, shared: Arc<BackendShared>) {
        let mut attempt: u32 = 0;
        loop {
            // #732: the backend fans `notifications/resources/updated`
            // per-session, so the gateway must hold a *real* session id
            // that exists in the backend's `SessionManager`. Only an
            // `initialize` RPC creates a session; a bare GET /mcp with
            // a made-up id would 404. Do the handshake, then open the
            // SSE stream with whatever id the backend minted.
            let session_id = match self.handshake_session_id(&url).await {
                Ok(id) => id,
                Err(e) => {
                    tracing::debug!(
                        backend = %url,
                        attempt,
                        error = %e,
                        "gateway SSE: initialize handshake failed — will retry"
                    );
                    attempt = attempt.saturating_add(1);
                    *shared.reconnect_attempts.lock() = attempt;
                    tokio::time::sleep(backoff_delay(attempt)).await;
                    continue;
                }
            };
            *shared.session_id.lock() = Some(session_id.clone());

            match self.open_stream(&url, &session_id).await {
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

    /// Perform the `initialize` handshake against a backend and return
    /// the session id the backend minted. This id is the contract the
    /// per-session fan-out in `background_impl::spawn_notifications_task`
    /// uses, so we must use it for both the SSE GET and any forwarded
    /// `resources/subscribe` on behalf of clients (#732).
    pub(super) async fn handshake_session_id(&self, url: &str) -> Result<String, String> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "gw-sub-init",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {
                    "name": "dcc-mcp-gateway-subscriber",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }
        });
        // Explicit short timeout for the handshake — unlike the SSE
        // stream, this is a bounded request/response round-trip and
        // must not hang indefinitely when the backend is slow.
        let resp = self
            .inner
            .http_client
            .post(url)
            .timeout(std::time::Duration::from_secs(5))
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| format!("transport: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("status {}", resp.status()));
        }
        // Prefer the `Mcp-Session-Id` response header — that's the
        // canonical carrier. Fall back to the `__session_id` field some
        // older code paths splice into the `result` object.
        if let Some(header) = resp.headers().get("mcp-session-id")
            && let Ok(s) = header.to_str()
            && !s.is_empty()
        {
            let owned = s.to_owned();
            // Drain the body so the connection is reusable.
            let _ = resp.bytes().await;
            return Ok(owned);
        }
        let text = resp.text().await.map_err(|e| format!("read body: {e}"))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("parse body: {e}"))?;
        value
            .get("result")
            .and_then(|r| r.get("__session_id"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| "initialize response lacked session id".to_string())
    }

    pub(super) async fn open_stream(
        &self,
        url: &str,
        session_id: &str,
    ) -> reqwest::Result<reqwest::Response> {
        // NOTE: Intentionally do NOT call `.timeout(..)` here.
        //
        // `RequestBuilder::timeout()` in reqwest 0.13 applies to the *entire*
        // request — including the streaming response body — so for an SSE
        // subscription it would abort the long-lived stream as soon as the
        // timeout elapsed, producing a recurring "error decoding response
        // body" every few seconds (gateway SSE reconnect storm, visible in
        // the logs as back-to-back `gatewayReconnect` events).
        //
        // The idle/heartbeat timeout for the established stream is enforced
        // by `pump_stream` via `tokio::time::timeout` around each chunk
        // read (see [`STREAM_IDLE_TIMEOUT`]), so the connect phase here
        // only needs whatever default the shared `reqwest::Client` was
        // built with.
        //
        // #732: the `Mcp-Session-Id` header binds this SSE stream to a
        // stable backend session id. Any `resources/subscribe` the gateway
        // forwards on behalf of a client is sent with the SAME header so
        // the backend's per-session `notifications/resources/updated`
        // fan-out lands on this stream (and nowhere else).
        self.inner
            .http_client
            .get(url)
            .header("accept", "text/event-stream")
            .header("Mcp-Session-Id", session_id)
            .send()
            .await
            .and_then(|r| r.error_for_status())
    }

    pub(super) async fn pump_stream(&self, resp: reqwest::Response, shared: &BackendShared) {
        let mut stream = resp.bytes_stream();
        let mut scratch: Vec<u8> = Vec::with_capacity(4096);
        loop {
            // Apply an idle/read timeout *per chunk* rather than to the
            // whole request — this keeps the long-lived SSE stream alive
            // as long as the backend emits heartbeats within the window,
            // while still failing fast if the backend stalls.
            let chunk = match tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()).await {
                Ok(Some(item)) => item,
                // Stream terminated cleanly by the server.
                Ok(None) => break,
                Err(_) => {
                    tracing::debug!(
                        backend = %shared.url,
                        idle_secs = STREAM_IDLE_TIMEOUT.as_secs(),
                        "gateway SSE: read idle timeout — reconnecting"
                    );
                    break;
                }
            };
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
