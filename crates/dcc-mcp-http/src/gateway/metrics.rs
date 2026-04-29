//! Prometheus `/metrics` endpoint wiring for the gateway (issue #559).
//!
//! This module is compiled only when the `prometheus` Cargo feature is enabled.
//! It mounts a `GET /metrics` route on the gateway router and exposes
//! gateway-specific gauges/counters in addition to the per-instance metrics.

use axum::{Router, response::IntoResponse};
use std::sync::Arc;

use dcc_mcp_telemetry::PrometheusExporter;

/// Attach the `/metrics` route to the gateway router.
///
/// The exporter is created fresh here; the caller can later clone it
/// into background tasks that update gauges. The handler is a closure
/// over an `Arc<PrometheusExporter>` so the route does not change the
/// router's `S` (state) type — keeping the `Router<()>` shape that the
/// rest of the gateway expects.
pub fn attach_gateway_metrics_route(router: Router) -> Router {
    let exporter = Arc::new(PrometheusExporter::new());
    router.route(
        "/metrics",
        axum::routing::get({
            let exporter = exporter.clone();
            move || handle_gateway_metrics(exporter.clone())
        }),
    )
}

async fn handle_gateway_metrics(exporter: Arc<PrometheusExporter>) -> impl IntoResponse {
    match exporter.render() {
        Ok(body) => {
            let mut response = (axum::http::StatusCode::OK, body).into_response();
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static(dcc_mcp_telemetry::PROMETHEUS_CONTENT_TYPE),
            );
            response
        }
        Err(e) => {
            tracing::warn!("Prometheus render failed: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to render metrics: {e}"),
            )
                .into_response()
        }
    }
}
