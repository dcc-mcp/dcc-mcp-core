//! Framework-enforced payload size limits and SSE chunking (issue #771).
//!
//! ## Truncation envelope
//!
//! When a resource, prompt, or tool-call response exceeds
//! [`McpHttpConfig::max_response_content_bytes`](crate::config::McpHttpConfig::max_response_content_bytes),
//! the server wraps the truncated content in a [`TruncationEnvelope`]:
//!
//! ```json
//! {
//!   "content": "...(truncated)...",
//!   "truncated": true,
//!   "original_size": 1048576,
//!   "truncated_size": 65536
//! }
//! ```
//!
//! ## SSE chunking
//!
//! Large SSE event data strings are split into sequential `chunk` /
//! `chunk_end` events so that a single oversized event cannot stall a
//! client's read loop.  Each fragment event looks like:
//!
//! ```text
//! event: chunk
//! data: {"seq":0,"total":3,"data":"...base64..."}
//!
//! event: chunk
//! data: {"seq":1,"total":3,"data":"...base64..."}
//!
//! event: chunk_end
//! data: {"seq":2,"total":3,"data":"...base64..."}
//! ```

use serde::{Deserialize, Serialize};

// ── TruncationEnvelope ────────────────────────────────────────────────────────

/// Wrapper returned when a response payload exceeds
/// [`McpHttpConfig::max_response_content_bytes`](crate::config::McpHttpConfig::max_response_content_bytes).
///
/// The `content` field contains the UTF-8 prefix of the original payload
/// truncated to fit within the configured limit. Callers should check the
/// `truncated` flag and surface the `original_size` / `truncated_size`
/// metadata to the AI agent so it can decide whether to request a smaller
/// slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TruncationEnvelope {
    /// The (possibly truncated) content string.
    pub content: String,
    /// `true` if the content was truncated.
    pub truncated: bool,
    /// Original byte length of the full content before truncation.
    pub original_size: usize,
    /// Byte length of the `content` field as returned.
    pub truncated_size: usize,
}

impl TruncationEnvelope {
    /// Wrap `content`, truncating at a UTF-8 character boundary if
    /// `content.len() > max_bytes`. When `max_bytes == 0` or the content
    /// already fits, returns an un-truncated envelope (with `truncated: false`).
    pub fn new(content: impl Into<String>, max_bytes: usize) -> Self {
        let content = content.into();
        let original_size = content.len();
        if max_bytes == 0 || original_size <= max_bytes {
            return Self {
                truncated_size: original_size,
                content,
                truncated: false,
                original_size,
            };
        }
        // Truncate at a char boundary so the resulting string is valid UTF-8.
        let truncated_content = truncate_at_char_boundary(&content, max_bytes);
        let truncated_size = truncated_content.len();
        Self {
            content: truncated_content,
            truncated: true,
            original_size,
            truncated_size,
        }
    }
}

/// Truncate `s` to at most `max_bytes`, respecting UTF-8 char boundaries.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    // Walk back from max_bytes until we land on a char boundary.
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

// ── SSE chunking ──────────────────────────────────────────────────────────────

/// A single SSE chunk frame produced by [`chunk_sse_data`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SseChunkFrame {
    /// 0-based sequence number within this message.
    pub seq: usize,
    /// Total number of frames this message was split into.
    pub total: usize,
    /// Base64-encoded raw bytes for this frame.
    pub data: String,
}

/// Split `payload` into a sequence of [`SseChunkFrame`]s each of at most
/// `chunk_size` bytes.
///
/// If `chunk_size == 0` or `payload.len() <= chunk_size` the input is
/// returned as a single frame with `seq=0, total=1`.
///
/// Frames are raw byte slices (not UTF-8 sub-strings) to guarantee that
/// every frame is exactly `chunk_size` bytes (except the last). The caller
/// is responsible for base64-encoding the `data` field before serialising —
/// the `SseChunkFrame::data` field already contains the base64 representation.
pub fn chunk_sse_data(payload: &[u8], chunk_size: usize) -> Vec<SseChunkFrame> {
    use base64::{Engine, engine::general_purpose::STANDARD};

    if chunk_size == 0 || payload.len() <= chunk_size {
        return vec![SseChunkFrame {
            seq: 0,
            total: 1,
            data: STANDARD.encode(payload),
        }];
    }

    let total = payload.len().div_ceil(chunk_size);
    payload
        .chunks(chunk_size)
        .enumerate()
        .map(|(seq, bytes)| SseChunkFrame {
            seq,
            total,
            data: STANDARD.encode(bytes),
        })
        .collect()
}

/// Format `payload` as SSE text events ready to be sent over the wire.
///
/// - If `payload.len() <= chunk_size` (or `chunk_size == 0`), emits a single
///   `event: message\ndata: <payload>\n\n`.
/// - Otherwise, emits N `event: chunk\ndata: <json>\n\n` events followed by a
///   final `event: chunk_end\ndata: <json>\n\n`.
pub fn format_chunked_sse(payload: &str, chunk_size: usize) -> Vec<String> {
    if chunk_size == 0 || payload.len() <= chunk_size {
        return vec![format!("data: {payload}\n\n")];
    }

    let frames = chunk_sse_data(payload.as_bytes(), chunk_size);
    let total = frames.len();
    frames
        .into_iter()
        .enumerate()
        .map(|(i, frame)| {
            let event_type = if i + 1 == total { "chunk_end" } else { "chunk" };
            let json = serde_json::to_string(&frame).unwrap_or_default();
            format!("event: {event_type}\ndata: {json}\n\n")
        })
        .collect()
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── TruncationEnvelope ──────────────────────────────────────────────────

    #[test]
    fn truncation_envelope_no_truncation_when_under_limit() {
        let env = TruncationEnvelope::new("hello", 100);
        assert!(!env.truncated);
        assert_eq!(env.content, "hello");
        assert_eq!(env.original_size, 5);
        assert_eq!(env.truncated_size, 5);
    }

    #[test]
    fn truncation_envelope_exact_limit_is_not_truncated() {
        let env = TruncationEnvelope::new("hello", 5);
        assert!(!env.truncated);
        assert_eq!(env.content, "hello");
    }

    #[test]
    fn truncation_envelope_truncates_at_byte_limit() {
        let input = "a".repeat(200);
        let env = TruncationEnvelope::new(input.clone(), 100);
        assert!(env.truncated);
        assert_eq!(env.content.len(), 100);
        assert_eq!(env.original_size, 200);
        assert_eq!(env.truncated_size, 100);
    }

    #[test]
    fn truncation_envelope_respects_utf8_char_boundary() {
        // "é" is 2 bytes (0xC3 0xA9).  A limit of 3 bytes must land on
        // a char boundary, yielding "é" (2 bytes) not a broken byte sequence.
        let input = "éàü".to_string(); // 6 bytes total
        let env = TruncationEnvelope::new(input, 3);
        assert!(env.truncated);
        // The result must be valid UTF-8 and ≤ 3 bytes.
        assert!(env.content.len() <= 3);
        assert!(std::str::from_utf8(env.content.as_bytes()).is_ok());
    }

    #[test]
    fn truncation_envelope_zero_limit_means_no_truncation() {
        let env = TruncationEnvelope::new("anything", 0);
        assert!(!env.truncated);
        assert_eq!(env.content, "anything");
    }

    // ── SSE chunking ────────────────────────────────────────────────────────

    #[test]
    fn chunk_sse_data_small_payload_single_frame() {
        let frames = chunk_sse_data(b"hello", 100);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].seq, 0);
        assert_eq!(frames[0].total, 1);
    }

    #[test]
    fn chunk_sse_data_exact_size_single_frame() {
        let frames = chunk_sse_data(b"hello", 5);
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn chunk_sse_data_splits_into_multiple_frames() {
        let payload = b"abcdefgh"; // 8 bytes
        let frames = chunk_sse_data(payload, 3);
        // 8 / 3 = 3 full chunks + 1 partial = 3 frames
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].seq, 0);
        assert_eq!(frames[0].total, 3);
        assert_eq!(frames[2].seq, 2);
    }

    #[test]
    fn chunk_sse_data_zero_chunk_size_returns_single_frame() {
        let frames = chunk_sse_data(b"something", 0);
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn format_chunked_sse_small_payload_plain_event() {
        let events = format_chunked_sse("{\"foo\":1}", 100);
        assert_eq!(events.len(), 1);
        assert!(events[0].starts_with("data: "), "got: {}", events[0]);
        assert!(events[0].ends_with("\n\n"));
    }

    #[test]
    fn format_chunked_sse_large_payload_emits_chunk_events() {
        let payload = "x".repeat(200);
        let events = format_chunked_sse(&payload, 64);
        // Must have > 1 events
        assert!(events.len() > 1, "expected multiple chunk events");
        // All but the last must be `event: chunk`
        for ev in &events[..events.len() - 1] {
            assert!(ev.starts_with("event: chunk\n"), "got: {ev}");
        }
        // Last must be `event: chunk_end`
        let last = events.last().unwrap();
        assert!(last.starts_with("event: chunk_end\n"), "got: {last}");
    }

    #[test]
    fn format_chunked_sse_zero_chunk_size_plain_event() {
        let events = format_chunked_sse("big data", 0);
        assert_eq!(events.len(), 1);
        assert!(events[0].starts_with("data: "));
    }
}
