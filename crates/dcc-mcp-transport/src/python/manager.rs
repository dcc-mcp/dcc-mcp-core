//! Python binding for `TransportManager`.
//!
//! Wraps the Rust `TransportManager` and bridges async operations to synchronous
//! calls via an internal Tokio runtime.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use std::collections::HashMap;

#[cfg(feature = "python-bindings")]
use crate::config::{PoolConfig, SessionConfig, TransportConfig};
#[cfg(feature = "python-bindings")]
use crate::discovery::types::{ServiceEntry, ServiceKey, ServiceStatus};
#[cfg(feature = "python-bindings")]
use crate::routing::RoutingStrategy;
#[cfg(feature = "python-bindings")]
use crate::transport::TransportManager;

#[cfg(feature = "python-bindings")]
use super::helpers::{parse_uuid, session_to_py};
#[cfg(feature = "python-bindings")]
use super::types::{PyRoutingStrategy, PyServiceEntry, PyServiceStatus};

// ── PyTransportManager ──

/// Python-facing TransportManager.
///
/// Wraps the Rust `TransportManager` with a Tokio runtime for async→sync bridging.
///
/// ```python
/// from dcc_mcp_core import TransportManager
///
/// transport = TransportManager("/path/to/registry")
/// transport.register_service("maya", "127.0.0.1", 18812)
/// instances = transport.list_instances("maya")
/// session_id = transport.get_or_create_session("maya")
/// transport.shutdown()
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "TransportManager")]
pub struct PyTransportManager {
    pub(super) inner: TransportManager,
    pub(super) runtime: tokio::runtime::Runtime,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyTransportManager {
    /// Create a new TransportManager.
    ///
    /// Args:
    ///     registry_dir: Directory for the file-based service registry.
    ///     max_connections_per_dcc: Maximum connections per DCC type (default: 10).
    ///     idle_timeout: Session idle timeout in seconds (default: 300).
    ///     heartbeat_interval: Heartbeat interval in seconds (default: 5).
    ///     connect_timeout: Connection timeout in seconds (default: 10).
    ///     reconnect_max_retries: Max reconnection retries (default: 3).
    #[new]
    #[pyo3(signature = (
        registry_dir,
        max_connections_per_dcc=10,
        idle_timeout=300,
        heartbeat_interval=5,
        connect_timeout=10,
        reconnect_max_retries=3,
    ))]
    fn py_new(
        registry_dir: &str,
        max_connections_per_dcc: usize,
        idle_timeout: u64,
        heartbeat_interval: u64,
        connect_timeout: u64,
        reconnect_max_retries: u32,
    ) -> PyResult<Self> {
        let config = TransportConfig {
            pool: PoolConfig {
                max_connections_per_type: max_connections_per_dcc,
                ..Default::default()
            },
            session: SessionConfig {
                idle_timeout: std::time::Duration::from_secs(idle_timeout),
                reconnect_max_retries,
                heartbeat_interval: std::time::Duration::from_secs(heartbeat_interval),
                ..Default::default()
            },
            connect_timeout: std::time::Duration::from_secs(connect_timeout),
            heartbeat_interval: std::time::Duration::from_secs(heartbeat_interval),
            listen_address: None,
        };

        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "failed to create tokio runtime: {}",
                e
            ))
        })?;

        let inner = TransportManager::new(config, registry_dir)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        Ok(Self { inner, runtime })
    }

    // ── Service Discovery ──

    /// Register a DCC service instance.
    ///
    /// Args:
    ///     dcc_type: DCC application type (e.g. "maya", "houdini").
    ///     host: Host address (e.g. "127.0.0.1").
    ///     port: Port number.
    ///     version: DCC version string (optional).
    ///     scene: Currently open scene/file (optional).
    ///     metadata: Arbitrary metadata dict (optional).
    ///
    /// Returns:
    ///     The instance_id (UUID string) of the registered service.
    #[pyo3(name = "register_service")]
    #[pyo3(signature = (dcc_type, host, port, version=None, scene=None, metadata=None))]
    fn py_register_service(
        &self,
        dcc_type: &str,
        host: &str,
        port: u16,
        version: Option<String>,
        scene: Option<String>,
        metadata: Option<HashMap<String, String>>,
    ) -> PyResult<String> {
        let mut entry = ServiceEntry::new(dcc_type, host, port);
        entry.version = version;
        entry.scene = scene;
        if let Some(md) = metadata {
            entry.metadata = md;
        }
        let instance_id = entry.instance_id.to_string();
        self.inner
            .register_service(entry)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(instance_id)
    }

    /// Deregister a DCC service instance.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Instance UUID string.
    ///
    /// Returns:
    ///     True if the service was found and removed.
    #[pyo3(name = "deregister_service")]
    fn py_deregister_service(&self, dcc_type: &str, instance_id: &str) -> PyResult<bool> {
        let uuid = uuid::Uuid::parse_str(instance_id)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid UUID: {}", e)))?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        let result = self
            .inner
            .deregister_service(&key)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(result.is_some())
    }

    /// List all instances for a given DCC type.
    ///
    /// Returns:
    ///     List of ServiceEntry objects.
    #[pyo3(name = "list_instances")]
    fn py_list_instances(&self, dcc_type: &str) -> Vec<PyServiceEntry> {
        self.inner
            .list_instances(dcc_type)
            .iter()
            .map(PyServiceEntry::from)
            .collect()
    }

    /// List all registered services.
    ///
    /// Returns:
    ///     List of ServiceEntry objects.
    #[pyo3(name = "list_all_services")]
    fn py_list_all_services(&self) -> Vec<PyServiceEntry> {
        self.inner
            .list_all_services()
            .iter()
            .map(PyServiceEntry::from)
            .collect()
    }

    /// Get a specific service entry by DCC type and instance ID.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Instance UUID string.
    ///
    /// Returns:
    ///     ServiceEntry if found, None otherwise.
    #[pyo3(name = "get_service")]
    fn py_get_service(
        &self,
        dcc_type: &str,
        instance_id: &str,
    ) -> PyResult<Option<PyServiceEntry>> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        Ok(self
            .inner
            .get_service(&key)
            .as_ref()
            .map(PyServiceEntry::from))
    }

    /// Update heartbeat for a service.
    #[pyo3(name = "heartbeat")]
    fn py_heartbeat(&self, dcc_type: &str, instance_id: &str) -> PyResult<bool> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        self.inner
            .heartbeat(&key)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Update the status of a registered service.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Instance UUID string.
    ///     status: New status (ServiceStatus enum value).
    ///
    /// Returns:
    ///     True if the service was found and updated.
    #[pyo3(name = "update_service_status")]
    fn py_update_service_status(
        &self,
        dcc_type: &str,
        instance_id: &str,
        status: &PyServiceStatus,
    ) -> PyResult<bool> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        let rust_status = match status {
            PyServiceStatus::Available => ServiceStatus::Available,
            PyServiceStatus::Busy => ServiceStatus::Busy,
            PyServiceStatus::Unreachable => ServiceStatus::Unreachable,
            PyServiceStatus::ShuttingDown => ServiceStatus::ShuttingDown,
        };
        self.inner
            .update_service_status(&key, rust_status)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    // ── Session Management ──

    /// Get or create a session for a DCC instance (lazy creation).
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Specific instance UUID (optional). If None, picks first available.
    ///
    /// Returns:
    ///     Session ID (UUID string).
    #[pyo3(name = "get_or_create_session")]
    #[pyo3(signature = (dcc_type, instance_id=None))]
    fn py_get_or_create_session(
        &self,
        dcc_type: &str,
        instance_id: Option<&str>,
    ) -> PyResult<String> {
        let uuid = instance_id.map(parse_uuid).transpose()?;
        let session_id = self
            .inner
            .get_or_create_session(dcc_type, uuid)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(session_id.to_string())
    }

    /// Get or create a session with smart routing.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     strategy: Routing strategy (optional, uses default if None).
    ///     hint: Routing hint (instance_id for SPECIFIC, scene name for SCENE_MATCH).
    ///
    /// Returns:
    ///     Session ID (UUID string).
    #[pyo3(name = "get_or_create_session_routed")]
    #[pyo3(signature = (dcc_type, strategy=None, hint=None))]
    fn py_get_or_create_session_routed(
        &self,
        dcc_type: &str,
        strategy: Option<&PyRoutingStrategy>,
        hint: Option<&str>,
    ) -> PyResult<String> {
        let rust_strategy = strategy.map(RoutingStrategy::from);
        let session_id = self
            .inner
            .get_or_create_session_routed(dcc_type, rust_strategy, hint)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(session_id.to_string())
    }

    /// Get session info by ID.
    ///
    /// Returns:
    ///     Dict with session info, or None if not found.
    #[pyo3(name = "get_session")]
    fn py_get_session(&self, py: Python, session_id: &str) -> PyResult<Option<Py<PyAny>>> {
        let uuid = parse_uuid(session_id)?;
        Ok(self.inner.get_session(&uuid).map(|s| session_to_py(py, &s)))
    }

    /// Record a successful request on a session.
    #[pyo3(name = "record_success")]
    fn py_record_success(&self, session_id: &str, latency_ms: u64) -> PyResult<()> {
        let uuid = parse_uuid(session_id)?;
        self.inner
            .record_request_success(&uuid, std::time::Duration::from_millis(latency_ms));
        Ok(())
    }

    /// Record a failed request on a session.
    #[pyo3(name = "record_error")]
    fn py_record_error(&self, session_id: &str, latency_ms: u64, error: &str) -> PyResult<()> {
        let uuid = parse_uuid(session_id)?;
        self.inner
            .record_request_error(&uuid, std::time::Duration::from_millis(latency_ms), error);
        Ok(())
    }

    /// Begin reconnection. Returns backoff duration in milliseconds.
    #[pyo3(name = "begin_reconnect")]
    fn py_begin_reconnect(&self, session_id: &str) -> PyResult<u64> {
        let uuid = parse_uuid(session_id)?;
        let backoff = self
            .inner
            .begin_reconnect(&uuid)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(backoff.as_millis() as u64)
    }

    /// Mark reconnection as successful.
    #[pyo3(name = "reconnect_success")]
    fn py_reconnect_success(&self, session_id: &str) -> PyResult<()> {
        let uuid = parse_uuid(session_id)?;
        self.inner
            .reconnect_success(&uuid)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Close a session.
    #[pyo3(name = "close_session")]
    fn py_close_session(&self, session_id: &str) -> PyResult<bool> {
        let uuid = parse_uuid(session_id)?;
        let result = self
            .inner
            .close_session(&uuid)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(result.is_some())
    }

    /// List all active sessions.
    #[pyo3(name = "list_sessions")]
    fn py_list_sessions(&self, py: Python) -> Vec<Py<PyAny>> {
        self.inner
            .list_sessions()
            .iter()
            .map(|s| session_to_py(py, s))
            .collect()
    }

    /// List sessions for a specific DCC type.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///
    /// Returns:
    ///     List of session info dicts for the given DCC type.
    #[pyo3(name = "list_sessions_for_dcc")]
    fn py_list_sessions_for_dcc(&self, py: Python, dcc_type: &str) -> Vec<Py<PyAny>> {
        self.inner
            .list_sessions_for_dcc(dcc_type)
            .iter()
            .map(|s| session_to_py(py, s))
            .collect()
    }

    /// Get the number of active sessions.
    #[pyo3(name = "session_count")]
    fn py_session_count(&self) -> usize {
        self.inner.session_count()
    }

    // ── Connection Pool ──

    /// Acquire a connection (async bridged to sync).
    #[pyo3(name = "acquire_connection")]
    #[pyo3(signature = (dcc_type, instance_id=None))]
    fn py_acquire_connection(&self, dcc_type: &str, instance_id: Option<&str>) -> PyResult<String> {
        let uuid = instance_id.map(parse_uuid).transpose()?;
        let conn_id = self
            .runtime
            .block_on(self.inner.acquire_connection(dcc_type, uuid))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(conn_id.to_string())
    }

    /// Release a connection back to the pool.
    #[pyo3(name = "release_connection")]
    fn py_release_connection(&self, dcc_type: &str, instance_id: &str) -> PyResult<()> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        self.inner.release_connection(&key);
        Ok(())
    }

    /// Get pool size.
    #[pyo3(name = "pool_size")]
    fn py_pool_size(&self) -> usize {
        self.inner.pool_size()
    }

    /// Get the number of connections for a specific DCC type.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///
    /// Returns:
    ///     Number of pooled connections for the given DCC type.
    #[pyo3(name = "pool_count_for_dcc")]
    fn py_pool_count_for_dcc(&self, dcc_type: &str) -> usize {
        self.inner.pool_count_for_dcc(dcc_type)
    }

    // ── Lifecycle ──

    /// Cleanup stale services, idle sessions, and evict idle connections.
    ///
    /// Returns:
    ///     Tuple of (stale_services, closed_sessions, evicted_connections).
    #[pyo3(name = "cleanup")]
    fn py_cleanup(&self) -> PyResult<(usize, usize, usize)> {
        self.inner
            .cleanup()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Gracefully shut down the transport.
    #[pyo3(name = "shutdown")]
    fn py_shutdown(&self) {
        self.inner.shutdown();
    }

    /// Check if the transport is shut down.
    #[pyo3(name = "is_shutdown")]
    fn py_is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }

    fn __repr__(&self) -> String {
        format!(
            "TransportManager(services={}, sessions={}, pool={})",
            self.inner.list_all_services().len(),
            self.inner.session_count(),
            self.inner.pool_size(),
        )
    }

    fn __len__(&self) -> usize {
        self.inner.session_count()
    }
}
