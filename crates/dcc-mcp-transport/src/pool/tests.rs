//! ConnectionPool unit tests.

use super::*;

use std::time::Duration;

fn make_key(dcc_type: &str) -> ServiceKey {
    ServiceKey {
        dcc_type: dcc_type.to_string(),
        instance_id: Uuid::new_v4(),
    }
}

#[test]
fn test_pool_new() {
    let pool = ConnectionPool::new(PoolConfig::default());
    assert!(pool.is_empty());
    assert_eq!(pool.len(), 0);
    assert!(pool.list_connections().is_empty());
}

#[test]
fn test_pooled_connection_new() {
    let key = make_key("maya");
    let addr = TransportAddress::tcp("127.0.0.1", 18812);
    let conn = PooledConnection::new(key, addr);
    assert_eq!(conn.state, ConnectionState::Available);
    assert!(!conn.is_expired(Duration::from_secs(60)));
    assert!(conn.is_expired(Duration::from_nanos(0)));
    assert_eq!(conn.host(), "127.0.0.1");
    assert_eq!(conn.port(), 18812);
}

#[test]
fn test_pooled_connection_touch() {
    let key = make_key("blender");
    let addr = TransportAddress::named_pipe("test-pipe");
    let mut conn = PooledConnection::new(key, addr);
    assert_eq!(conn.request_count, 0);
    conn.touch();
    assert_eq!(conn.request_count, 1);
    assert_eq!(conn.host(), "127.0.0.1");
    assert_eq!(conn.port(), 0);
}

#[test]
fn test_pool_drain() {
    let pool = ConnectionPool::new(PoolConfig::default());
    let drained = pool.drain();
    assert!(drained.is_empty());
}

#[test]
fn test_pool_evict_stale() {
    let config = PoolConfig {
        max_idle_time: Duration::from_millis(0),
        ..Default::default()
    };
    let pool = ConnectionPool::new(config);
    assert_eq!(pool.evict_stale(), 0);
}

#[test]
fn test_pool_acquire_and_release() {
    let pool = ConnectionPool::new(PoolConfig::default());
    let key = make_key("maya");
    assert!(pool.remove(&key).is_none());
    assert!(pool.remove_metadata(&key).is_none());
}

#[test]
fn test_pool_count_for_dcc() {
    let pool = ConnectionPool::new(PoolConfig::default());
    assert_eq!(pool.count_for_dcc("maya"), 0);
}

#[tokio::test]
async fn test_acquire_active_tcp_connection() {
    let pool = ConnectionPool::new(PoolConfig::default());
    let key = make_key("maya");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = TransportAddress::tcp("127.0.0.1", port);

    tokio::spawn(async move {
        loop {
            let _ = listener.accept().await;
        }
    });

    let conn = pool
        .acquire_active(&key, &addr, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(pool.len(), 1);

    let guard = conn.lock().unwrap();
    assert!(guard.is_alive());
    assert_eq!(guard.state, ConnectionState::InUse);
    assert_eq!(guard.transport_name(), "tcp");
}

#[tokio::test]
async fn test_acquire_reuses_available_connection() {
    let pool = ConnectionPool::new(PoolConfig::default());
    let key = make_key("maya");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = TransportAddress::tcp("127.0.0.1", port);

    tokio::spawn(async move {
        loop {
            let _ = listener.accept().await;
        }
    });

    let c1 = pool
        .acquire_active(&key, &addr, Duration::from_secs(5))
        .await
        .unwrap();
    let id1 = c1.lock().unwrap().id;
    pool.release(&key);
    let c2 = pool
        .acquire_active(&key, &addr, Duration::from_secs(5))
        .await
        .unwrap();
    let id2 = c2.lock().unwrap().id;

    assert_eq!(id1, id2); // Same connection reused
    assert_eq!(pool.len(), 1);
}

#[tokio::test]
async fn test_acquire_backward_compat_returns_uuid() {
    let pool = ConnectionPool::new(PoolConfig::default());
    let key = make_key("maya");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let _ = listener.accept().await;
        }
    });

    let _id = pool.acquire(&key, "127.0.0.1", port).await.unwrap();
    assert_eq!(pool.len(), 1);
}

#[tokio::test]
async fn test_reconnect_active_with_backoff_success() {
    // First connect, then "kill" the connection by taking the framed,
    // then reconnect — should succeed and give us a fresh connection.
    let pool = ConnectionPool::new(PoolConfig::default());
    let key = make_key("maya");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = TransportAddress::tcp("127.0.0.1", port);

    tokio::spawn(async move {
        loop {
            let _ = listener.accept().await;
        }
    });

    // Initial connection
    let conn = pool
        .acquire_active(&key, &addr, Duration::from_secs(5))
        .await
        .unwrap();
    let first_id = conn.lock().unwrap().id;
    pool.release(&key);

    // Simulate dead connection by taking the FramedIo
    {
        let mut guard = conn.lock().unwrap();
        guard.take_framed();
    }

    // Reconnect should create a new connection
    let reconnected = pool
        .reconnect_active_with_backoff(
            &key,
            &addr,
            Duration::from_secs(5),
            Duration::from_millis(10),
            3,
        )
        .await
        .unwrap();

    let second_id = reconnected.lock().unwrap().id;
    assert_ne!(first_id, second_id, "should be a new connection");
    assert_eq!(pool.len(), 1, "pool should still have exactly 1 entry");
}

#[tokio::test]
async fn test_reconnect_active_with_backoff_fails_after_retries() {
    let pool = ConnectionPool::new(PoolConfig::default());
    let key = make_key("houdini");
    // Port that nothing is listening on
    let addr = TransportAddress::tcp("127.0.0.1", 1);

    let result = pool
        .reconnect_active_with_backoff(
            &key,
            &addr,
            Duration::from_millis(100),
            Duration::from_millis(1),
            2,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("reconnect failed"),
        "unexpected error: {err_str}"
    );
}
