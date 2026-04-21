//! Prometheus `/metrics` endpoint wiring (issue #331).
//!
//! Compiled only when the `prometheus` Cargo feature is enabled on
//! `dcc-mcp-http`. The endpoint sits on the same Axum router as the
//! main MCP handler — it is not exposed on a separate port — so ops
//! teams can front both with the same TLS / ingress layer.
//!
//! # Security
//!
//! Optional HTTP Basic authentication is implemented here (see
//! [`McpHttpConfig::prometheus_basic_auth`](crate::config::McpHttpConfig::prometheus_basic_auth)).
//! When no credentials are configured the endpoint is open — that is
//! acceptable for localhost-only development but production deployments
//! should always configure credentials.

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use std::sync::Arc;

use dcc_mcp_telemetry::{PROMETHEUS_CONTENT_TYPE, PrometheusExporter};

/// Shared application state for the `/metrics` route.
///
/// A clone of this struct is attached to the Axum router via
/// `.with_state(...)`. The exporter is reference-counted; cloning is
/// cheap and thread-safe.
#[derive(Clone)]
pub struct MetricsState {
    pub exporter: PrometheusExporter,
    /// Pre-formatted "user:pass" ASCII bytes used for constant-time
    /// comparison against the Authorization header. `None` means the
    /// endpoint is open.
    pub expected_basic_auth: Option<Arc<Vec<u8>>>,
}

impl MetricsState {
    /// Build a state from an exporter and the optional basic-auth
    /// credentials. The credentials are immediately encoded into the
    /// `user:pass` form so the per-request comparison is a plain byte
    /// equality check.
    pub fn new(exporter: PrometheusExporter, auth: Option<(String, String)>) -> Self {
        let expected = auth.map(|(u, p)| Arc::new(format!("{u}:{p}").into_bytes()));
        Self {
            exporter,
            expected_basic_auth: expected,
        }
    }
}

/// Axum handler for `GET /metrics`.
///
/// Returns a `text/plain; version=0.0.4` payload on success, or
/// `401 Unauthorized` when basic auth is configured and the request
/// fails to present matching credentials.
pub async fn handle_metrics(State(state): State<MetricsState>, headers: HeaderMap) -> Response {
    if let Some(expected) = state.expected_basic_auth.as_ref() {
        let Some(auth_header) = headers.get(header::AUTHORIZATION) else {
            return unauthorized_response();
        };
        let Ok(auth_str) = auth_header.to_str() else {
            return unauthorized_response();
        };
        let Some(encoded) = auth_str.strip_prefix("Basic ") else {
            return unauthorized_response();
        };
        let Ok(decoded) = BASE64_STANDARD.decode(encoded.trim()) else {
            return unauthorized_response();
        };
        // Constant-ish time comparison — we only care about thwarting
        // trivial timing attacks at the HTTP boundary; the outer TLS
        // layer has already bounded what an attacker can learn.
        if !constant_time_eq(&decoded, expected) {
            return unauthorized_response();
        }
    }

    match state.exporter.render() {
        Ok(body) => {
            let mut response = (StatusCode::OK, body).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(PROMETHEUS_CONTENT_TYPE),
            );
            response
        }
        Err(e) => {
            tracing::warn!("Prometheus render failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to render metrics: {e}"),
            )
                .into_response()
        }
    }
}

fn unauthorized_response() -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "Unauthorized\n").into_response();
    // Advertise the auth scheme so curl / scrapers know what to do.
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static(r#"Basic realm="dcc-mcp metrics", charset="UTF-8""#),
    );
    response
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_byte_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn metrics_state_encodes_credentials_for_comparison() {
        let state = MetricsState::new(
            PrometheusExporter::new(),
            Some(("admin".to_string(), "s3cret".to_string())),
        );
        let expected = state.expected_basic_auth.as_ref().unwrap();
        assert_eq!(&**expected, b"admin:s3cret");
    }
}
