use super::*;

#[cfg(feature = "auto-gateway")]
fn unused_local_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn instance_id_is_none_when_gateway_registration_is_disabled() {
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = 0;

    let handle = McpHttpServer::new(Arc::new(ToolRegistry::new()), config)
        .start()
        .await
        .unwrap();

    assert_eq!(handle.instance_id, None);
    handle.shutdown().await;
}

#[cfg(feature = "auto-gateway")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn instance_id_matches_the_registered_service_key() {
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;

    let registry_dir = tempfile::tempdir().unwrap();
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = unused_local_port();
    config.gateway.registry_dir = Some(registry_dir.path().to_path_buf());
    config.gateway.heartbeat_secs = 0;
    config.gateway.admin_enabled = false;
    config.instance.dcc_type = Some("maya".to_string());

    let handle = McpHttpServer::new(Arc::new(ToolRegistry::new()), config)
        .start()
        .await
        .unwrap();
    let instance_id = handle
        .instance_id
        .expect("gateway registration should publish an instance UUID");

    let registry = FileRegistry::new(registry_dir.path()).unwrap();
    let entries = registry.list_instances("maya");
    assert_eq!(entries.len(), 1);
    assert_eq!(instance_id, entries[0].instance_id);

    handle.shutdown().await;
}
