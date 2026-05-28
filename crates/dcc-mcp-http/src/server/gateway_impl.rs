use std::sync::Arc;

use crate::config::McpHttpConfig;
use crate::server::{LiveMeta, LiveMetaInner};
use dcc_mcp_gateway::{GatewayConfig, GatewayRunner, LiveSnapshot, MetadataProvider};
use dcc_mcp_skills::constants::resolve_registry_dcc_type;
use dcc_mcp_transport::discovery::types::ServiceEntry;

pub(crate) async fn start_gateway_runner(
    config: &McpHttpConfig,
    port: u16,
    live_meta: &LiveMeta,
) -> Option<dcc_mcp_gateway::GatewayHandle> {
    if config.gateway.gateway_port == 0 {
        return None;
    }

    let gateway_config = GatewayConfig {
        host: config.server.host.to_string(),
        gateway_port: config.gateway.gateway_port,
        remote_host: config.gateway.remote_host.clone(),
        remote_gateway_port: config.gateway.remote_gateway_port,
        stale_timeout_secs: config.gateway.stale_timeout_secs,
        heartbeat_secs: config.gateway.heartbeat_secs,
        server_name: config.server.server_name.clone(),
        gateway_name: config.gateway.gateway_name.clone(),
        server_version: config.server.server_version.clone(),
        registry_dir: config.gateway.registry_dir.clone(),
        challenger_timeout_secs: 120,
        challenger_poll_interval_secs: 10,
        backend_timeout_ms: config.gateway.backend_timeout_ms,
        async_dispatch_timeout_ms: config.gateway.gateway_async_dispatch_timeout_ms,
        wait_terminal_timeout_ms: config.gateway.gateway_wait_terminal_timeout_ms,
        route_ttl_secs: config.gateway.gateway_route_ttl_secs,
        max_routes_per_session: config.gateway.gateway_max_routes_per_session,
        allow_unknown_tools: config.gateway.allow_unknown_tools,
        #[cfg(feature = "mdns")]
        discover_mdns: config.gateway.discover_mdns,
        policy: config.gateway.policy.clone(),
        adapter_version: config.gateway.adapter_version.clone(),
        adapter_dcc: config
            .gateway
            .adapter_dcc
            .clone()
            .or_else(|| config.instance.dcc_type.clone()),
        middleware_chain: dcc_mcp_gateway::gateway::middleware::MiddlewareChain::new(),
        admin_enabled: config.gateway.admin_enabled,
        admin_path: config.gateway.admin_path.clone(),
        health_check_interval_secs: 5,
        health_check_failures: 2,
        admin_persist: dcc_mcp_gateway::AdminPersistConfig::default(),
    };

    let runner = match GatewayRunner::new(gateway_config) {
        Ok(runner) => runner,
        Err(err) => {
            tracing::warn!("Failed to create GatewayRunner: {err}");
            return None;
        }
    };

    let mut entry = ServiceEntry::new(
        resolve_registry_dcc_type(config.instance.dcc_type.as_deref()),
        config.server.host.to_string(),
        port,
    );
    entry.version = config.instance.dcc_version.clone();
    entry.scene = config.instance.scene.clone();
    entry.adapter_version = config.gateway.adapter_version.clone();
    entry.adapter_dcc = config
        .gateway
        .adapter_dcc
        .clone()
        .or_else(|| config.instance.dcc_type.clone());
    entry.metadata = config.instance.instance_metadata.clone();

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
