use super::*;

#[tokio::test]
async fn test_gateway_runner_single_start() {
    use crate::gateway::{GatewayConfig, GatewayRunner};

    let dir = tempfile::tempdir().unwrap();
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: 0,
        heartbeat_secs: 0,
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let runner = GatewayRunner::new(cfg).unwrap();
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let handle = runner.start(entry, None).await.unwrap();
    assert!(!handle.is_gateway);
}

#[tokio::test]
async fn test_gateway_port_competition() {
    use crate::gateway::{GatewayConfig, GatewayRunner};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();

    let cfg1 = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        heartbeat_secs: 0,
        registry_dir: Some(dir1.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let cfg2 = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        heartbeat_secs: 0,
        registry_dir: Some(dir2.path().to_path_buf()),
        ..GatewayConfig::default()
    };

    let runner1 = GatewayRunner::new(cfg1).unwrap();
    let runner2 = GatewayRunner::new(cfg2).unwrap();

    let entry1 = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let entry2 = ServiceEntry::new("maya", "127.0.0.1", 18813);

    let h1 = runner1.start(entry1, None).await.unwrap();
    let h2 = runner2.start(entry2, None).await.unwrap();

    assert_ne!(
        h1.is_gateway, h2.is_gateway,
        "exactly one process should win gateway port (h1={}, h2={})",
        h1.is_gateway, h2.is_gateway
    );
}

#[tokio::test]
async fn test_gateway_runner_is_gateway_true_when_port_free() {
    use crate::gateway::{GatewayConfig, GatewayRunner};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let dir = tempfile::tempdir().unwrap();
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        heartbeat_secs: 0,
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let runner = GatewayRunner::new(cfg).unwrap();
    let entry = ServiceEntry::new("blender", "127.0.0.1", 19000);
    let handle = runner.start(entry, None).await.unwrap();
    assert!(handle.is_gateway, "first runner should win free port");
}
