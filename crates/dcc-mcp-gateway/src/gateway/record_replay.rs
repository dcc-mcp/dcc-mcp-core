//! Gateway-owned, caller-scoped demonstration capture over redacted traffic frames.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_actions::events::EventEnvelope;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Maximum tool calls retained by one recording.
pub const MAX_RECORDING_EVENTS: usize = 500;

/// Request to begin an explicit caller-scoped demonstration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRecording {
    /// Server-derived task/session id whose calls may be captured.
    pub session_id: String,
    /// DCC family expected during the demonstration.
    pub dcc_type: String,
    /// Optional demonstrated instance for review-only correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

/// One safely retained gateway call before schema resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapturedToolCall {
    /// Monotonic event sequence.
    pub sequence: u64,
    /// Gateway slug used during demonstration; compilation resolves the current stable tool.
    pub tool_slug: String,
    /// Post-redaction arguments only.
    pub arguments: Value,
    /// Correlated request id when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Demonstrated result once the matching outbound frame arrives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
}

/// Review projection of an active or stopped demonstration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingDraft {
    /// Opaque recording id.
    pub recording_id: String,
    /// Trusted caller namespace.
    pub session_id: String,
    /// DCC family selected at start.
    pub dcc_type: String,
    /// Review-only demonstrated instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// `recording`, `stopped`, `interrupted`, or `truncated`.
    pub status: String,
    /// Start timestamp.
    pub started_at_ms: u64,
    /// Stop timestamp when finalized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_at_ms: Option<u64>,
    /// Ordered retained calls.
    pub events: Vec<CapturedToolCall>,
}

/// Recording lifecycle error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordingStoreError {
    /// Required id is empty or oversized.
    #[error("invalid {0}")]
    InvalidField(&'static str),
    /// This trusted caller already has an active recording.
    #[error("session already has an active recording")]
    AlreadyRecording,
    /// Recording does not exist or belongs to another caller.
    #[error("recording not found for this session")]
    NotFound,
}

#[derive(Default)]
struct StoreState {
    active_by_session: HashMap<String, String>,
    recordings: HashMap<String, RecordingDraft>,
}

/// Bounded in-memory recording registry fed only post-redaction frames.
#[derive(Clone, Default)]
pub struct RecordReplayStore {
    inner: Arc<Mutex<StoreState>>,
}

impl RecordReplayStore {
    /// Number of currently active caller-scoped recordings.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.inner.lock().active_by_session.len()
    }

    /// Begin one recording. Recording consent does not imply replay consent.
    pub fn start(&self, request: StartRecording) -> Result<RecordingDraft, RecordingStoreError> {
        validate_label(&request.session_id, "session_id")?;
        validate_label(&request.dcc_type, "dcc_type")?;
        let mut state = self.inner.lock();
        if state.active_by_session.contains_key(&request.session_id) {
            return Err(RecordingStoreError::AlreadyRecording);
        }
        let recording_id = Uuid::new_v4().to_string();
        let draft = RecordingDraft {
            recording_id: recording_id.clone(),
            session_id: request.session_id.clone(),
            dcc_type: request.dcc_type,
            instance_id: request.instance_id,
            status: "recording".to_owned(),
            started_at_ms: now_ms(),
            stopped_at_ms: None,
            events: Vec::new(),
        };
        state
            .active_by_session
            .insert(request.session_id, recording_id.clone());
        state.recordings.insert(recording_id, draft.clone());
        Ok(draft)
    }

    /// Stop and return a recording owned by the trusted caller.
    pub fn stop(
        &self,
        session_id: &str,
        recording_id: &str,
    ) -> Result<RecordingDraft, RecordingStoreError> {
        let mut state = self.inner.lock();
        if state.active_by_session.get(session_id).map(String::as_str) != Some(recording_id) {
            return Err(RecordingStoreError::NotFound);
        }
        state.active_by_session.remove(session_id);
        let draft = state
            .recordings
            .get_mut(recording_id)
            .ok_or(RecordingStoreError::NotFound)?;
        draft.status = if draft.events.len() >= MAX_RECORDING_EVENTS {
            "truncated"
        } else {
            "stopped"
        }
        .to_owned();
        draft.stopped_at_ms = Some(now_ms());
        Ok(draft.clone())
    }

    /// Read one recording only through its owning trusted caller namespace.
    pub fn get(&self, session_id: &str, recording_id: &str) -> Option<RecordingDraft> {
        self.inner
            .lock()
            .recordings
            .get(recording_id)
            .filter(|draft| draft.session_id == session_id)
            .cloned()
    }

    /// Consume one post-redaction traffic frame when its trusted session is recording.
    pub fn capture_frame(&self, envelope: &EventEnvelope) -> Option<Value> {
        let session_id = envelope
            .attributes
            .get("session_id")
            .and_then(Value::as_str)
            .or_else(|| {
                envelope
                    .correlation
                    .get("session_id")
                    .and_then(Value::as_str)
            })?;
        let mut state = self.inner.lock();
        let recording_id = state.active_by_session.get(session_id).cloned()?;
        let draft = state.recordings.get_mut(&recording_id)?;
        if let Some((request_id, success)) = captured_outcome(envelope) {
            let updated = draft
                .events
                .iter_mut()
                .rev()
                .find(|event| event.request_id.as_deref() == Some(request_id))
                .map(|event| {
                    event.success = Some(success);
                    event.clone()
                });
            if let Some(event) = updated {
                return Some(recording_session_event(
                    "recording.tool_call",
                    draft,
                    Some(serde_json::json!(event)),
                ));
            }
            return None;
        }
        let (tool_slug, arguments) = captured_call(envelope)?;
        if draft.events.len() >= MAX_RECORDING_EVENTS {
            draft.status = "truncated".to_owned();
            return None;
        }
        let event = CapturedToolCall {
            sequence: draft.events.len() as u64 + 1,
            tool_slug,
            arguments,
            request_id: envelope
                .correlation
                .get("request_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            success: None,
        };
        draft.events.push(event.clone());
        Some(recording_session_event(
            "recording.tool_call",
            draft,
            Some(serde_json::json!(event)),
        ))
    }

    /// Restore a stopped or interrupted draft without reopening capture authority.
    pub fn restore(&self, draft: RecordingDraft) {
        self.inner
            .lock()
            .recordings
            .insert(draft.recording_id.clone(), draft);
    }
}

/// Rebuild one recording from its ordered session-event projection.
pub(crate) fn recover_recording(
    recording_id: &str,
    rows: &[Value],
) -> Option<(RecordingDraft, bool)> {
    let mut draft = None;
    let mut calls = BTreeMap::new();
    let mut terminal = false;

    for row in rows
        .iter()
        .filter(|row| row.get("recording_id").and_then(Value::as_str) == Some(recording_id))
    {
        match row.get("event_type").and_then(Value::as_str) {
            Some("recording.started") => {
                let created_at_ms = row
                    .get("created_at_ms")
                    .and_then(Value::as_i64)
                    .and_then(|value| u64::try_from(value).ok())
                    .unwrap_or_else(now_ms);
                draft = Some(RecordingDraft {
                    recording_id: recording_id.to_owned(),
                    session_id: row.get("session_id")?.as_str()?.to_owned(),
                    dcc_type: row.get("dcc_type")?.as_str()?.to_owned(),
                    instance_id: row
                        .get("instance_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    status: "recording".to_owned(),
                    started_at_ms: row
                        .get("started_at_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(created_at_ms),
                    stopped_at_ms: None,
                    events: Vec::new(),
                });
            }
            Some("recording.tool_call") => {
                if let Some(payload) = row.get("payload")
                    && let Ok(event) = serde_json::from_value::<CapturedToolCall>(payload.clone())
                {
                    calls.insert(event.sequence, event);
                }
            }
            Some("recording.stopped") | Some("recording.interrupted") => {
                let recovered = draft.as_mut()?;
                recovered.status = row
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("interrupted")
                    .to_owned();
                recovered.stopped_at_ms =
                    row.get("stopped_at_ms")
                        .and_then(Value::as_u64)
                        .or_else(|| {
                            row.get("created_at_ms")
                                .and_then(Value::as_i64)
                                .and_then(|value| u64::try_from(value).ok())
                        });
                terminal = true;
            }
            _ => {}
        }
    }

    let mut draft = draft?;
    draft.events = calls.into_values().collect();
    if !terminal {
        draft.status = "interrupted".to_owned();
        draft.stopped_at_ms = Some(now_ms());
    }
    Some((draft, !terminal))
}

pub(crate) fn recording_session_event(
    event_type: &str,
    draft: &RecordingDraft,
    payload: Option<Value>,
) -> Value {
    let created_at_ms = now_ms();
    serde_json::json!({
        "session_id": draft.session_id,
        "event_type": event_type,
        "created_at_ms": created_at_ms.min(i64::MAX as u64) as i64,
        "recording_id": draft.recording_id,
        "dcc_type": draft.dcc_type,
        "instance_id": draft.instance_id,
        "status": draft.status,
        "started_at_ms": draft.started_at_ms,
        "stopped_at_ms": draft.stopped_at_ms,
        "payload": payload,
    })
}

fn validate_label(value: &str, name: &'static str) -> Result<(), RecordingStoreError> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(RecordingStoreError::InvalidField(name));
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn captured_call(envelope: &EventEnvelope) -> Option<(String, Value)> {
    if envelope.attributes.get("direction").and_then(Value::as_str) != Some("inbound") {
        return None;
    }
    let body = envelope.attributes.pointer("/body/data")?;
    let payload = body.get("params").unwrap_or(body);
    let arguments = payload.get("arguments")?;
    let nested = if payload.get("name").and_then(Value::as_str) == Some("call") {
        arguments
    } else {
        payload
    };
    let tool_slug = nested.get("tool_slug")?.as_str()?.to_owned();
    let arguments = nested
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    Some((tool_slug, arguments))
}

fn captured_outcome(envelope: &EventEnvelope) -> Option<(&str, bool)> {
    if envelope.attributes.get("direction").and_then(Value::as_str) != Some("outbound") {
        return None;
    }
    let request_id = envelope
        .correlation
        .get("request_id")
        .and_then(Value::as_str)?;
    let body = envelope.attributes.pointer("/body/data")?;
    let failed = body.get("error").is_some()
        || body.get("isError").and_then(Value::as_bool) == Some(true)
        || body.get("success").and_then(Value::as_bool) == Some(false)
        || body.pointer("/result/isError").and_then(Value::as_bool) == Some(true);
    Some((request_id, !failed))
}

#[cfg(test)]
mod tests {
    use dcc_mcp_actions::events::EventEnvelope;
    use serde_json::json;

    use super::*;

    fn frame(session_id: &str, token: &str) -> EventEnvelope {
        EventEnvelope::new(
            "traffic.frame",
            "frame-1",
            json!({}),
            json!({"request_id": "req-1", "session_id": session_id}),
            json!({
                "session_id": session_id,
                "direction": "inbound",
                "body": {"data": {
                    "tool_slug": "maya.abcd.scene__build",
                    "arguments": {"output": "demo", "api_token": token}
                }}
            }),
        )
    }

    fn outcome(session_id: &str, success: bool) -> EventEnvelope {
        EventEnvelope::new(
            "traffic.frame",
            "frame-2",
            json!({}),
            json!({"request_id": "req-1", "session_id": session_id}),
            json!({
                "session_id": session_id,
                "direction": "outbound",
                "body": {"data": {"result": {"isError": !success}}}
            }),
        )
    }

    #[test]
    fn isolates_same_logical_activity_by_trusted_session() {
        let store = RecordReplayStore::default();
        let first = store
            .start(StartRecording {
                session_id: "caller-a".to_owned(),
                dcc_type: "maya".to_owned(),
                instance_id: None,
            })
            .unwrap();
        let second = store
            .start(StartRecording {
                session_id: "caller-b".to_owned(),
                dcc_type: "houdini".to_owned(),
                instance_id: None,
            })
            .unwrap();

        store.capture_frame(&frame("caller-a", "[REDACTED]"));
        store.capture_frame(&outcome("caller-a", true));

        let recorded = store.get("caller-a", &first.recording_id).unwrap();
        assert_eq!(recorded.events.len(), 1);
        assert_eq!(recorded.events[0].success, Some(true));
        assert!(store.get("caller-b", &first.recording_id).is_none());
        assert!(store.get("caller-a", &second.recording_id).is_none());
        assert!(
            store
                .get("caller-b", &second.recording_id)
                .unwrap()
                .events
                .is_empty()
        );
    }
}
