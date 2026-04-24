use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) fn spawn_session_eviction_task(
    sessions: &crate::session::SessionManager,
    session_ttl_secs: u64,
) {
    if session_ttl_secs == 0 {
        return;
    }

    let sessions_bg = sessions.clone();
    let ttl = Duration::from_secs(session_ttl_secs);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            sessions_bg.evict_stale(ttl);
        }
    });
}

pub(crate) fn spawn_cancellation_gc_task(
    cancelled_requests: &Arc<dashmap::DashMap<String, Instant>>,
) {
    let cancelled_requests_bg = Arc::clone(cancelled_requests);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            cancelled_requests_bg.retain(|_, recorded_at: &mut Instant| {
                recorded_at.elapsed() < Duration::from_secs(30)
            });
        }
    });
}

pub(crate) fn spawn_resource_update_forwarder(
    enable_resources: bool,
    resources: &crate::resources::ResourceRegistry,
    sessions: &crate::session::SessionManager,
) {
    if !enable_resources {
        return;
    }

    let resources_bg = resources.clone();
    let sessions_bg = sessions.clone();
    tokio::spawn(async move {
        let mut rx = resources_bg.watch_updates();
        while let Ok(uri) = rx.recv().await {
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/resources/updated",
                "params": { "uri": uri }
            });
            let event = crate::protocol::format_sse_event(&notification, None);
            for session_id in resources_bg.sessions_subscribed_to(&uri) {
                sessions_bg.push_event(&session_id, event.clone());
            }
        }
    });
}

#[cfg(feature = "prometheus")]
pub(crate) fn prometheus_gauge_context(
    config: &crate::config::McpHttpConfig,
    registry: Arc<dcc_mcp_actions::ActionRegistry>,
    sessions: crate::session::SessionManager,
) -> Option<(
    Arc<dcc_mcp_actions::ActionRegistry>,
    crate::session::SessionManager,
)> {
    if config.enable_prometheus {
        Some((registry, sessions))
    } else {
        None
    }
}

#[cfg(feature = "prometheus")]
pub(crate) fn build_prometheus_exporter(
    config: &crate::config::McpHttpConfig,
    registry: &dcc_mcp_actions::ActionRegistry,
) -> Option<dcc_mcp_telemetry::PrometheusExporter> {
    if !config.enable_prometheus {
        return None;
    }

    let exporter = dcc_mcp_telemetry::PrometheusExporter::new();
    exporter.set_registered_tools(registry.list_actions(None).len() as i64);
    Some(exporter)
}

#[cfg(feature = "prometheus")]
pub(crate) fn attach_metrics_route(
    mut router: axum::Router,
    exporter: &Option<dcc_mcp_telemetry::PrometheusExporter>,
    config: &crate::config::McpHttpConfig,
    gauge_ctx: Option<(
        Arc<dcc_mcp_actions::ActionRegistry>,
        crate::session::SessionManager,
    )>,
) -> axum::Router {
    if let Some(exporter) = exporter.as_ref() {
        let metrics_state = crate::metrics::MetricsState::new(
            exporter.clone(),
            config.prometheus_basic_auth.clone(),
        );
        let metrics_router = axum::Router::new()
            .route(
                "/metrics",
                axum::routing::get(crate::metrics::handle_metrics),
            )
            .with_state(metrics_state);
        router = router.merge(metrics_router);
        tracing::info!("Prometheus /metrics endpoint enabled");

        if let Some((registry, sessions)) = gauge_ctx {
            let exporter_bg = exporter.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    exporter_bg.set_registered_tools(registry.list_actions(None).len() as i64);
                    exporter_bg.set_active_sessions(sessions.count() as i64);
                }
            });
        }
    }

    router
}
