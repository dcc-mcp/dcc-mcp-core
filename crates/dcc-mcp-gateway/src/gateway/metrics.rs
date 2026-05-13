//! Prometheus `/metrics` endpoint wiring for the gateway (issue #559).
//!
//! When the `prometheus` feature is enabled, this module mounts `GET /metrics`
//! and keeps a process-wide [`PrometheusExporter`] handle so backend hops can
//! increment `dcc_mcp_gateway_backend_errors_total` on the same registry.
//!
//! [`record_gateway_backend_error_kind`](record_gateway_backend_error_kind) is
//! always available (no-op without the `prometheus` feature).

#[cfg(feature = "prometheus")]
use std::sync::{Arc, OnceLock};

#[cfg(feature = "prometheus")]
use axum::{Router, response::IntoResponse};

#[cfg(feature = "prometheus")]
use dcc_mcp_telemetry::PrometheusExporter;

#[cfg(feature = "prometheus")]
static GATEWAY_PROMETHEUS_EXPORTER: OnceLock<Arc<PrometheusExporter>> = OnceLock::new();

/// Increment `dcc_mcp_gateway_backend_errors_total` when the `prometheus`
/// feature is enabled.
#[inline]
pub fn record_gateway_backend_error_kind(kind: &str) {
    #[cfg(feature = "prometheus")]
    {
        if let Some(exp) = GATEWAY_PROMETHEUS_EXPORTER.get() {
            exp.record_gateway_backend_error(kind);
        }
    }
    #[cfg(not(feature = "prometheus"))]
    {
        let _ = kind;
    }
}

/// Attach the `/metrics` route to the gateway router.
#[cfg(feature = "prometheus")]
pub fn attach_gateway_metrics_route(router: Router) -> Router {
    let exporter = Arc::new(PrometheusExporter::new());
    let _ = GATEWAY_PROMETHEUS_EXPORTER.set(exporter.clone());
    router.route(
        "/metrics",
        axum::routing::get({
            let exporter = exporter.clone();
            move || handle_gateway_metrics(exporter.clone())
        }),
    )
}

#[cfg(feature = "prometheus")]
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
