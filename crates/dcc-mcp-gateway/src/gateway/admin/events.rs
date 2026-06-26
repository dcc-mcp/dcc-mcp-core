use serde_json::{Value, json};

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
