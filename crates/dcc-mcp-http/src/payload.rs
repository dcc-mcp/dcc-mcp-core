//! Framework-enforced payload size limits and SSE chunking (issue #771).
//!
//! # Relocation notice (issue #852)
//!
//! The wire types and helpers in this module were migrated to the
//! dedicated [`dcc-mcp-http-types`](dcc_mcp_http_types) crate as part of
//! the `dcc-mcp-http` Clean-Architecture split (issue #852). This module
//! now re-exports them so existing call sites keep compiling under their
//! historical paths; the domain-level definitions live in
//! `dcc-mcp-http-types`.
//!
//! New code should depend on `dcc-mcp-http-types` directly when it only
//! needs the wire types.
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
//! client's read loop.  See [`chunk_sse_data`] and [`format_chunked_sse`].

pub use dcc_mcp_http_types::{
    SseChunkFrame, TruncationEnvelope, chunk_sse_data, format_chunked_sse,
};
