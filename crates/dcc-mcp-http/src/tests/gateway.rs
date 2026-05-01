use axum::http::HeaderValue;
use axum_test::TestServer;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

use crate::gateway::router::build_gateway_router;
use crate::gateway::state::GatewayState;

fn make_gateway_state() -> GatewayState {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.keep();
    let registry = FileRegistry::new(&path).unwrap();
    let (yield_tx, _yield_rx) = tokio::sync::watch::channel(false);
    let (events_tx, _) = tokio::sync::broadcast::channel(16);
    GatewayState {
        registry: Arc::new(RwLock::new(registry)),
        stale_timeout: Duration::from_secs(30),
        backend_timeout: Duration::from_secs(10),
        async_dispatch_timeout: Duration::from_secs(60),
        wait_terminal_timeout: Duration::from_secs(600),
        server_name: "test-gateway".to_string(),
        server_version: "0.1.0".to_string(),
        own_host: "127.0.0.1".to_string(),
        own_port: 0,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        adapter_version: None,
        adapter_dcc: None,
        tool_exposure: crate::gateway::GatewayToolExposure::Full,
        cursor_safe_tool_names: true,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
    }
}

fn make_gateway_router() -> axum::Router {
    build_gateway_router(make_gateway_state())
}

#[path = "gateway_batch.rs"]
mod batch;
#[path = "gateway_mcp.rs"]
mod mcp;
#[path = "gateway_pagination.rs"]
mod pagination;
#[path = "gateway_protocol.rs"]
mod protocol;
#[path = "gateway_resources.rs"]
mod resources;
#[path = "gateway_rest.rs"]
mod rest;
#[path = "gateway_runner.rs"]
mod runner;
#[path = "gateway_session.rs"]
mod session;
