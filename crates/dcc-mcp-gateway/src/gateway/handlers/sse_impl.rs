use super::*;

/// RAII guard that evicts a client session's subscriber sink when the
/// gateway SSE response is dropped (client disconnect).
struct SessionCleanup {
    mgr: super::super::sse_subscriber::SubscriberManager,
    session_id: String,
}

impl Drop for SessionCleanup {
    fn drop(&mut self) {
        self.mgr.forget_client(&self.session_id);
    }
}

/// Stream adapter that holds a [`SessionCleanup`] alive for the duration
/// of the response body.
struct GuardedStream {
    inner: Pin<Box<dyn futures::Stream<Item = Result<Event, Infallible>> + Send>>,
    _guard: SessionCleanup,
}

impl GuardedStream {
    fn new<S>(inner: S, guard: SessionCleanup) -> Self
    where
        S: futures::Stream<Item = Result<Event, Infallible>> + Send + 'static,
    {
        Self {
            inner: Box::pin(inner),
            _guard: guard,
        }
    }
}

impl futures::Stream for GuardedStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// `GET /mcp` — server-sent event stream for MCP push notifications.
pub async fn handle_gateway_get(State(gs): State<GatewayState>, headers: HeaderMap) -> Response {
    let accepts_sse = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|value| value.contains("text/event-stream"))
        .unwrap_or(false);

    if !accepts_sse {
        return (
            StatusCode::NOT_ACCEPTABLE,
            Json(json!({
                "error": "This endpoint streams SSE. Set Accept: text/event-stream",
                "auth_required": false,
                "hint": "Use POST /mcp with Accept: application/json, text/event-stream for JSON-RPC requests."
            })),
        )
            .into_response();
    }

    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("gw-{}", uuid::Uuid::new_v4().simple()));

    let broadcast_stream = make_broadcast_stream(gs.events_tx.subscribe());
    let per_session_stream = make_session_stream(gs.subscriber.register_client(&session_id));
    let cleanup = SessionCleanup {
        mgr: gs.subscriber.clone(),
        session_id: session_id.clone(),
    };
    let endpoint_event = stream::once(async {
        Ok::<Event, Infallible>(Event::default().event("endpoint").data("/mcp"))
    });
    let initial_tools_changed = stream::once(async {
        Ok::<Event, Infallible>(
            Event::default().data(
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/tools/list_changed",
                    "params": {}
                })
                .to_string(),
            ),
        )
    });
    let combined = endpoint_event
        .chain(initial_tools_changed)
        .chain(broadcast_stream.merge(per_session_stream));
    let guarded = GuardedStream::new(combined, cleanup);

    let mut response = Sse::new(guarded)
        .keep_alive(KeepAlive::default())
        .into_response();
    if let Ok(header_value) = session_id.parse() {
        response
            .headers_mut()
            .insert("Mcp-Session-Id", header_value);
    }
    response
}

fn make_broadcast_stream(
    rx: tokio::sync::broadcast::Receiver<String>,
) -> impl futures::Stream<Item = Result<Event, Infallible>> + Send {
    BroadcastStream::new(rx).filter_map(|result| {
        let data = match result {
            Ok(value) => value,
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(skipped)) => {
                tracing::warn!("Gateway SSE: client lagged, skipped {skipped} message(s)");
                return None;
            }
        };
        Some(Ok::<Event, Infallible>(Event::default().data(data)))
    })
}

fn make_session_stream(
    rx: tokio::sync::broadcast::Receiver<String>,
) -> impl futures::Stream<Item = Result<Event, Infallible>> + Send {
    BroadcastStream::new(rx).filter_map(|result| {
        let data = match result {
            Ok(value) => value,
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(skipped)) => {
                tracing::warn!(
                    "Gateway SSE (per-session): client lagged, skipped {skipped} message(s)"
                );
                return None;
            }
        };
        let payload = data
            .strip_prefix("data: ")
            .and_then(|value| value.strip_suffix("\n\n"))
            .unwrap_or(&data)
            .to_owned();
        Some(Ok::<Event, Infallible>(Event::default().data(payload)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Request, header};
    use axum::routing::get;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{RwLock, broadcast, watch};
    use tower::ServiceExt;

    fn make_gateway_state() -> GatewayState {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(
            dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
        ));
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        GatewayState {
            registry,
            http_instance_registry: Arc::new(parking_lot::RwLock::new(
                crate::gateway::http_registration::HttpInstanceRegistry::default(),
            )),

            mdns_instance_registry: Arc::new(parking_lot::RwLock::new(
                crate::gateway::mdns_registration::MdnsInstanceRegistry::default(),
            )),
            stale_timeout: Duration::from_secs(30),
            backend_timeout: Duration::from_secs(10),
            async_dispatch_timeout: Duration::from_secs(60),
            wait_terminal_timeout: Duration::from_secs(600),
            server_name: "test-gateway".into(),
            server_version: "0.0.0-test".into(),
            own_host: "127.0.0.1".into(),
            own_port: 9765,
            http_client: reqwest::Client::new(),
            yield_tx: Arc::new(yield_tx),
            events_tx: Arc::new(events_tx),
            protocol_version: Arc::new(RwLock::new(None)),
            resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            client_attribution: Arc::new(
                crate::gateway::caller_attribution::ClientAttributionStore::default(),
            ),
            pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
            subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
            allow_unknown_tools: false,
            policy: Arc::new(crate::gateway::GatewayPolicy::default()),
            adapter_version: None,
            adapter_dcc: None,
            capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
            event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
            #[cfg(feature = "prometheus")]
            gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
            middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
            instance_diagnostics: Arc::new(
                crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
            ),
            traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
            search_telemetry: Arc::new(
                crate::gateway::search_telemetry::SearchTelemetryStore::new(),
            ),
            debug_routes_enabled: false,
        }
    }

    #[tokio::test]
    async fn get_mcp_without_sse_accept_is_protocol_hint_not_auth_challenge() {
        let app = axum::Router::new()
            .route("/mcp", get(handle_gateway_get))
            .with_state(make_gateway_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mcp")
                    .header(header::ACCEPT, "application/json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
        assert!(
            response.headers().get(header::WWW_AUTHENTICATE).is_none(),
            "Accept negotiation failures must not trigger browser/client login flows"
        );
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["auth_required"], false);
        assert!(
            body["hint"].as_str().unwrap().contains("POST /mcp"),
            "expected actionable POST /mcp hint, got {body}"
        );
    }
}
