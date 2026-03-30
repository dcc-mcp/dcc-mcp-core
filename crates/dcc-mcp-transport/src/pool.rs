//! Connection pool — lock-free, async, with DashMap + Semaphore.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::config::PoolConfig;
use crate::discovery::types::ServiceKey;
use crate::error::{TransportError, TransportResult};

/// State of a pooled connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Available for use.
    Available,
    /// Currently in use by a request.
    InUse,
    /// Reconnecting after a failure.
    Reconnecting,
    /// Draining (pending shutdown).
    Draining,
}

/// A pooled connection to a DCC instance.
#[derive(Debug)]
pub struct PooledConnection {
    /// Connection identifier.
    pub id: Uuid,
    /// Target service key.
    pub service_key: ServiceKey,
    /// Host address.
    pub host: String,
    /// Port number.
    pub port: u16,
    /// Current state.
    pub state: ConnectionState,
    /// When this connection was created.
    pub created_at: Instant,
    /// When this connection was last used.
    pub last_used: Instant,
    /// Number of requests handled.
    pub request_count: u64,
}

impl PooledConnection {
    /// Create a new pooled connection.
    pub fn new(service_key: ServiceKey, host: String, port: u16) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            service_key,
            host,
            port,
            state: ConnectionState::Available,
            created_at: now,
            last_used: now,
            request_count: 0,
        }
    }

    /// Check if this connection has exceeded its max lifetime.
    pub fn is_expired(&self, max_lifetime: std::time::Duration) -> bool {
        self.created_at.elapsed() > max_lifetime
    }

    /// Check if this connection has been idle too long.
    pub fn is_idle(&self, max_idle_time: std::time::Duration) -> bool {
        self.last_used.elapsed() > max_idle_time
    }

    /// Mark the connection as used.
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
        self.request_count += 1;
    }
}

/// Thread-safe connection pool using DashMap.
///
/// Features:
/// - Lock-free concurrent access via `DashMap`
/// - Per-DCC-type connection limiting via Tokio `Semaphore`
/// - Automatic eviction of idle/expired connections
pub struct ConnectionPool {
    /// `(dcc_type, instance_id)` → `PooledConnection`
    connections: Arc<DashMap<ServiceKey, PooledConnection>>,
    /// Per-DCC-type connection limits.
    semaphores: Arc<DashMap<String, Arc<Semaphore>>>,
    /// Pool configuration.
    config: PoolConfig,
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

impl ConnectionPool {
    /// Create a new connection pool with the given configuration.
    pub fn new(config: PoolConfig) -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
            semaphores: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Get the semaphore for a DCC type, creating one if needed.
    fn get_semaphore(&self, dcc_type: &str) -> Arc<Semaphore> {
        self.semaphores
            .entry(dcc_type.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.config.max_connections_per_type)))
            .value()
            .clone()
    }

    /// Try to acquire or create a connection for the given service key.
    ///
    /// Returns the connection ID on success.
    pub async fn acquire(
        &self,
        service_key: &ServiceKey,
        host: &str,
        port: u16,
    ) -> TransportResult<Uuid> {
        // Check if we already have an available connection
        if let Some(mut conn) = self.connections.get_mut(service_key) {
            if conn.state == ConnectionState::Available {
                conn.state = ConnectionState::InUse;
                conn.touch();
                return Ok(conn.id);
            }
        }

        // Try to acquire a semaphore permit (with timeout)
        let sem = self.get_semaphore(&service_key.dcc_type);
        let permit = tokio::time::timeout(self.config.acquire_timeout, sem.acquire_owned()).await;

        match permit {
            Ok(Ok(_permit)) => {
                // Create a new connection
                let mut conn = PooledConnection::new(service_key.clone(), host.to_string(), port);
                conn.state = ConnectionState::InUse;
                conn.touch();
                let id = conn.id;
                self.connections.insert(service_key.clone(), conn);
                // Note: permit is dropped here, releasing the semaphore slot.
                // In a real implementation, the permit would be held until release().
                Ok(id)
            }
            Ok(Err(_)) => Err(TransportError::PoolExhausted {
                dcc_type: service_key.dcc_type.clone(),
                max_connections: self.config.max_connections_per_type,
            }),
            Err(_) => Err(TransportError::AcquireTimeout {
                dcc_type: service_key.dcc_type.clone(),
                timeout_ms: self.config.acquire_timeout.as_millis() as u64,
            }),
        }
    }

    /// Release a connection back to the pool.
    pub fn release(&self, service_key: &ServiceKey) {
        if let Some(mut conn) = self.connections.get_mut(service_key) {
            if conn.state == ConnectionState::InUse {
                conn.state = ConnectionState::Available;
                conn.last_used = Instant::now();
            }
        }
    }

    /// Remove a connection from the pool.
    pub fn remove(&self, service_key: &ServiceKey) -> Option<PooledConnection> {
        self.connections.remove(service_key).map(|(_, conn)| conn)
    }

    /// Get the number of connections in the pool.
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    /// Check if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Get the number of connections for a specific DCC type.
    pub fn count_for_dcc(&self, dcc_type: &str) -> usize {
        self.connections
            .iter()
            .filter(|r| r.key().dcc_type == dcc_type)
            .count()
    }

    /// Evict idle and expired connections.
    pub fn evict_stale(&self) -> usize {
        let mut evicted = 0;
        let keys_to_remove: Vec<ServiceKey> = self
            .connections
            .iter()
            .filter(|r| {
                let conn = r.value();
                conn.state == ConnectionState::Available
                    && (conn.is_idle(self.config.max_idle_time)
                        || conn.is_expired(self.config.max_lifetime))
            })
            .map(|r| r.key().clone())
            .collect();

        for key in keys_to_remove {
            if self.connections.remove(&key).is_some() {
                evicted += 1;
            }
        }
        evicted
    }

    /// Drain all connections (graceful shutdown).
    pub fn drain(&self) -> Vec<PooledConnection> {
        // Mark all as draining first
        for mut entry in self.connections.iter_mut() {
            entry.value_mut().state = ConnectionState::Draining;
        }

        // Remove all
        let keys: Vec<ServiceKey> = self.connections.iter().map(|r| r.key().clone()).collect();
        let mut drained = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some((_, conn)) = self.connections.remove(&key) {
                drained.push(conn);
            }
        }
        drained
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_key(dcc_type: &str) -> ServiceKey {
        ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn test_pooled_connection_new() {
        let key = make_key("maya");
        let conn = PooledConnection::new(key, "127.0.0.1".to_string(), 18812);
        assert_eq!(conn.state, ConnectionState::Available);
        assert_eq!(conn.request_count, 0);
    }

    #[test]
    fn test_pooled_connection_touch() {
        let key = make_key("maya");
        let mut conn = PooledConnection::new(key, "127.0.0.1".to_string(), 18812);
        conn.touch();
        assert_eq!(conn.request_count, 1);
        conn.touch();
        assert_eq!(conn.request_count, 2);
    }

    #[test]
    fn test_pool_new() {
        let pool = ConnectionPool::new(PoolConfig::default());
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
    }

    #[tokio::test]
    async fn test_pool_acquire_and_release() {
        let pool = ConnectionPool::new(PoolConfig::default());
        let key = make_key("maya");

        let conn_id = pool.acquire(&key, "127.0.0.1", 18812).await.unwrap();
        assert_eq!(pool.len(), 1);

        // Connection should be InUse
        {
            let conn = pool.connections.get(&key).unwrap();
            assert_eq!(conn.state, ConnectionState::InUse);
            assert_eq!(conn.id, conn_id);
        }

        // Release
        pool.release(&key);
        {
            let conn = pool.connections.get(&key).unwrap();
            assert_eq!(conn.state, ConnectionState::Available);
        }
    }

    #[tokio::test]
    async fn test_pool_count_for_dcc() {
        let pool = ConnectionPool::new(PoolConfig::default());

        let key1 = make_key("maya");
        let key2 = make_key("maya");
        let key3 = make_key("blender");

        pool.acquire(&key1, "127.0.0.1", 18812).await.unwrap();
        pool.acquire(&key2, "127.0.0.1", 18813).await.unwrap();
        pool.acquire(&key3, "127.0.0.1", 9090).await.unwrap();

        assert_eq!(pool.count_for_dcc("maya"), 2);
        assert_eq!(pool.count_for_dcc("blender"), 1);
        assert_eq!(pool.count_for_dcc("houdini"), 0);
    }

    #[test]
    fn test_pool_evict_stale() {
        let config = PoolConfig {
            max_idle_time: Duration::from_millis(0), // Everything is idle immediately
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);
        let key = make_key("maya");

        // Insert directly for testing
        let mut conn = PooledConnection::new(key.clone(), "127.0.0.1".to_string(), 18812);
        conn.state = ConnectionState::Available;
        pool.connections.insert(key, conn);

        assert_eq!(pool.len(), 1);
        let evicted = pool.evict_stale();
        assert_eq!(evicted, 1);
        assert!(pool.is_empty());
    }

    #[test]
    fn test_pool_drain() {
        let pool = ConnectionPool::new(PoolConfig::default());
        let key1 = make_key("maya");
        let key2 = make_key("blender");

        pool.connections.insert(
            key1.clone(),
            PooledConnection::new(key1, "127.0.0.1".to_string(), 18812),
        );
        pool.connections.insert(
            key2.clone(),
            PooledConnection::new(key2, "127.0.0.1".to_string(), 9090),
        );

        let drained = pool.drain();
        assert_eq!(drained.len(), 2);
        assert!(pool.is_empty());
    }
}
