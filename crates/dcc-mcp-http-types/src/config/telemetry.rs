use serde::{Deserialize, Serialize};

// ── TelemetryConfig ────────────────────────────────────────────────────────

/// Prometheus metrics configuration.
///
/// One of the orthogonal sub-configs that compose `McpHttpConfig`
/// (issue #852). The `prometheus` Cargo feature on `dcc-mcp-http`
/// gates the actual `/metrics` endpoint; this struct only carries
/// the user-facing knobs so external config validators can
/// round-trip the shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable the Prometheus `/metrics` endpoint (issue #331).
    ///
    /// Requires the `prometheus` Cargo feature on both `dcc-mcp-http`
    /// and `dcc-mcp-telemetry`. When `true`, the server mounts a
    /// `GET /metrics` route on the same Axum router that serves
    /// `/mcp`; the body is a standard Prometheus text-exposition
    /// payload (`text/plain; version=0.0.4`).
    ///
    /// Defaults to `false`: the endpoint is opt-in, and when the
    /// feature is compiled out this flag has no effect.
    #[serde(default)]
    pub enable_prometheus: bool,

    /// Optional HTTP Basic auth guard for `/metrics` (issue #331).
    ///
    /// When `Some((user, pass))`, scrapers must present a matching
    /// `Authorization: Basic ...` header or the endpoint responds
    /// with `401 Unauthorized`. When `None` (default), the endpoint
    /// is unauthenticated — acceptable for localhost-only
    /// development but strongly discouraged in production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prometheus_basic_auth: Option<(String, String)>,
}
