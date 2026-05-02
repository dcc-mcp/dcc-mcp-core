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
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({"error": "This endpoint streams SSE. Set Accept: text/event-stream"})),
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
