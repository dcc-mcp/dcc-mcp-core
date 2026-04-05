//! TransportManager — main entry point for the transport layer.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use uuid::Uuid;

use crate::config::TransportConfig;
use crate::discovery::ServiceRegistry;
use crate::discovery::types::{ServiceEntry, ServiceKey, ServiceStatus};
use crate::error::{TransportError, TransportResult};
use crate::framed::FramedIo;
use crate::ipc::TransportAddress;
use crate::listener::IpcListener;
use crate::pool::{ActiveConnection, ConnectionPool};
use crate::routing::{InstanceRouter, RoutingStrategy};
use crate::session::{Session, SessionManager};

/// Main entry point for the transport layer.
///
/// Manages the connection pool, service registry, session manager,
/// instance router, and provides a unified API for communicating with DCC applications.
pub struct TransportManager {
    /// Connection pool.
    pool: ConnectionPool,
    /// Service registry.
    registry: ServiceRegistry,
    /// Session manager.
    sessions: SessionManager,
    /// Instance router for smart DCC instance selection.
    router: InstanceRouter,
    /// Transport configuration.
    config: TransportConfig,
    /// Whether the transport is shut down.
    shutdown: Arc<AtomicBool>,
}

impl TransportManager {
    /// Create a new transport manager with file-based service discovery.
    pub fn new(config: TransportConfig, registry_dir: impl Into<PathBuf>) -> TransportResult<Self> {
        let registry = ServiceRegistry::file_based(registry_dir)?;
        let sessions = SessionManager::new(config.session.clone());
        Ok(Self {
            pool: ConnectionPool::new(config.pool.clone()),
            registry,
            sessions,
            router: InstanceRouter::default(),
            config,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create a transport manager with a custom service registry.
    pub fn with_registry(config: TransportConfig, registry: ServiceRegistry) -> Self {
        let sessions = SessionManager::new(config.session.clone());
        Self {
            pool: ConnectionPool::new(config.pool.clone()),
            registry,
            sessions,
            router: InstanceRouter::default(),
            config,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the instance router for configuration.
    pub fn router(&self) -> &InstanceRouter {
        &self.router
    }

    /// Get a mutable reference to the instance router.
    pub fn router_mut(&mut self) -> &mut InstanceRouter {
        &mut self.router
    }

    // ── Service Discovery ──

    /// Register a DCC service instance.
    pub fn register_service(&self, entry: ServiceEntry) -> TransportResult<()> {
        self.check_shutdown()?;
        self.registry.register(entry)
    }

    /// Deregister a DCC service instance.
    pub fn deregister_service(&self, key: &ServiceKey) -> TransportResult<Option<ServiceEntry>> {
        self.check_shutdown()?;
        // Close associated session and remove from pool
        if let Some(session) = self.sessions.get_by_service(key) {
            let _ = self.sessions.close(&session.id);
        }
        self.pool.remove(key);
        self.registry.deregister(key)
    }

    /// List all instances for a given DCC type.
    pub fn list_instances(&self, dcc_type: &str) -> Vec<ServiceEntry> {
        self.registry.list_instances(dcc_type)
    }

    /// List all registered services.
    pub fn list_all_services(&self) -> Vec<ServiceEntry> {
        self.registry.list_all()
    }

    /// Get a specific service entry.
    pub fn get_service(&self, key: &ServiceKey) -> Option<ServiceEntry> {
        self.registry.get(key)
    }

    /// Update heartbeat for a service.
    pub fn heartbeat(&self, key: &ServiceKey) -> TransportResult<bool> {
        self.registry.heartbeat(key)
    }

    /// Update the status of a registered service.
    pub fn update_service_status(
        &self,
        key: &ServiceKey,
        status: ServiceStatus,
    ) -> TransportResult<bool> {
        self.check_shutdown()?;
        self.registry.update_status(key, status)
    }

    // ── Session Management ──

    /// Get or create a session for a DCC instance (lazy creation).
    ///
    /// If no instance_id is specified, uses the router to select an instance.
    pub fn get_or_create_session(
        &self,
        dcc_type: &str,
        instance_id: Option<Uuid>,
    ) -> TransportResult<Uuid> {
        self.check_shutdown()?;

        let entry = self.resolve_instance(dcc_type, instance_id, None, None)?;
        let address = entry.effective_address();
        self.sessions
            .get_or_create_with_address(&entry.dcc_type, entry.instance_id, &address)
    }

    /// Get or create a session with routing strategy and hint.
    ///
    /// This is the advanced API that supports smart instance selection.
    pub fn get_or_create_session_routed(
        &self,
        dcc_type: &str,
        strategy: Option<RoutingStrategy>,
        hint: Option<&str>,
    ) -> TransportResult<Uuid> {
        self.check_shutdown()?;

        let entry = self.resolve_instance(dcc_type, None, strategy, hint)?;
        let address = entry.effective_address();
        self.sessions
            .get_or_create_with_address(&entry.dcc_type, entry.instance_id, &address)
    }

    /// Get session info by ID.
    pub fn get_session(&self, session_id: &Uuid) -> Option<Session> {
        self.sessions.get(session_id)
    }

    /// Get session for a service key.
    pub fn get_session_by_service(&self, key: &ServiceKey) -> Option<Session> {
        self.sessions.get_by_service(key)
    }

    /// Record a successful request on a session.
    pub fn record_request_success(&self, session_id: &Uuid, latency: Duration) {
        self.sessions.record_success(session_id, latency);
    }

    /// Record a failed request on a session.
    pub fn record_request_error(&self, session_id: &Uuid, latency: Duration, error: &str) {
        self.sessions.record_error(session_id, latency, error);
    }

    /// Begin reconnection for a session. Returns the backoff duration.
    pub fn begin_reconnect(&self, session_id: &Uuid) -> TransportResult<Duration> {
        self.sessions.begin_reconnect(session_id)
    }

    /// Mark reconnection as successful.
    pub fn reconnect_success(&self, session_id: &Uuid) -> TransportResult<()> {
        self.sessions.reconnect_success(session_id)
    }

    /// Close a session.
    pub fn close_session(&self, session_id: &Uuid) -> TransportResult<Option<Session>> {
        self.sessions.close(session_id)
    }

    /// List all active sessions.
    pub fn list_sessions(&self) -> Vec<Session> {
        self.sessions.list_all()
    }

    /// List sessions for a specific DCC type.
    pub fn list_sessions_for_dcc(&self, dcc_type: &str) -> Vec<Session> {
        self.sessions.list_for_dcc(dcc_type)
    }

    /// Get session count.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    // ── Connection Pool ──

    /// Acquire a connection to a service instance.
    ///
    /// If no instance_id is specified, uses the router to select an instance.
    pub async fn acquire_connection(
        &self,
        dcc_type: &str,
        instance_id: Option<Uuid>,
    ) -> TransportResult<Uuid> {
        self.check_shutdown()?;

        let entry = self.resolve_instance(dcc_type, instance_id, None, None)?;
        let key = entry.key();
        let address = entry.effective_address();
        self.pool.acquire_with_address(&key, &address).await
    }

    /// Acquire a connection with routing strategy and hint.
    pub async fn acquire_connection_routed(
        &self,
        dcc_type: &str,
        strategy: Option<RoutingStrategy>,
        hint: Option<&str>,
    ) -> TransportResult<Uuid> {
        self.check_shutdown()?;

        let entry = self.resolve_instance(dcc_type, None, strategy, hint)?;
        let key = entry.key();
        let address = entry.effective_address();
        self.pool.acquire_with_address(&key, &address).await
    }

    /// Release a connection back to the pool.
    pub fn release_connection(&self, key: &ServiceKey) {
        self.pool.release(key);
    }

    /// Accept a single incoming connection from a listener and insert it into the pool.
    ///
    /// This is the server-side counterpart to `acquire_connection`. A DCC plugin
    /// calls this after accepting a connection from an [`IpcListener`]:
    ///
    /// ```ignore
    /// let listener = IpcListener::bind(&addr).await?;
    /// let key = ServiceKey { dcc_type: "maya".into(), instance_id: Uuid::new_v4() };
    /// let conn_id = manager.accept_into_pool(&listener, key, addr).await?;
    /// ```
    ///
    /// The accepted stream is wrapped in a [`FramedIo`] and stored as an
    /// [`ActiveConnection`] in the pool, ready for bidirectional message exchange.
    pub async fn accept_into_pool(
        &self,
        listener: &IpcListener,
        service_key: ServiceKey,
        address: TransportAddress,
    ) -> TransportResult<Uuid> {
        self.check_shutdown()?;

        let stream = listener.accept().await?;
        let framed = FramedIo::new(stream);
        let active = ActiveConnection::from_framed(service_key.clone(), address, framed);
        let id = active.id;
        self.pool.insert_active(service_key, active);
        Ok(id)
    }

    /// Spawn a background task that continuously accepts connections from the listener
    /// and inserts them into the pool.
    ///
    /// Returns a [`tokio::task::JoinHandle`] for the accept loop. Call `.abort()` on it
    /// to stop accepting new connections.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let addr = TransportAddress::tcp("127.0.0.1", 0);
    /// let listener = IpcListener::bind(&addr).await?;
    /// let key = ServiceKey { dcc_type: "maya".into(), instance_id: Uuid::new_v4() };
    /// let handle = manager.serve(listener, key);
    /// // ... later ...
    /// handle.abort();
    /// ```
    pub fn serve(
        self: Arc<Self>,
        listener: IpcListener,
        service_key: ServiceKey,
    ) -> tokio::task::JoinHandle<()> {
        let manager = self;
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok(stream) => {
                        let addr = listener
                            .local_address()
                            .unwrap_or_else(|_| TransportAddress::tcp("127.0.0.1", 0));
                        let framed = FramedIo::new(stream);
                        let active =
                            ActiveConnection::from_framed(service_key.clone(), addr, framed);
                        manager.pool.insert_active(service_key.clone(), active);
                        tracing::debug!(dcc_type = %service_key.dcc_type, "accepted incoming connection");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "accept error; stopping serve loop");
                        break;
                    }
                }

                if manager.is_shutdown() {
                    break;
                }
            }
        })
    }

    /// Get pool statistics.
    pub fn pool_size(&self) -> usize {
        self.pool.len()
    }

    /// Get the number of connections for a specific DCC type.
    pub fn pool_count_for_dcc(&self, dcc_type: &str) -> usize {
        self.pool.count_for_dcc(dcc_type)
    }

    /// Get the underlying active connection from the pool.
    ///
    /// Returns `None` if no connection exists for the given key. The caller can lock the
    /// returned `Arc<Mutex<ActiveConnection>>` and call `framed_mut()` to send/receive messages.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let arc = manager.get_active_connection(&key).unwrap();
    /// let mut guard = arc.lock().unwrap();
    /// let framed = guard.framed_mut().unwrap();
    /// framed.send(&my_request).await?;
    /// ```
    pub fn get_active_connection(
        &self,
        key: &ServiceKey,
    ) -> Option<Arc<std::sync::Mutex<ActiveConnection>>> {
        self.pool.get_active(key)
    }

    /// Reconnect an active connection with exponential backoff.
    ///
    /// Combines the session-layer backoff configuration with the pool-layer reconnection
    /// logic. The manager will:
    ///
    /// 1. Look up the session for `service_key` and use its `reconnect_max_retries` /
    ///    `reconnect_backoff_base` settings from [`SessionConfig`].
    /// 2. Delegate to [`ConnectionPool::reconnect_active_with_backoff`] using those settings.
    /// 3. On success, record the session reconnect success via [`SessionManager::reconnect_success`].
    ///
    /// Falls back to default backoff settings (3 retries, 1 s base) when no session exists.
    ///
    /// # Arguments
    ///
    /// * `service_key` — identifies the DCC instance to reconnect.
    /// * `address` — the transport address to reconnect to.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::ReconnectFailed`] if all retries are exhausted,
    /// or the usual connection errors on transport failure.
    pub async fn reconnect_active(
        &self,
        service_key: &ServiceKey,
        address: &TransportAddress,
    ) -> TransportResult<Arc<std::sync::Mutex<ActiveConnection>>> {
        // Backoff settings always come from the transport config's session section.
        let max_retries = self.config.session.reconnect_max_retries;
        let backoff_base = self.config.session.reconnect_backoff_base;

        let result = self
            .pool
            .reconnect_active_with_backoff(
                service_key,
                address,
                self.config.connect_timeout,
                backoff_base,
                max_retries,
            )
            .await;

        // On success, update the session state
        if result.is_ok() {
            if let Some(session) = self.sessions.get_by_service(service_key) {
                let _ = self.sessions.reconnect_success(&session.id);
            }
        }

        result
    }

    /// Bind an [`IpcListener`] using the `listen_address` from [`TransportConfig`].
    ///
    /// Returns an error if `config.listen_address` is not set or the address is invalid.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut config = TransportConfig::default();
    /// config.listen_address = Some("tcp://127.0.0.1:9000".to_string());
    /// let manager = TransportManager::new(config, dir.path())?;
    /// let listener = manager.listen().await?;
    /// ```
    pub async fn listen(&self) -> TransportResult<IpcListener> {
        let addr_str =
            self.config.listen_address.as_deref().ok_or_else(|| {
                TransportError::Internal("listen_address not configured".to_string())
            })?;

        let addr = TransportAddress::parse(addr_str).map_err(|e| {
            TransportError::Internal(format!("invalid listen_address '{addr_str}': {e}"))
        })?;

        IpcListener::bind(&addr).await
    }

    // ── Lifecycle ──

    /// Get the transport configuration.
    pub fn config(&self) -> &TransportConfig {
        &self.config
    }

    /// Cleanup stale services, idle sessions, and evict idle connections.
    pub fn cleanup(&self) -> TransportResult<(usize, usize, usize)> {
        let heartbeat_timeout = self.config.heartbeat_interval * 3;
        let stale_services = self.registry.cleanup_stale(heartbeat_timeout)?;
        let idle_sessions = self.sessions.mark_idle_sessions();
        let expired_sessions = self.sessions.close_expired();
        let evicted_connections = self.pool.evict_stale();
        Ok((
            stale_services,
            idle_sessions + expired_sessions,
            evicted_connections,
        ))
    }

    /// Gracefully shut down the transport.
    pub fn shutdown(&self) -> (Vec<Session>, Vec<crate::pool::PooledConnection>) {
        self.shutdown.store(true, Ordering::SeqCst);
        tracing::info!("transport shutting down");
        let sessions = self.sessions.shutdown_all();
        let connections = self.pool.drain();
        (sessions, connections)
    }

    /// Check if the transport is shut down.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    // ── Internal ──

    fn check_shutdown(&self) -> TransportResult<()> {
        if self.is_shutdown() {
            Err(TransportError::Shutdown)
        } else {
            Ok(())
        }
    }

    fn resolve_instance(
        &self,
        dcc_type: &str,
        instance_id: Option<Uuid>,
        strategy: Option<RoutingStrategy>,
        hint: Option<&str>,
    ) -> TransportResult<ServiceEntry> {
        // If a specific instance_id is given, look it up directly
        if let Some(id) = instance_id {
            let key = ServiceKey {
                dcc_type: dcc_type.to_string(),
                instance_id: id,
            };
            return self
                .registry
                .get(&key)
                .ok_or_else(|| TransportError::ServiceNotFound {
                    dcc_type: dcc_type.to_string(),
                    instance_id: id.to_string(),
                });
        }

        // Use the router to select an instance
        let instances = self.registry.list_instances(dcc_type);
        if instances.is_empty() {
            return Err(TransportError::ServiceNotFound {
                dcc_type: dcc_type.to_string(),
                instance_id: "any".to_string(),
            });
        }

        self.router.select(&instances, strategy, hint).map_err(|_| {
            TransportError::ServiceNotFound {
                dcc_type: dcc_type.to_string(),
                instance_id: format!(
                    "strategy={}, hint={:?}",
                    strategy.unwrap_or(self.router.default_strategy()),
                    hint
                ),
            }
        })
    }
}

#[cfg(test)]
mod tests {
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
        let mut config = TransportConfig::default();
        config.listen_address = Some("tcp://127.0.0.1:0".to_string());
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
}
