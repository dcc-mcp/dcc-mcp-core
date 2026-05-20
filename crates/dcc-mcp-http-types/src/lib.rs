//! Wire-level value types for the DCC MCP HTTP server (issue #852).
//!
//! # Clean Architecture — layer 0 (types)
//!
//! This crate hosts the *pure wire types* exposed by the `dcc-mcp-http`
//! server surface. It intentionally has **no dependency** on:
//!
//! - `axum` / `tower` / `tokio` / any async runtime or HTTP framework,
//! - the `reqwest` client,
//! - `pyo3` or any Python binding machinery,
//! - the broader `dcc-mcp-http` application crate.
//!
//! The dependency direction is strictly inward:
//!
//! ```text
//! dcc-mcp-http (server + pyo3 bindings)  →  dcc-mcp-http-types  (types)
//! ```
//!
//! Consumers in `dcc-mcp-http` re-export these types under their historical
//! paths so existing call sites keep compiling.
//!
//! # Module map
//!
//! | Module      | What lives here                                           |
//! |-------------|-----------------------------------------------------------|
//! | crate root  | [`TruncationEnvelope`], [`SseChunkFrame`] + chunk helpers |
//! | [`error`]   | [`HttpError`] / [`HttpResult`] error taxonomy             |
//! | [`config`]  | [`McpHttpConfig`], [`ServerConfig`], [`SessionConfig`], [`GatewayConfig`], [`ServerSpawnMode`], [`JobRecoveryPolicy`], [`JobConfig`], [`WorkflowConfig`], [`TelemetryConfig`], [`FeatureFlags`], [`InstanceConfig`], [`QueueConfig`] |
//! | [`dynamic_tools`] | [`dynamic_tools::ToolSpec`] dynamic-tool registration wire type |
//! | [`debug_session`] | [`debug_session::DebugSessionDescriptor`] optional debugger attach metadata |
//! | [`output`]  | [`output::OutputStream`] and [`output::OutputEntry`] output capture wire types |
//! | [`prompts`] | [`prompts::PromptSpec`], [`prompts::PromptsSpec`], and related prompt spec types |
//! | [`resources`] | [`resources::ProducerContent`] / [`resources::ResourceError`] resource values |
//! | [`session`] | [`session::SessionLogLevel`] / [`session::SessionLogMessage`] log values |
//! | [`session_events`] | [`session_events::SessionEvent`] bounded adapter runtime event values |
//! | [`ui_automation`] | Cross-DCC UI automation observation/action contract values |
//!
//! # Migration plan (issue #852)
//!
//! The current boundary line:
//!
//! | Lives here now              | Stays in `dcc-mcp-http` for now |
//! |-----------------------------|----------------------------------|
//! | [`TruncationEnvelope`]      |                                  |
//! | [`SseChunkFrame`]           |                                  |
//! | [`chunk_sse_data`]          |                                  |
//! | [`format_chunked_sse`]      |                                  |
//! | [`error::HttpError`]        |                                  |
//! | [`config::ServerConfig`]    |                                  |
//! | [`config::SessionConfig`]   |                                  |
//! | [`config::GatewayConfig`]   |                                  |
//! | [`config::ServerSpawnMode`] |                                  |
//! | [`config::JobRecoveryPolicy`]|                                 |
//! | [`config::JobConfig`]       |                                  |
//! | [`config::WorkflowConfig`]  |                                  |
//! | [`config::TelemetryConfig`] |                                  |
//! | [`config::FeatureFlags`]    |                                  |
//! | [`config::InstanceConfig`]  |                                  |
//! | [`config::QueueConfig`]     |                                  |
//! | [`config::McpHttpConfig`]   |                                  |
//! | [`dynamic_tools::ToolSpec`] |                                  |
//! | [`output::OutputStream`]    |                                  |
//! | [`output::OutputEntry`]     |                                  |
//! | [`prompts::PromptError`]    |                                  |
//! | [`prompts::PromptSpec`]     |                                  |
//! | [`prompts::PromptsSpec`]    |                                  |
//! | [`resources::ProducerContent`] |                                |
//! | [`resources::ResourceError`] |                                  |
//! | [`session::SessionLogLevel`] |                                  |
//! | [`session::SessionLogMessage`] |                                |
//!
//! Each new round of #852 PRs migrates one self-contained subsystem at a
//! time and re-exports it from `dcc-mcp-http` to preserve the public API.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod debug_session;
pub mod dynamic_tools;
pub mod error;
pub mod output;
pub mod prompts;
pub mod resources;
pub mod session;
pub mod session_events;
pub mod ui_automation;

use serde::{Deserialize, Serialize};

// ── Truncation envelope ────────────────────────────────────────────────────

/// Envelope returned by the HTTP server when a response payload exceeds the
/// configured byte limit (issue #771).
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
    #[must_use]
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

// ── SSE chunking ───────────────────────────────────────────────────────────

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
#[must_use]
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
#[must_use]
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
    fn truncation_envelope_respects_utf8_char_boundaries() {
        // Each '日' is 3 UTF-8 bytes; cap at 4 bytes → must truncate to 3.
        let input = "日日日";
        let env = TruncationEnvelope::new(input, 4);
        assert!(env.truncated);
        assert_eq!(env.content.len(), 3);
        assert!(env.content.is_char_boundary(env.content.len()));
    }

    #[test]
    fn truncation_envelope_zero_limit_keeps_content() {
        let env = TruncationEnvelope::new("anything", 0);
        assert!(!env.truncated);
        assert_eq!(env.content, "anything");
    }

    // ── SseChunkFrame ───────────────────────────────────────────────────────

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
        let payload = b"abcdefghij"; // 10 bytes
        let frames = chunk_sse_data(payload, 3);
        // 10 / 3 = 4 (ceil)
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0].seq, 0);
        assert_eq!(frames[3].seq, 3);
        assert_eq!(frames[0].total, 4);
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
        assert!(events[0].starts_with("data: "));
        assert!(!events[0].contains("event: chunk"));
    }

    #[test]
    fn format_chunked_sse_large_payload_emits_chunk_events() {
        let payload = "a".repeat(200);
        let events = format_chunked_sse(&payload, 64);
        assert!(events.len() > 1);
        // All but the last event are "chunk"; the last is "chunk_end".
        for e in events.iter().take(events.len() - 1) {
            assert!(e.starts_with("event: chunk\n"));
        }
        assert!(events.last().unwrap().starts_with("event: chunk_end\n"));
    }

    #[test]
    fn format_chunked_sse_zero_chunk_size_plain_event() {
        let events = format_chunked_sse("big data", 0);
        assert_eq!(events.len(), 1);
        assert!(events[0].starts_with("data: "));
    }
}
