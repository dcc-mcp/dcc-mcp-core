//! Admin event utilities and SSE streaming endpoint.
//!
//! - `contend_event_to_admin_row`: converts contention events to admin log rows.
//! - `handle_admin_events`: `GET /admin/api/events` SSE stream for real-time updates.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use serde_json::{Value, json};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use super::state::AdminState;
use crate::gateway::event_log::{ContendEvent, EventKind};

pub(crate) fn contend_event_to_admin_row(e: ContendEvent) -> Value {
    if matches!(e.event, EventKind::OperatorNote) {
        let message = e
            .reason
            .clone()
            .unwrap_or_else(|| "operator note".to_string());
        return json!({
            "timestamp": e.timestamp,
            "level": "info",
            "message": message,
            "source": "admin",
            "event": e.event,
            "dcc_type": e.dcc_type,
            "instance_id": e.instance_id,
            "reason": e.reason,
        });
    }
    let label = e.event.as_label();
    let mut message = format!("{label} dcc_type={} instance={}", e.dcc_type, e.instance_id);
    if let Some(r) = &e.reason {
        message.push_str(" - ");
        message.push_str(r);
    }
    json!({
        "timestamp": e.timestamp,
        "level": "info",
        "message": message,
        "source": "contention",
        "event": e.event,
        "dcc_type": e.dcc_type,
        "instance_id": e.instance_id,
        "reason": e.reason,
    })
}

/// SSE event wrapper sent to admin UI clients.
#[derive(Debug, Clone, serde::Serialize)]
struct AdminSseEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    data: Value,
}

/// `GET /admin/api/events` — Server-Sent Events endpoint for real-time admin updates.
///
/// Streams gateway events (contention, health changes, session updates) to the
/// admin UI. Includes a keep-alive heartbeat every 15 seconds.
///
/// The client can supply a `Last-Event-ID` header to request replay of missed
/// events from SQLite (not yet implemented — currently starts from live only).
pub async fn handle_admin_events(
    State(s): State<AdminState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = s.gateway.events_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        let json_str = match result {
            Ok(s) => s,
            Err(_lagged_or_closed) => {
                // BroadcastStream wraps RecvError; handle both Lagged and Closed gracefully.
                // We emit a lag event for any error — the client can reconnect if needed.
                return Some(Ok(Event::default()
                    .event("lag")
                    .data(json!({"note": "event stream interrupted"}).to_string())));
            }
        };

        // Try to parse the broadcast JSON to extract an event type.
        // The gateway broadcasts raw JSON strings; we wrap them into typed SSE events.
        let parsed: Value = match serde_json::from_str(&json_str) {
            Ok(v) => v,
            Err(_) => {
                return Some(Ok(Event::default().event("message").data(json_str)));
            }
        };

        let event_type = parsed
            .get("event")
            .and_then(|v| v.as_str())
            .unwrap_or("message")
            .to_string();

        let id = parsed
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let data = serde_json::to_string(&AdminSseEvent {
            event_type: event_type.clone(),
            id: id.clone(),
            data: parsed,
        })
        .unwrap_or(json_str);

        Some(Ok(Event::default()
            .event(event_type)
            .id(id.unwrap_or_default())
            .data(data)))
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
