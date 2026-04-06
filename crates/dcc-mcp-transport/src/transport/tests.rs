//! TransportManager unit tests.

use super::*;

use crate::config::TransportConfig;
use crate::ipc::TransportAddress;
use crate::listener::IpcListener;

fn setup() -> (tempfile::TempDir, TransportManager) {
    let dir = tempfile::tempdir().unwrap();
    let manager = TransportManager::new(TransportConfig::default(), dir.path()).unwrap();
    (dir, manager)
}

#[test]
fn test_transport_manager_register_service() {
    let (_dir, manager) = setup();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    manager.register_service(entry).unwrap();

    assert_eq!(manager.list_instances("maya").len(), 1);
}

#[test]
fn test_transport_manager_deregister_service() {
    let (_dir, manager) = setup();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    manager.register_service(entry).unwrap();

    let removed = manager.deregister_service(&key).unwrap();
    assert!(removed.is_some());
    assert!(manager.list_instances("maya").is_empty());
}

#[test]
fn test_transport_manager_session_lifecycle() {
    let (_dir, manager) = setup();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let instance_id = entry.instance_id;
    manager.register_service(entry).unwrap();

    // Create a session
    let session_id = manager
        .get_or_create_session("maya", Some(instance_id))
        .unwrap();
    assert_eq!(manager.session_count(), 1);

    // Get session info
    let session = manager.get_session(&session_id).unwrap();
    assert_eq!(session.dcc_type, "maya");
    assert_eq!(session.instance_id, instance_id);

    // Record some metrics
    manager.record_request_success(&session_id, Duration::from_millis(100));
    manager.record_request_error(&session_id, Duration::from_millis(50), "timeout");

    let session = manager.get_session(&session_id).unwrap();
    assert_eq!(session.metrics.request_count, 2);
    assert_eq!(session.metrics.error_count, 1);

    // Close session
    let closed = manager.close_session(&session_id).unwrap();
    assert!(closed.is_some());
    assert_eq!(manager.session_count(), 0);
}

#[test]
fn test_transport_manager_session_auto_pick() {
    let (_dir, manager) = setup();

    manager
        .register_service(ServiceEntry::new("maya", "127.0.0.1", 18812))
        .unwrap();

    // Should auto-pick the available instance
    let _session_id = manager.get_or_create_session("maya", None).unwrap();
    assert_eq!(manager.session_count(), 1);
}

#[tokio::test]
async fn test_transport_manager_acquire_connection() {
    // Bind a listener on a dynamic port so no real external service is needed.
    let listen_addr = TransportAddress::tcp("127.0.0.1", 0);
    let listener = IpcListener::bind(&listen_addr).await.unwrap();
    let local_addr = listener.local_address().unwrap();
    let port = match &local_addr {
        TransportAddress::Tcp { port, .. } => *port,
        _ => panic!("expected TCP address"),
    };

    let (_dir, manager) = setup();
    let entry = ServiceEntry::new("maya", "127.0.0.1", port);
    let key = entry.key();
    let instance_id = entry.instance_id;
    manager.register_service(entry).unwrap();

    // Accept in the background so that acquire_connection has a peer to connect to.
    let accept_handle = tokio::spawn(async move {
        listener.accept().await.unwrap();
    });

    let _conn_id = manager
        .acquire_connection("maya", Some(instance_id))
        .await
        .unwrap();
    assert_eq!(manager.pool_size(), 1);

    manager.release_connection(&key);
    let _ = accept_handle.await;
}

#[tokio::test]
async fn test_transport_manager_acquire_any_instance() {
    // Two listeners on dynamic ports.
    let addr1 = TransportAddress::tcp("127.0.0.1", 0);
    let listener1 = IpcListener::bind(&addr1).await.unwrap();
    let port1 = match listener1.local_address().unwrap() {
        TransportAddress::Tcp { port, .. } => port,
        _ => panic!("expected TCP"),
    };

    let addr2 = TransportAddress::tcp("127.0.0.1", 0);
    let listener2 = IpcListener::bind(&addr2).await.unwrap();
    let port2 = match listener2.local_address().unwrap() {
        TransportAddress::Tcp { port, .. } => port,
        _ => panic!("expected TCP"),
    };

    let (_dir, manager) = setup();
    manager
        .register_service(ServiceEntry::new("maya", "127.0.0.1", port1))
        .unwrap();
    manager
        .register_service(ServiceEntry::new("maya", "127.0.0.1", port2))
        .unwrap();

    // Accept on both so the pool can pick either.
    let accept1 = tokio::spawn(async move { listener1.accept().await });
    let accept2 = tokio::spawn(async move { listener2.accept().await });

    let _conn_id = manager.acquire_connection("maya", None).await.unwrap();
    assert_eq!(manager.pool_size(), 1);

    // One of the two accept handles will have succeeded; abort the other.
    accept1.abort();
    accept2.abort();
}

#[tokio::test]
async fn test_transport_manager_accept_into_pool() {
    // Start a listener on a dynamic port.
    let listen_addr = TransportAddress::tcp("127.0.0.1", 0);
    let listener = IpcListener::bind(&listen_addr).await.unwrap();
    let local_addr = listener.local_address().unwrap();
    let port = match &local_addr {
        TransportAddress::Tcp { port, .. } => *port,
        _ => panic!("expected TCP address"),
    };

    let (_dir, manager) = setup();
    let service_key = ServiceKey {
        dcc_type: "maya".to_string(),
        instance_id: Uuid::new_v4(),
    };
    let addr = TransportAddress::tcp("127.0.0.1", port);

    // Connect a client in background so accept doesn't block forever.
    tokio::spawn(async move {
        let _ = crate::connector::connect(&addr, std::time::Duration::from_secs(5)).await;
    });

    let conn_id = manager
        .accept_into_pool(&listener, service_key.clone(), local_addr)
        .await
        .unwrap();

    // Verify the connection landed in the pool.
    assert!(manager.pool.get_active(&service_key).is_some());
    let active = manager.pool.get_active(&service_key).unwrap();
    assert_eq!(active.lock().unwrap().id, conn_id);
}

#[tokio::test]
async fn test_transport_manager_service_not_found() {
    let (_dir, manager) = setup();

    let result = manager.acquire_connection("maya", None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        TransportError::ServiceNotFound { .. }
    ));
}

#[test]
fn test_transport_manager_shutdown() {
    let (_dir, manager) = setup();

    // Create some state
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let instance_id = entry.instance_id;
    manager.register_service(entry).unwrap();
    manager
        .get_or_create_session("maya", Some(instance_id))
        .unwrap();

    assert!(!manager.is_shutdown());
    let (sessions, _connections) = manager.shutdown();
    assert!(manager.is_shutdown());
    assert_eq!(sessions.len(), 1);

    // Operations should fail after shutdown
    let entry = ServiceEntry::new("blender", "127.0.0.1", 9090);
    assert!(matches!(
        manager.register_service(entry),
        Err(TransportError::Shutdown)
    ));
}

#[test]
fn test_transport_manager_cleanup() {
    let (_dir, manager) = setup();

    let (stale, sessions, evicted) = manager.cleanup().unwrap();
    assert_eq!(stale, 0);
    assert_eq!(sessions, 0);
    assert_eq!(evicted, 0);
}

#[test]
fn test_transport_manager_deregister_closes_session() {
    let (_dir, manager) = setup();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    let instance_id = entry.instance_id;
    manager.register_service(entry).unwrap();

    // Create session
    manager
        .get_or_create_session("maya", Some(instance_id))
        .unwrap();
    assert_eq!(manager.session_count(), 1);

    // Deregistering should also close the session
    manager.deregister_service(&key).unwrap();
    assert_eq!(manager.session_count(), 0);
}

#[test]
fn test_transport_manager_update_service_status() {
    let (_dir, manager) = setup();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    manager.register_service(entry).unwrap();

    // Verify initial status
    let service = manager.get_service(&key).unwrap();
    assert_eq!(service.status, ServiceStatus::Available);

    // Update to Busy
    let updated = manager
        .update_service_status(&key, ServiceStatus::Busy)
        .unwrap();
    assert!(updated);

    let service = manager.get_service(&key).unwrap();
    assert_eq!(service.status, ServiceStatus::Busy);

    // Update to Unreachable
    manager
        .update_service_status(&key, ServiceStatus::Unreachable)
        .unwrap();
    let service = manager.get_service(&key).unwrap();
    assert_eq!(service.status, ServiceStatus::Unreachable);
}

#[test]
fn test_transport_manager_update_status_nonexistent() {
    let (_dir, manager) = setup();

    let key = ServiceKey {
        dcc_type: "maya".to_string(),
        instance_id: uuid::Uuid::new_v4(),
    };
    let updated = manager
        .update_service_status(&key, ServiceStatus::Busy)
        .unwrap();
    assert!(!updated);
}

#[test]
fn test_transport_manager_update_status_after_shutdown() {
    let (_dir, manager) = setup();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    manager.register_service(entry).unwrap();

    manager.shutdown();

    let result = manager.update_service_status(&key, ServiceStatus::Busy);
    assert!(matches!(result, Err(TransportError::Shutdown)));
}

#[test]
fn test_transport_manager_listen_no_address_configured() {
    let (_dir, manager) = setup();
    // No listen_address in default config → should error
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(manager.listen());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("listen_address not configured"));
}

#[tokio::test]
async fn test_transport_manager_listen_with_address() {
    let dir = tempfile::tempdir().unwrap();
    let config = TransportConfig {
        listen_address: Some("tcp://127.0.0.1:0".to_string()),
        ..Default::default()
    };
    let manager = TransportManager::new(config, dir.path()).unwrap();

    let listener = manager.listen().await.unwrap();
    let local = listener.local_address().unwrap();
    // The OS assigns a real port > 0
    match local {
        TransportAddress::Tcp { port, .. } => assert!(port > 0),
        _ => panic!("expected TCP address"),
    }
}

#[tokio::test]
async fn test_transport_manager_get_active_connection_none() {
    let (_dir, manager) = setup();
    let key = ServiceKey {
        dcc_type: "maya".to_string(),
        instance_id: Uuid::new_v4(),
    };
    assert!(manager.get_active_connection(&key).is_none());
}

#[tokio::test]
async fn test_transport_manager_get_active_connection_after_accept() {
    let listen_addr = TransportAddress::tcp("127.0.0.1", 0);
    let listener = IpcListener::bind(&listen_addr).await.unwrap();
    let local_addr = listener.local_address().unwrap();
    let port = match &local_addr {
        TransportAddress::Tcp { port, .. } => *port,
        _ => panic!("expected TCP address"),
    };

    let (_dir, manager) = setup();
    let service_key = ServiceKey {
        dcc_type: "maya".to_string(),
        instance_id: Uuid::new_v4(),
    };
    let addr = TransportAddress::tcp("127.0.0.1", port);

    tokio::spawn(async move {
        let _ = crate::connector::connect(&addr, std::time::Duration::from_secs(5)).await;
    });

    let _conn_id = manager
        .accept_into_pool(&listener, service_key.clone(), local_addr)
        .await
        .unwrap();

    let arc = manager.get_active_connection(&service_key);
    assert!(arc.is_some());
    let guard = arc.unwrap();
    let conn = guard.lock().unwrap();
    assert!(conn.framed().is_some());
}
