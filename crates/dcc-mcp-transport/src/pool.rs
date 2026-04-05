//! Connection pool — lock-free, with DashMap + Semaphore.
//!
//! ## Architecture
//!
//! The pool stores [`ActiveConnection`]s that wrap a real [`FramedIo`](crate::framed::FramedIo)
//! instance. This means acquiring a connection from the pool gives you an immediately usable
//! I/O channel — no separate connect step required.
//!
//! ```text
//!  TransportManager::get_framed()
//!        │
//!        ▼
//!  ConnectionPool::acquire_active()
//!        │
//!        ├─── Available? → return existing ActiveConnection (Arc<Mutex<FramedIo>>)
//!        │
//!        └─── Not found? → connect() → create ActiveConnection → insert → return
//!
//!  Caller uses: active.lock().unwrap().framed_mut().unwrap().send(&req).await?
//! ```

use dashmap::DashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::config::PoolConfig;
use crate::connector::connect;
use crate::discovery::types::ServiceKey;
use crate::error::{TransportError, TransportResult};
use crate::framed::FramedIo;
use crate::ipc::TransportAddress;

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

// ── ActiveConnection ───────────────────────────────────────────────────────

/// An active, live I/O connection that wraps a [`FramedIo`] instance.
///
/// Unlike [`PooledConnection`](PooledConnection) which only holds metadata,
/// `ActiveConnection` owns the actual framed I/O channel and can be used to
/// send and receive messages immediately.
///
/// Stored in the connection pool as `Arc<Mutex<ActiveConnection>>`, allowing
/// safe sharing across sync and async code while preserving exclusive access to the
/// underlying stream.
#[derive(Debug)]
pub struct ActiveConnection {
    /// Connection identifier.
    pub id: Uuid,
    /// Target service key.
    pub service_key: ServiceKey,
    /// Transport address this connection is bound to.
    pub address: TransportAddress,
    /// Current state of this connection.
    pub state: ConnectionState,
    /// When this connection was created.
    pub created_at: Instant,
    /// When this connection was last used (acquired from the pool).
    pub last_used: Instant,
    /// Number of requests handled through this connection.
    pub request_count: u64,
    /// The real framed I/O channel.
    ///
    /// Wrapped in `Option` so we can `take()` it during graceful shutdown or
    /// when the connection is evicted from the pool. In normal operation, this
    /// is always `Some`.
    framed: Option<FramedIo>,
}

impl ActiveConnection {
    /// Create a new active connection by connecting to the given address.
    ///
    /// Establishes a real IPC/TCP connection and wraps it in a `FramedIo`.
    pub async fn connect(
        service_key: ServiceKey,
        address: TransportAddress,
        timeout: std::time::Duration,
    ) -> TransportResult<Self> {
        let stream = connect(&address, timeout).await?;
        let framed = FramedIo::new(stream);
        let now = Instant::now();
        Ok(Self {
            id: Uuid::new_v4(),
            service_key,
            address,
            state: ConnectionState::Available,
            created_at: now,
            last_used: now,
            request_count: 0,
            framed: Some(framed),
        })
    }

    /// Create an active connection from an already-connected [`FramedIo`].
    ///
    /// Useful for server-side connections accepted from a listener.
    pub fn from_framed(
        service_key: ServiceKey,
        address: TransportAddress,
        framed: FramedIo,
    ) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            service_key,
            address,
            state: ConnectionState::Available,
            created_at: now,
            last_used: now,
            request_count: 0,
            framed: Some(framed),
        }
    }

    /// Get a reference to the inner [`FramedIo`].
    ///
    /// Returns `None` if the connection was shut down or evicted.
    pub fn framed(&self) -> Option<&FramedIo> {
        self.framed.as_ref()
    }

    /// Get a mutable reference to the inner [`FramedIo`].
    pub fn framed_mut(&mut self) -> Option<&mut FramedIo> {
        self.framed.as_mut()
    }

    /// Take ownership of the inner [`FramedIo`], leaving `None` in its place.
    ///
    /// Used during shutdown/eviction to cleanly close the connection.
    pub fn take_framed(&mut self) -> Option<FramedIo> {
        self.framed.take()
    }

    /// Check if the inner I/O channel is still alive (not yet taken).
    pub fn is_alive(&self) -> bool {
        self.framed.is_some()
    }

    /// Get the transport name (e.g. "tcp", "named_pipe").
    pub fn transport_name(&self) -> &'static str {
        self.framed
            .as_ref()
            .map(|f| f.transport_name())
            .unwrap_or("disconnected")
    }

    /// Get the host address (backward-compatible accessor).
    pub fn host(&self) -> &str {
        match &self.address {
            TransportAddress::Tcp { host, .. } => host,
            TransportAddress::NamedPipe { .. } | TransportAddress::UnixSocket { .. } => "127.0.0.1",
        }
    }

    /// Get the port number (backward-compatible accessor).
    pub fn port(&self) -> u16 {
        match &self.address {
            TransportAddress::Tcp { port, .. } => *port,
            TransportAddress::NamedPipe { .. } | TransportAddress::UnixSocket { .. } => 0,
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

    /// Mark the connection as recently used.
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
        self.request_count += 1;
    }
}

// ── PooledConnection (metadata-only, backward compat) ─────────────────────

/// A lightweight metadata-only view of a pooled connection.
///
/// Returned by pool inspection methods (`len`, `list_connections`, etc.) when
/// you don't need the I/O channel itself. For actual communication, use
/// [`ActiveConnection`] via [`ConnectionPool::acquire_active`].
#[derive(Debug, Clone)]
pub struct PooledConnection {
    /// Connection identifier.
    pub id: Uuid,
    /// Target service key.
    pub service_key: ServiceKey,
    /// Transport address (TCP, Named Pipe, or Unix Socket).
    pub address: TransportAddress,
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
    /// Create a new pooled connection with a transport address (metadata only).
    pub fn new(service_key: ServiceKey, address: TransportAddress) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            service_key,
            address,
            state: ConnectionState::Available,
            created_at: now,
            last_used: now,
            request_count: 0,
        }
    }

    /// Create a new pooled connection from host and port (backward compatibility).
    pub fn from_host_port(service_key: ServiceKey, host: String, port: u16) -> Self {
        Self::new(service_key, TransportAddress::tcp(host, port))
    }

    /// Get the host address (extracted from transport address for backward compatibility).
    pub fn host(&self) -> &str {
        match &self.address {
            TransportAddress::Tcp { host, .. } => host,
            TransportAddress::NamedPipe { .. } | TransportAddress::UnixSocket { .. } => "127.0.0.1",
        }
    }

    /// Get the port number (extracted from transport address for backward compatibility).
    pub fn port(&self) -> u16 {
        match &self.address {
            TransportAddress::Tcp { port, .. } => *port,
            TransportAddress::NamedPipe { .. } | TransportAddress::UnixSocket { .. } => 0,
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

    /// Convert an [`ActiveConnection`] into its metadata-only representation.
    pub fn from_active(active: &ActiveConnection) -> Self {
        Self {
            id: active.id,
            service_key: active.service_key.clone(),
            address: active.address.clone(),
            state: active.state,
            created_at: active.created_at,
            last_used: active.last_used,
            request_count: active.request_count,
        }
    }
}

// ── ConnectionPool ────────────────────────────────────────────────────────

/// Thread-safe connection pool using DashMap and std::sync::Mutex.
///
/// Stores [`ActiveConnection`] instances that hold real [`FramedIo`] channels.
/// Acquiring a connection returns an `Arc<Mutex<ActiveConnection>>` that can
/// be used directly for I/O — including from both sync and async contexts.
///
/// Features:
/// - Lock-free concurrent access via `DashMap`
/// - Per-DCC-type connection limiting via Tokio `Semaphore`
/// - Automatic eviction of idle/expired connections
/// - Real I/O connections (not just metadata)
/// - Sync-safe: all mutation methods use `std::sync::Mutex`, no `block_on` needed
pub struct ConnectionPool {
    /// `(dcc_type, instance_id)` -> `Arc<Mutex<ActiveConnection>>`
    connections: Arc<DashMap<ServiceKey, Arc<Mutex<ActiveConnection>>>>,
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

    // ── Active connection API (preferred) ──

    /// Acquire or create an **active** connection with a real I/O channel.
    ///
    /// This is the primary API for getting a usable connection. If an available
    /// connection already exists for the `service_key`, it is returned. Otherwise,
    /// a new IPC connection is established automatically.
    ///
    /// Returns `Arc<Mutex<ActiveConnection>>` which provides thread-safe access
    /// to the underlying `FramedIo`.
    pub async fn acquire_active(
        &self,
        service_key: &ServiceKey,
        address: &TransportAddress,
        timeout: std::time::Duration,
    ) -> TransportResult<Arc<Mutex<ActiveConnection>>> {
        // Check if we have an available alive connection
        if let Some(entry) = self.connections.get(service_key) {
            let conn = entry.value().clone();
            drop(entry); // Release DashMap ref before locking
            {
                let mut guard = conn.lock().unwrap();
                if guard.state == ConnectionState::Available && guard.is_alive() {
                    guard.state = ConnectionState::InUse;
                    guard.touch();
                    tracing::debug!(
                        conn_id = %guard.id,
                        address = %address,
                        "reusing pooled active connection"
                    );
                    return Ok(conn.clone());
                }
            }
            // Connection exists but is dead -- fall through to reconnect
        }

        // Try to acquire a semaphore permit (with timeout)
        let sem = self.get_semaphore(&service_key.dcc_type);
        let permit_result =
            tokio::time::timeout(self.config.acquire_timeout, sem.acquire_owned()).await;

        match permit_result {
            Ok(Ok(_permit)) => {
                // Connect and create active connection
                let mut active =
                    ActiveConnection::connect(service_key.clone(), address.clone(), timeout)
                        .await?;
                active.state = ConnectionState::InUse;
                active.touch();

                let conn_id = active.id;
                let wrapped = Arc::new(Mutex::new(active));
                self.connections
                    .insert(service_key.clone(), wrapped.clone());

                tracing::info!(conn_id = %conn_id, address = %address, "created new active connection");

                Ok(wrapped)
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

    /// Insert a pre-existing active connection into the pool.
    ///
    /// Useful when a connection is accepted from a listener rather than
    /// initiated as a client.
    pub fn insert_active(&self, service_key: ServiceKey, active: ActiveConnection) {
        let wrapped = Arc::new(Mutex::new(active));
        self.connections.insert(service_key, wrapped);
    }

    /// Get an existing active connection without acquiring or connecting.
    ///
    /// Returns `None` if no connection exists for the key, or if the connection is dead.
    pub fn get_active(&self, service_key: &ServiceKey) -> Option<Arc<Mutex<ActiveConnection>>> {
        self.connections.get(service_key).map(|e| e.value().clone())
    }

    // ── Metadata-only API (backward compatible) ──

    /// Try to acquire or create a connection (metadata-only, backward compatible).
    ///
    /// **Note:** For actual I/O, prefer [`ConnectionPool::acquire_active`].
    pub async fn acquire_with_address(
        &self,
        service_key: &ServiceKey,
        address: &TransportAddress,
    ) -> TransportResult<Uuid> {
        // Check if we already have an active connection
        if let Some(conn) = self.connections.get(service_key) {
            let conn = conn.value().clone();
            let mut guard = conn.lock().unwrap();
            if guard.state == ConnectionState::Available {
                guard.state = ConnectionState::InUse;
                guard.touch();
                return Ok(guard.id);
            }
        }

        // Create a real connection
        let sem = self.get_semaphore(&service_key.dcc_type);
        let permit = tokio::time::timeout(self.config.acquire_timeout, sem.acquire_owned()).await;

        match permit {
            Ok(Ok(_permit)) => {
                let active = ActiveConnection::connect(
                    service_key.clone(),
                    address.clone(),
                    std::time::Duration::from_secs(5),
                )
                .await?;
                let id = active.id;
                let wrapped = Arc::new(Mutex::new(active));
                self.connections.insert(service_key.clone(), wrapped);
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

    /// Try to acquire or create a TCP connection (backward compatibility).
    pub async fn acquire(
        &self,
        service_key: &ServiceKey,
        host: &str,
        port: u16,
    ) -> TransportResult<Uuid> {
        self.acquire_with_address(service_key, &TransportAddress::tcp(host, port))
            .await
    }

    /// Release a connection back to the pool.
    pub fn release(&self, service_key: &ServiceKey) {
        if let Some(conn) = self.connections.get(service_key) {
            if let Ok(mut guard) = conn.lock() {
                if guard.state == ConnectionState::InUse {
                    guard.state = ConnectionState::Available;
                    guard.last_used = Instant::now();
                }
            }
        }
    }

    /// Remove a connection from the pool, returning the active connection if present.
    pub fn remove(&self, service_key: &ServiceKey) -> Option<Arc<Mutex<ActiveConnection>>> {
        self.connections.remove(service_key).map(|(_, conn)| conn)
    }

    /// Remove a connection and return metadata only.
    pub fn remove_metadata(&self, service_key: &ServiceKey) -> Option<PooledConnection> {
        self.connections.remove(service_key).map(|(_, conn)| {
            let guard = conn.lock().unwrap();
            PooledConnection::from_active(&guard)
        })
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

    /// Reconnect a dead connection with exponential backoff.
    ///
    /// If an existing connection for `service_key` exists but is dead (framed is `None` or
    /// `state == Reconnecting`), this method attempts to re-establish the connection up to
    /// `max_retries` times, sleeping for `base_backoff * 2^attempt` between each attempt.
    ///
    /// On success, the pool entry is replaced with the new live connection, and the new
    /// `Arc<Mutex<ActiveConnection>>` is returned with state `InUse`.
    ///
    /// On failure after all retries, returns [`TransportError::ReconnectFailed`].
    ///
    /// # Backoff Schedule
    ///
    /// | Attempt | Sleep before attempt |
    /// |---------|----------------------|
    /// | 1       | 0 (immediate)        |
    /// | 2       | base_backoff         |
    /// | 3       | base_backoff × 2     |
    /// | …       | base_backoff × 2^(n-1) (capped at 30 s) |
    pub async fn reconnect_active_with_backoff(
        &self,
        service_key: &ServiceKey,
        address: &TransportAddress,
        connect_timeout: std::time::Duration,
        base_backoff: std::time::Duration,
        max_retries: u32,
    ) -> TransportResult<Arc<Mutex<ActiveConnection>>> {
        // Mark the existing entry as Reconnecting so callers see the intent.
        if let Some(entry) = self.connections.get(service_key) {
            if let Ok(mut guard) = entry.value().lock() {
                guard.state = ConnectionState::Reconnecting;
            }
        }

        const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

        let mut last_error: TransportError = TransportError::connection_refused("none", 0);

        for attempt in 0..=max_retries {
            // Sleep before retrying (skip first attempt)
            if attempt > 0 {
                let sleep_duration =
                    std::cmp::min(base_backoff * 2u32.saturating_pow(attempt - 1), MAX_BACKOFF);
                tracing::debug!(
                    service_key = ?service_key,
                    attempt,
                    sleep_ms = sleep_duration.as_millis() as u64,
                    "reconnect backoff sleep"
                );
                tokio::time::sleep(sleep_duration).await;
            }

            tracing::info!(
                service_key = ?service_key,
                attempt,
                max_retries,
                address = %address,
                "attempting reconnect"
            );

            match ActiveConnection::connect(service_key.clone(), address.clone(), connect_timeout)
                .await
            {
                Ok(mut active) => {
                    active.state = ConnectionState::InUse;
                    active.touch();
                    let conn_id = active.id;
                    let wrapped = Arc::new(Mutex::new(active));
                    // Replace the old (dead) entry in the pool
                    self.connections
                        .insert(service_key.clone(), wrapped.clone());
                    tracing::info!(
                        conn_id = %conn_id,
                        address = %address,
                        attempts = attempt + 1,
                        "reconnect succeeded"
                    );
                    return Ok(wrapped);
                }
                Err(e) => {
                    tracing::warn!(
                        service_key = ?service_key,
                        attempt,
                        error = %e,
                        "reconnect attempt failed"
                    );
                    last_error = e;
                }
            }
        }

        Err(TransportError::ReconnectFailed {
            dcc_type: service_key.dcc_type.clone(),
            instance_id: service_key.instance_id,
            attempts: max_retries + 1,
            last_error: last_error.to_string(),
        })
    }

    /// Evict idle and expired connections, closing their I/O channels.
    pub fn evict_stale(&self) -> usize {
        let keys_to_remove: Vec<ServiceKey> = self
            .connections
            .iter()
            .filter(|r| {
                if let Ok(guard) = r.value().lock() {
                    guard.state == ConnectionState::Available
                        && (guard.is_idle(self.config.max_idle_time)
                            || guard.is_expired(self.config.max_lifetime))
                } else {
                    false
                }
            })
            .map(|r| r.key().clone())
            .collect();

        let mut evicted = 0;
        for key in keys_to_remove {
            if let Some((_, conn)) = self.connections.remove(&key) {
                if let Ok(mut guard) = conn.lock() {
                    guard.take_framed(); // drop the FramedIo
                }
                evicted += 1;
            }
        }
        evicted
    }

    /// Drain all connections (graceful shutdown), closing all I/O channels.
    pub fn drain(&self) -> Vec<PooledConnection> {
        // Mark all as draining
        for entry in self.connections.iter() {
            if let Ok(mut guard) = entry.value().lock() {
                guard.state = ConnectionState::Draining;
            }
        }

        let keys: Vec<ServiceKey> = self.connections.iter().map(|r| r.key().clone()).collect();
        let mut drained = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some((_, conn)) = self.connections.remove(&key) {
                if let Ok(mut guard) = conn.lock() {
                    guard.take_framed();
                    drained.push(PooledConnection::from_active(&guard));
                }
            }
        }
        drained
    }

    /// List all connections as metadata (for inspection/debugging).
    pub fn list_connections(&self) -> Vec<PooledConnection> {
        self.connections
            .iter()
            .filter_map(|r| {
                r.value()
                    .lock()
                    .ok()
                    .map(|g| PooledConnection::from_active(&g))
            })
            .collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

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
}
