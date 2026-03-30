//! TransportManager — main entry point for the transport layer.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::config::TransportConfig;
use crate::discovery::types::{ServiceEntry, ServiceKey, ServiceStatus};
use crate::discovery::ServiceRegistry;
use crate::error::{TransportError, TransportResult};
use crate::pool::ConnectionPool;
use crate::session::{Session, SessionManager};

/// Main entry point for the transport layer.
///
/// Manages the connection pool, service registry, session manager,
/// and provides a unified API for communicating with DCC applications.
pub struct TransportManager {
    /// Connection pool.
    pool: ConnectionPool,
    /// Service registry.
    registry: ServiceRegistry,
    /// Session manager.
    sessions: SessionManager,
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
            config,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
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

    // ── Session Management ──

    /// Get or create a session for a DCC instance (lazy creation).
    ///
    /// If no instance_id is specified, picks the first available instance.
    pub fn get_or_create_session(
        &self,
        dcc_type: &str,
        instance_id: Option<Uuid>,
    ) -> TransportResult<Uuid> {
        self.check_shutdown()?;

        let entry = self.resolve_instance(dcc_type, instance_id)?;
        self.sessions
            .get_or_create(&entry.dcc_type, entry.instance_id, &entry.host, entry.port)
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
    /// If no instance_id is specified, picks the first available instance.
    pub async fn acquire_connection(
        &self,
        dcc_type: &str,
        instance_id: Option<Uuid>,
    ) -> TransportResult<Uuid> {
        self.check_shutdown()?;

        let entry = self.resolve_instance(dcc_type, instance_id)?;
        let key = entry.key();
        self.pool.acquire(&key, &entry.host, entry.port).await
    }

    /// Release a connection back to the pool.
    pub fn release_connection(&self, key: &ServiceKey) {
        self.pool.release(key);
    }

    /// Get pool statistics.
    pub fn pool_size(&self) -> usize {
        self.pool.len()
    }

    /// Get the number of connections for a specific DCC type.
    pub fn pool_count_for_dcc(&self, dcc_type: &str) -> usize {
        self.pool.count_for_dcc(dcc_type)
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
    ) -> TransportResult<ServiceEntry> {
        if let Some(id) = instance_id {
            let key = ServiceKey {
                dcc_type: dcc_type.to_string(),
                instance_id: id,
            };
            self.registry
                .get(&key)
                .ok_or_else(|| TransportError::ServiceNotFound {
                    dcc_type: dcc_type.to_string(),
                    instance_id: id.to_string(),
                })
        } else {
            let instances = self.registry.list_instances(dcc_type);
            instances
                .into_iter()
                .find(|e| e.status == ServiceStatus::Available)
                .ok_or_else(|| TransportError::ServiceNotFound {
                    dcc_type: dcc_type.to_string(),
                    instance_id: "any".to_string(),
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TransportConfig;

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
        let session_id = manager.get_or_create_session("maya", None).unwrap();
        assert_eq!(manager.session_count(), 1);
    }

    #[tokio::test]
    async fn test_transport_manager_acquire_connection() {
        let (_dir, manager) = setup();

        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let key = entry.key();
        let instance_id = entry.instance_id;
        manager.register_service(entry).unwrap();

        let _conn_id = manager
            .acquire_connection("maya", Some(instance_id))
            .await
            .unwrap();
        assert_eq!(manager.pool_size(), 1);

        manager.release_connection(&key);
    }

    #[tokio::test]
    async fn test_transport_manager_acquire_any_instance() {
        let (_dir, manager) = setup();

        manager
            .register_service(ServiceEntry::new("maya", "127.0.0.1", 18812))
            .unwrap();
        manager
            .register_service(ServiceEntry::new("maya", "127.0.0.1", 18813))
            .unwrap();

        let _conn_id = manager.acquire_connection("maya", None).await.unwrap();
        assert_eq!(manager.pool_size(), 1);
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
        let (sessions, connections) = manager.shutdown();
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
}
