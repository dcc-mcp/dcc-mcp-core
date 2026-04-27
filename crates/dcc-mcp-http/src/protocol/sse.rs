//! SSE event formatter + cursor pagination helpers.

use serde::Serialize;

/// Format a JSON-RPC message as an SSE event string.
pub fn format_sse_event(data: &impl Serialize, event_id: Option<&str>) -> String {
    let json = serde_json::to_string(data).unwrap_or_default();
    if let Some(id) = event_id {
        format!("id: {id}\ndata: {json}\n\n")
    } else {
        format!("data: {json}\n\n")
    }
}

/// Encode a page offset as an opaque cursor string.
pub fn encode_cursor(offset: usize) -> String {
    format!("{offset}")
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Decode a cursor produced by [`encode_cursor`]. Returns `None` if malformed.
pub fn decode_cursor(cursor: &str) -> Option<usize> {
    if cursor.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..cursor.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cursor[i..i + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()?.parse().ok()
}
