use super::backend::BackendShared;
use super::helpers::{backoff_delay, find_record_end, parse_sse_record, record_delim_len};
use super::*;

impl SubscriberManager {
    // ── Backend reconnect loop ─────────────────────────────────────────

    pub(super) async fn run_backend_loop(self, url: String, shared: Arc<BackendShared>) {
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

    pub(super) async fn open_stream(&self, url: &str) -> reqwest::Result<reqwest::Response> {
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
        self.inner
            .http_client
            .get(url)
            .header("accept", "text/event-stream")
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
