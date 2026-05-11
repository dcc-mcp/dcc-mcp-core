use serde::{Deserialize, Serialize};

// ── QueueConfig ────────────────────────────────────────────────────────────

/// Queue depth & backpressure configuration (issue #715).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    /// Capacity of the HTTP → `DccExecutor` mpsc channel (issue #715).
    ///
    /// Controls how many outstanding `tools/call` submissions may queue
    /// up before the backpressure path kicks in. When the channel is
    /// full, the HTTP worker blocks for up to [`Self::queue_send_timeout_ms`]
    /// waiting for the DCC main thread to drain; if the drain does not
    /// happen in time, the call returns a structured
    /// `HttpError::QueueOverloaded`.
    ///
    /// Default: `16`. Override via
    /// `--queue-deferred-cap=<N>` / `MCP_QUEUE_DEFERRED_CAP`.
    pub deferred_queue_depth: usize,

    /// Capacity of the `DeferredExecutor` → `host_bridge` mpsc channel
    /// (issue #715).
    ///
    /// Previously hard-coded to `16`. Exposed as a config knob so
    /// operators can tune the bridge depth without patching the crate.
    /// Set to `0` to fall back to `DEFAULT_BRIDGE_QUEUE_DEPTH`
    /// — this keeps misconfigured env-vars (`MCP_QUEUE_BRIDGE_CAP=0`)
    /// from silently disabling backpressure.
    ///
    /// Default: `16`. Override via
    /// `--queue-bridge-cap=<N>` / `MCP_QUEUE_BRIDGE_CAP`.
    pub bridge_queue_depth: usize,

    /// Capacity of the host-side `QueueDispatcher` (issue #715).
    ///
    /// When non-zero, applies to
    /// [`dcc_mcp_host::QueueDispatcher::with_capacity`] so posts that
    /// pile up past `N` surface
    /// [`dcc_mcp_host::DispatchError::QueueOverloaded`] instead of
    /// growing an unbounded queue. When zero (default), the dispatcher
    /// stays unbounded — matches today's behaviour.
    ///
    /// Default: `0` (unbounded). Override via
    /// `--queue-dispatcher-cap=<N>` / `MCP_QUEUE_DISPATCHER_CAP`.
    pub host_queue_depth: usize,

    /// How long an HTTP worker will block on a full executor channel
    /// before returning `HttpError::QueueOverloaded`
    /// (issue #715).
    ///
    /// Chose "block with timeout" over "immediate error" so healthy
    /// bursty workloads still make progress when the main thread
    /// yields momentarily — the typed `QueueOverloaded` only fires on
    /// sustained saturation. Same strategy across all three layers.
    ///
    /// Default: `2_000` ms. Override via
    /// `--queue-send-timeout-ms=<N>` / `MCP_QUEUE_SEND_TIMEOUT_MS`.
    pub queue_send_timeout_ms: u64,

    // ── Issue #771: framework-enforced payload size limits ────────────────
    /// Maximum allowed size (bytes) for an incoming request body (issue #771).
    ///
    /// Enforced via `tower_http::limit::RequestBodyLimitLayer` at the axum
    /// router level. Requests exceeding this size are rejected with
    /// `413 Payload Too Large` before the JSON-RPC layer even sees the body.
    ///
    /// Default: `4_194_304` (4 MiB). Override via
    /// `MCP_MAX_REQUEST_BODY_BYTES`.
    pub max_request_body_bytes: usize,

    /// Maximum content size (bytes) for a single resource, prompt, or tool
    /// call result before the response is truncated (issue #771).
    ///
    /// When a response payload exceeds this limit the server wraps it in a
    /// `TruncationEnvelope`:
    /// ```json
    /// {"content": "...", "truncated": true, "original_size": N, "truncated_size": M}
    /// ```
    ///
    /// Default: `1_048_576` (1 MiB). Override via
    /// `MCP_MAX_RESPONSE_CONTENT_BYTES`.
    pub max_response_content_bytes: usize,

    /// Target chunk size (bytes) for SSE event payloads (issue #771).
    ///
    /// Large SSE events are automatically split into sequential `chunk` /
    /// `chunk_end` events so that a single oversized event cannot stall the
    /// connection's read loop on the client side.
    ///
    /// Set to `0` to disable SSE chunking.
    ///
    /// Default: `65_536` (64 KiB). Override via
    /// `MCP_SSE_CHUNK_SIZE_BYTES`.
    pub sse_chunk_size_bytes: usize,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            deferred_queue_depth: 16,
            bridge_queue_depth: 16,
            host_queue_depth: 0,
            queue_send_timeout_ms: 2_000,
            // #771: payload size limits and SSE chunking
            max_request_body_bytes: 4 * 1024 * 1024, // 4 MiB
            max_response_content_bytes: 1024 * 1024, // 1 MiB
            sse_chunk_size_bytes: 64 * 1024,         // 64 KiB
        }
    }
}

impl QueueConfig {
    /// Load the queue-stack knobs from the environment (issue #715).
    ///
    /// Honours four optional env-vars:
    /// - `MCP_QUEUE_DEFERRED_CAP`
    /// - `MCP_QUEUE_BRIDGE_CAP`
    /// - `MCP_QUEUE_DISPATCHER_CAP`
    /// - `MCP_QUEUE_SEND_TIMEOUT_MS`
    ///
    /// Unset or unparsable values leave the existing field untouched
    /// so a typo never silently flips the cap.
    #[must_use]
    pub fn apply_env_overrides(self) -> Self {
        fn load_usize(key: &str) -> Option<usize> {
            std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
        }
        fn load_u64(key: &str) -> Option<u64> {
            std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
        }
        let mut s = self;
        if let Some(v) = load_usize("MCP_QUEUE_DEFERRED_CAP") {
            s.deferred_queue_depth = v.max(1);
        }
        if let Some(v) = load_usize("MCP_QUEUE_BRIDGE_CAP") {
            s.bridge_queue_depth = v;
        }
        if let Some(v) = load_usize("MCP_QUEUE_DISPATCHER_CAP") {
            s.host_queue_depth = v;
        }
        if let Some(v) = load_u64("MCP_QUEUE_SEND_TIMEOUT_MS") {
            s.queue_send_timeout_ms = v;
        }
        s
    }

    /// Builder: set the maximum request body size (issue #771).
    ///
    /// Requests larger than `bytes` are rejected with 413 at the axum router
    /// level via `tower_http::limit::RequestBodyLimitLayer`.
    #[must_use]
    pub fn with_max_request_body_bytes(mut self, bytes: usize) -> Self {
        self.max_request_body_bytes = bytes;
        self
    }

    /// Builder: set the maximum response content size before truncation
    /// (issue #771).
    ///
    /// Resource, prompt and tool-call responses larger than `bytes` are
    /// wrapped in a `TruncationEnvelope`.
    #[must_use]
    pub fn with_max_response_content_bytes(mut self, bytes: usize) -> Self {
        self.max_response_content_bytes = bytes;
        self
    }

    /// Builder: set the SSE chunking threshold (issue #771).
    ///
    /// SSE events larger than `bytes` are split into sequential
    /// `chunk` / `chunk_end` frames. Set to `0` to disable chunking.
    #[must_use]
    pub fn with_sse_chunk_size_bytes(mut self, bytes: usize) -> Self {
        self.sse_chunk_size_bytes = bytes;
        self
    }
}
