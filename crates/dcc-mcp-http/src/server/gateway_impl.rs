use std::sync::Arc;

use crate::config::McpHttpConfig;
use crate::gateway::{GatewayConfig, GatewayRunner, LiveSnapshot, MetadataProvider};
use crate::server::{LiveMeta, LiveMetaInner};
use dcc_mcp_transport::discovery::types::ServiceEntry;

pub(crate) async fn start_gateway_runner(
    config: &McpHttpConfig,
    port: u16,
    live_meta: &LiveMeta,
) -> Option<crate::gateway::GatewayHandle> {
    if config.gateway_port == 0 {
        return None;
    }

    let gateway_config = GatewayConfig {
        host: config.host.to_string(),
        gateway_port: config.gateway_port,
        stale_timeout_secs: config.stale_timeout_secs,
        heartbeat_secs: config.heartbeat_secs,
        server_name: config.server_name.clone(),
        server_version: config.server_version.clone(),
        registry_dir: config.registry_dir.clone(),
        challenger_timeout_secs: 120,
        backend_timeout_ms: config.backend_timeout_ms,
        async_dispatch_timeout_ms: config.gateway_async_dispatch_timeout_ms,
        wait_terminal_timeout_ms: config.gateway_wait_terminal_timeout_ms,
        route_ttl_secs: config.gateway_route_ttl_secs,
        max_routes_per_session: config.gateway_max_routes_per_session,
        allow_unknown_tools: config.allow_unknown_tools,
        adapter_version: config.adapter_version.clone(),
        adapter_dcc: config
            .adapter_dcc
            .clone()
            .or_else(|| config.dcc_type.clone()),
    };

    let runner = match GatewayRunner::new(gateway_config) {
        Ok(runner) => runner,
        Err(err) => {
            tracing::warn!("Failed to create GatewayRunner: {err}");
            return None;
        }
    };

    let mut entry = ServiceEntry::new(
        config.dcc_type.as_deref().unwrap_or("unknown"),
        config.host.to_string(),
        port,
    );
    entry.version = config.dcc_version.clone();
    entry.scene = config.scene.clone();
    entry.adapter_version = config.adapter_version.clone();
    entry.adapter_dcc = config
        .adapter_dcc
        .clone()
        .or_else(|| config.dcc_type.clone());

    let metadata_provider = Some(build_metadata_provider(Arc::clone(live_meta)));
    match runner.start(entry, metadata_provider).await {
        Ok(handle) => Some(handle),
        Err(err) => {
            tracing::warn!("Gateway runner failed to start: {err}");
            None
        }
    }
}

fn build_metadata_provider(live_meta: LiveMeta) -> MetadataProvider {
    Arc::new(move || {
        let guard: parking_lot::RwLockReadGuard<'_, LiveMetaInner> = live_meta.read();
        LiveSnapshot {
            scene: guard.scene.clone(),
            version: guard.version.clone(),
            documents: guard.documents.clone(),
            display_name: guard.display_name.clone(),
        }
    })
}
