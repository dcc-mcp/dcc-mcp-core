//! TransportManager — main entry point for the transport layer.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// Round-robin counter for same-tier load balancing in find_best_service.
    round_robin_counter: Arc<AtomicUsize>,
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
            round_robin_counter: Arc::new(AtomicUsize::new(0)),
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
            round_robin_counter: Arc::new(AtomicUsize::new(0)),
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

    /// Update scene and/or version metadata for a service, and refresh heartbeat.
    ///
    /// This is the primary way for a running instance to report that the user
    /// has opened a different scene or that the DCC version has changed.
    pub fn update_metadata(
        &self,
        key: &ServiceKey,
        scene: Option<&str>,
        version: Option<&str>,
    ) -> TransportResult<bool> {
        self.check_shutdown()?;
        self.registry.update_metadata(key, scene, version)
    }

    /// Update the active document, full document list, and optional display name.
    ///
    /// Designed for multi-document DCC applications (e.g. Photoshop, After Effects).
    /// For single-document DCCs (Maya, Blender) this is equivalent to `update_metadata`
    /// with the `scene` field, but also stores `display_name` when provided.
    pub fn update_documents(
        &self,
        key: &ServiceKey,
        active_document: Option<&str>,
        documents: &[String],
        display_name: Option<&str>,
    ) -> TransportResult<bool> {
        self.check_shutdown()?;
        self.registry
            .update_documents(key, active_document, documents, display_name)
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
    /// let mut guard = arc.lock();
    /// let framed = guard.framed_mut().unwrap();
    /// framed.send(&my_request).await?;
    /// ```
    pub fn get_active_connection(
        &self,
        key: &ServiceKey,
    ) -> Option<Arc<parking_lot::Mutex<ActiveConnection>>> {
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
    ) -> TransportResult<Arc<parking_lot::Mutex<ActiveConnection>>> {
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

    // ── High-level service auto-registration & discovery ──

    /// Bind a listener on the optimal transport for this machine, register the
    /// service in the registry, and return the `(instance_id, listener)` pair.
    ///
    /// **DCC plugin usage** — one call replaces the manual bind → local_address → register flow:
    ///
    /// ```ignore
    /// let (instance_id, listener) = manager
    ///     .bind_and_register("maya", Some("2025"), None)
    ///     .await?;
    ///
    /// // listener is ready; start serving connections
    /// manager.serve(Arc::new(manager), listener, ServiceKey {
    ///     dcc_type: "maya".into(),
    ///     instance_id,
    /// });
    /// ```
    ///
    /// Transport selection priority:
    /// 1. Named Pipe (Windows) / Unix Socket (macOS/Linux) — zero-config, PID-unique
    /// 2. TCP on ephemeral port (`:0`) — OS assigns a free port automatically
    pub async fn bind_and_register(
        &self,
        dcc_type: &str,
        version: Option<String>,
        metadata: Option<std::collections::HashMap<String, String>>,
    ) -> TransportResult<(uuid::Uuid, IpcListener)> {
        self.check_shutdown()?;

        let pid = std::process::id();
        let addr = TransportAddress::default_local(dcc_type, pid);

        // Bind the listener — for IPC addresses this is immediate;
        // for TCP :0 the OS assigns a free port.
        let listener = IpcListener::bind(&addr).await?;

        // Read back the actual bound address (important for TCP :0 → real port)
        let bound_addr = listener.local_address()?;

        let mut entry = ServiceEntry::new(
            dcc_type,
            match &bound_addr {
                TransportAddress::Tcp { host, .. } => host.as_str(),
                _ => "127.0.0.1",
            },
            match &bound_addr {
                TransportAddress::Tcp { port, .. } => *port,
                _ => 0,
            },
        );
        entry.version = version;
        if let Some(md) = metadata {
            entry.metadata = md;
        }
        entry.transport_address = Some(bound_addr);
        let instance_id = entry.instance_id;

        tracing::info!(
            dcc_type,
            %instance_id,
            transport = %listener.transport_name(),
            "auto-registered DCC service"
        );

        self.registry.register(entry)?;
        Ok((instance_id, listener))
    }

    /// Discover the best available service instance for the given DCC type.
    ///
    /// Returns the highest-priority *live* [`ServiceEntry`] using the following
    /// priority order:
    ///
    /// 1. **Local IPC** (Named Pipe / Unix Socket) — lowest latency, same machine
    /// 2. **Local TCP** (`127.0.0.1` / `localhost`) — same machine
    /// 3. **Remote TCP** — cross-machine
    ///
    /// Within the same tier, `AVAILABLE` instances are preferred over `BUSY`.
    /// `UNREACHABLE` and `SHUTTING_DOWN` instances are excluded.
    ///
    /// When multiple instances share the same best score (e.g. two local AVAILABLE IPC
    /// instances of Maya are both running), round-robin selection distributes load evenly
    /// across calls.
    pub fn find_best_service(&self, dcc_type: &str) -> TransportResult<ServiceEntry> {
        let ranked = self.rank_services(dcc_type)?;
        // All entries in ranked are sorted by score. Pick the best score bucket
        // and round-robin within it.
        let best_score = self.score_entry(&ranked[0]);
        let best_tier: Vec<&ServiceEntry> = ranked
            .iter()
            .take_while(|e| self.score_entry(e) == best_score)
            .collect();

        let idx = self.round_robin_counter.fetch_add(1, Ordering::Relaxed) % best_tier.len();

        tracing::debug!(
            dcc_type,
            candidates = best_tier.len(),
            selected = idx,
            score = best_score,
            "find_best_service selected instance"
        );

        Ok(best_tier[idx].clone())
    }

    /// Return all live instances for `dcc_type`, sorted by connection preference.
    ///
    /// This is the list-form companion to [`find_best_service`]. Use it when the
    /// caller needs to iterate over all viable options — for example to implement
    /// custom fallback logic or display all available instances to the user.
    ///
    /// Ordering (lower score = more preferred):
    ///
    /// | Score | Tier |
    /// |-------|------|
    /// | 0 | Local IPC, AVAILABLE |
    /// | 1 | Local IPC, BUSY |
    /// | 2 | Local TCP, AVAILABLE |
    /// | 3 | Local TCP, BUSY |
    /// | 4 | Remote TCP, AVAILABLE |
    /// | 5 | Remote TCP, BUSY |
    ///
    /// `UNREACHABLE` and `SHUTTING_DOWN` instances are excluded.
    pub fn rank_services(&self, dcc_type: &str) -> TransportResult<Vec<ServiceEntry>> {
        let instances = self.registry.list_instances(dcc_type);
        if instances.is_empty() {
            return Err(TransportError::ServiceNotFound {
                dcc_type: dcc_type.to_string(),
                instance_id: "any".to_string(),
            });
        }

        let mut live: Vec<ServiceEntry> = instances
            .into_iter()
            .filter(|e| matches!(e.status, ServiceStatus::Available | ServiceStatus::Busy))
            .collect();

        if live.is_empty() {
            return Err(TransportError::ServiceNotFound {
                dcc_type: dcc_type.to_string(),
                instance_id: "all instances are unreachable or shutting down".to_string(),
            });
        }

        live.sort_by_key(|e| self.score_entry(e));
        Ok(live)
    }

    /// Compute the preference score for a service entry (lower = more preferred).
    fn score_entry(&self, e: &ServiceEntry) -> u8 {
        let busy_penalty: u8 = if e.status == ServiceStatus::Available {
            0
        } else {
            1
        };
        if e.is_ipc() {
            busy_penalty
        } else if e.effective_address().is_local() {
            2 + busy_penalty
        } else {
            4 + busy_penalty
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
mod tests;
