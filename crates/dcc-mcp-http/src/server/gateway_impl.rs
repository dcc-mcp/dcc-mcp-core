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
        // The embedded HTTP server owns no pidfile; recovery from a
        // service-dead port holder (#2405) still works through the
        // gateway autolaunch manifest written into the registry directory.
        pidfile: None,
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
        relay_sources: config
            .gateway
            .relay_sources
            .iter()
            .map(|source| dcc_mcp_gateway::RelaySourceConfig {
                admin_url: source.admin_url.clone(),
                public_base_url: source.public_base_url.clone(),
                poll_interval_secs: source.poll_interval_secs,
            })
            .collect(),
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
        // #1365 — embedded auto-gateway never enables auth on the
        // registration plane; that mode lives on a single workstation
        // and trusts the local file registry. Daemon mode opts in via
        // GatewayRunner::with_config and is documented in
        // docs/guide/gateway.md § Security.
        auth: dcc_mcp_gateway::GatewayAuth::disabled(),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
        semantic_search_enabled: false,
    };

    let runner = match GatewayRunner::new(gateway_config) {
        Ok(runner) => runner,
        Err(err) => {
            tracing::warn!("Failed to create GatewayRunner: {err}");
            return None;
        }
    };

    let entry = build_registration_entry(config, port);

    let metadata_provider = Some(build_metadata_provider(Arc::clone(live_meta)));
    match runner.start(entry, metadata_provider).await {
        Ok(handle) => Some(handle),
        Err(err) => {
            tracing::warn!("Gateway runner failed to start: {err}");
            None
        }
    }
}

/// Build the `FileRegistry` row this server publishes.
///
/// Kept free of gateway wiring so the seeding rules can be unit-tested
/// without starting a gateway.
fn build_registration_entry(config: &McpHttpConfig, port: u16) -> ServiceEntry {
    let mut entry = ServiceEntry::new(
        resolve_registry_dcc_type(config.instance.dcc_type.as_deref()),
        config.server.host.to_string(),
        port,
    );
    entry.version = config.instance.dcc_version.clone();
    entry.host_pid = config.instance.host_pid;
    entry.scene = config.instance.scene.clone();
    entry.adapter_version = config.gateway.adapter_version.clone();
    entry.adapter_dcc = config
        .gateway
        .adapter_dcc
        .clone()
        .or_else(|| config.instance.dcc_type.clone());
    entry.metadata = config.instance.instance_metadata.clone();
    entry.extras = config.instance.instance_extras.clone();
    // A JSON null is a removal tombstone, not a stored value — see
    // `LiveMetaInner::extras`. Persisting one here would publish the null to
    // services.json and to every reader, only for the next heartbeat to send
    // the same null back as a delete. Drop the tombstones from the seeded row
    // but keep them in the live map, which still needs them as delete markers.
    entry.extras.retain(|_, value| !value.is_null());
    entry
}

fn build_metadata_provider(live_meta: LiveMeta) -> MetadataProvider {
    Arc::new(move || {
        let guard: parking_lot::RwLockReadGuard<'_, LiveMetaInner> = live_meta.read();
        LiveSnapshot {
            scene: guard.scene.clone(),
            version: guard.version.clone(),
            documents: guard.documents.clone(),
            display_name: guard.display_name.clone(),
            metadata: guard.metadata.clone(),
            extras: guard.extras.clone(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Issue #2500 — a `Null` in `instance_extras` is a removal tombstone, not a
    /// value. The seeded registration row must not publish it: the heartbeat
    /// would subsequently send the same null back as a delete, so persisting it
    /// first would briefly expose a null to every reader and contradict the
    /// getter, which filters tombstones out.
    #[test]
    fn registration_entry_drops_extras_tombstones() {
        let mut config = McpHttpConfig::default();
        config.instance.dcc_type = Some("auroraview".to_string());
        config.set_instance_extras(HashMap::from([
            ("host_dcc".to_string(), serde_json::json!("maya-2024")),
            ("cdp_port".to_string(), serde_json::json!(9222)),
            ("dropped".to_string(), serde_json::Value::Null),
        ]));

        let entry = build_registration_entry(&config, 18812);

        assert_eq!(
            entry.extras.get("host_dcc"),
            Some(&serde_json::json!("maya-2024")),
            "seeded values must be published"
        );
        assert_eq!(
            entry.extras.get("cdp_port"),
            Some(&serde_json::json!(9222)),
            "types must survive the seed"
        );
        assert!(
            !entry.extras.contains_key("dropped"),
            "tombstones must not reach the registration row"
        );
    }

    #[test]
    fn registration_entry_without_extras_stays_empty() {
        let config = McpHttpConfig::default();
        let entry = build_registration_entry(&config, 18812);
        assert!(entry.extras.is_empty());
    }
}
