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
/// ```python,ignore
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
    ///     scene: Currently active scene/document (optional).
    ///     documents: All open documents for multi-document apps like Photoshop (optional list).
    ///     pid: OS process ID — used to disambiguate instances with the same scene (optional).
    ///     display_name: Human-readable label, e.g. "Maya-Rigging" (optional).
    ///     metadata: Arbitrary string metadata dict (optional).
    ///     transport_address: Preferred transport address (optional). When provided,
    ///         enables IPC registration (Named Pipe / Unix Socket) for lower latency.
    ///         Use TransportAddress.default_local(dcc_type, pid) to auto-select the
    ///         optimal IPC transport for the current platform.
    ///     extras: Arbitrary DCC-specific extras dict with JSON-compatible values
    ///         (dict / list / int / float / str / bool / None) — useful for
    ///         WebView / bridge specific fields such as ``cdp_port`` or ``url``.
    ///
    /// Returns:
    ///     The instance_id (UUID string) of the registered service.
    #[pyo3(name = "register_service")]
    #[pyo3(signature = (dcc_type, host, port, version=None, scene=None, documents=None, pid=None, display_name=None, metadata=None, transport_address=None, extras=None))]
    fn py_register_service(
        &self,
        dcc_type: &str,
        host: &str,
        port: u16,
        version: Option<String>,
        scene: Option<String>,
        documents: Option<Vec<String>>,
        pid: Option<u32>,
        display_name: Option<String>,
        metadata: Option<HashMap<String, String>>,
        transport_address: Option<PyRef<'_, super::types::PyTransportAddress>>,
        extras: Option<Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<String> {
        let mut entry = ServiceEntry::new(dcc_type, host, port);
        entry.version = version;
        entry.scene = scene;
        entry.documents = documents.unwrap_or_default();
        entry.pid = pid;
        entry.display_name = display_name;
        if let Some(md) = metadata {
            entry.metadata = md;
        }
        if let Some(addr) = transport_address {
            entry.transport_address = Some(addr.inner.clone());
        }
        if let Some(ex) = extras {
            let mut out = HashMap::with_capacity(ex.len());
            for (k, v) in ex.iter() {
                let key = k.extract::<String>().map_err(|_| {
                    pyo3::exceptions::PyTypeError::new_err("extras dict keys must be strings")
                })?;
                out.insert(key, super::helpers::py_to_json_value(&v)?);
            }
            entry.extras = out;
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

    /// List all registered instances across all DCC types.
    ///
    /// Alias for `list_all_services()` using the naming convention expected
    /// by smart-routing integrations (see dcc-mcp-ipc #27).
    ///
    /// Returns:
    ///     List of ServiceEntry objects for all registered DCC instances.
    ///
    /// Example:
    ///
    /// ```text
    /// from dcc_mcp_core import TransportManager
    /// mgr = TransportManager("/tmp/dcc-mcp")
    /// all_instances = mgr.list_all_instances()
    /// for entry in all_instances:
    ///     print(entry.dcc_type, entry.instance_id, entry.status)
    /// ```
    #[pyo3(name = "list_all_instances")]
    fn py_list_all_instances(&self) -> Vec<PyServiceEntry> {
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

    /// Update scene and/or version metadata for a registered service.
    ///
    /// This is the primary way for a running DCC instance (e.g. Photoshop
    /// bridge plugin) to report that the user has opened a different scene
    /// or that the DCC version has changed.  The update is written to the
    /// shared FileRegistry so the gateway and other processes can see it.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Instance UUID string.
    ///     scene: New scene name (pass None to leave unchanged, empty string to clear).
    ///     version: New DCC version (pass None to leave unchanged, empty string to clear).
    ///
    /// Returns:
    ///     True if the service was found and updated.
    #[pyo3(name = "update_scene")]
    #[pyo3(signature = (dcc_type, instance_id, scene=None, version=None))]
    fn py_update_scene(
        &self,
        dcc_type: &str,
        instance_id: &str,
        scene: Option<&str>,
        version: Option<&str>,
    ) -> PyResult<bool> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        self.inner
            .update_metadata(&key, scene, version)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Update the active document, document list, and display name for a DCC instance.
    ///
    /// Designed for **multi-document applications** (Photoshop, After Effects, etc.) that
    /// keep several files open simultaneously.  For single-document DCCs you can use
    /// :meth:`update_scene` instead — both methods refresh the heartbeat.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Instance UUID string.
    ///     active_document: The currently focused file (stored in ``scene``).
    ///         Pass ``None`` to leave unchanged; pass ``""`` to clear.
    ///     documents: Full list of open documents — **replaces** the previous list.
    ///         Pass an empty list ``[]`` to clear.
    ///     display_name: Human-readable instance label, e.g. ``"PS-Marketing"``.
    ///         Pass ``None`` to leave unchanged; pass ``""`` to clear.
    ///
    /// Returns:
    ///     ``True`` if the service was found and updated.
    ///
    /// Example::
    ///
    ///     mgr.update_documents(
    ///         "photoshop", iid,
    ///         active_document="logo.psd",
    ///         documents=["logo.psd", "banner.psd", "icon.psd"],
    ///         display_name="PS-Marketing",
    ///     )
    #[pyo3(name = "update_documents")]
    #[pyo3(signature = (dcc_type, instance_id, active_document=None, documents=None, display_name=None))]
    fn py_update_documents(
        &self,
        dcc_type: &str,
        instance_id: &str,
        active_document: Option<&str>,
        documents: Option<Vec<String>>,
        display_name: Option<&str>,
    ) -> PyResult<bool> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        let docs = documents.unwrap_or_default();
        self.inner
            .update_documents(&key, active_document, &docs, display_name)
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

    // ── High-level auto-registration & discovery ──

    /// Bind a listener on the optimal transport for this machine, register the
    /// service, and return `(instance_id, listener)`.
    ///
    /// This is the recommended DCC plugin startup call. It replaces the manual
    /// ``IpcListener.bind`` → ``local_address`` → ``register_service`` sequence.
    ///
    /// Transport priority:
    ///
    /// 1. Named Pipe (Windows) / Unix Socket (macOS/Linux) — zero-config, PID-unique
    /// 2. TCP on ephemeral port — OS assigns a free port automatically
    ///
    /// Args:
    ///     dcc_type: DCC application type (e.g. ``"maya"``).
    ///     version:  DCC version string (optional).
    ///     metadata: Arbitrary metadata dict (optional).
    ///
    /// Returns:
    ///     Tuple of ``(instance_id: str, listener: IpcListener)``.
    ///
    /// Example::
    ///
    ///     import os
    ///     from dcc_mcp_core import TransportManager
    ///
    ///     mgr = TransportManager("/tmp/dcc-mcp")
    ///     instance_id, listener = mgr.bind_and_register("maya", version="2025")
    ///     local_addr = listener.local_address()
    ///     print(f"Maya listening on {local_addr}")
    #[pyo3(name = "bind_and_register")]
    #[pyo3(signature = (dcc_type, version=None, metadata=None))]
    fn py_bind_and_register(
        &self,
        dcc_type: &str,
        version: Option<String>,
        metadata: Option<HashMap<String, String>>,
    ) -> PyResult<(String, super::listener::PyIpcListener)> {
        let (instance_id, listener) = self
            .runtime
            .block_on(self.inner.bind_and_register(dcc_type, version, metadata))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "failed to create tokio runtime for listener: {e}"
            ))
        })?;

        let py_listener = super::listener::PyIpcListener::from_listener(listener, runtime);
        Ok((instance_id.to_string(), py_listener))
    }

    /// Discover the best available service instance for the given DCC type.
    ///
    /// Returns the highest-priority live ``ServiceEntry`` using the following
    /// priority order:
    ///
    /// 1. **Local IPC** (Named Pipe / Unix Socket) — same machine, lowest latency
    /// 2. **Local TCP** (``127.0.0.1`` / ``localhost``) — same machine
    /// 3. **Remote TCP** — cross-machine
    ///
    /// Within each tier, ``AVAILABLE`` instances are preferred over ``BUSY``.
    /// ``UNREACHABLE`` and ``SHUTTING_DOWN`` instances are excluded.
    ///
    /// Args:
    ///     dcc_type: DCC application type (e.g. ``"maya"``).
    ///
    /// Returns:
    ///     The best :class:`ServiceEntry`.
    ///
    /// Raises:
    ///     RuntimeError: If no live instances are registered for the given DCC type.
    ///
    /// Example::
    ///
    ///     from dcc_mcp_core import TransportManager
    ///
    ///     mgr = TransportManager("/tmp/dcc-mcp")
    ///     entry = mgr.find_best_service("maya")
    ///     print(entry.dcc_type, entry.status, entry.effective_address())
    ///     session_id = mgr.get_or_create_session("maya", entry.instance_id)
    #[pyo3(name = "find_best_service")]
    fn py_find_best_service(&self, dcc_type: &str) -> PyResult<PyServiceEntry> {
        self.inner
            .find_best_service(dcc_type)
            .map(|e| PyServiceEntry::from(&e))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Return all live instances for `dcc_type`, sorted by connection preference.
    ///
    /// This is the list-form companion to :meth:`find_best_service`. Use it when
    /// you need to iterate over all viable options — for example to implement custom
    /// fallback logic, display a picker UI, or handle the multi-instance case where
    /// you want to talk to **all** running Maya instances in parallel.
    ///
    /// Ordering (lower = more preferred):
    ///
    /// +-------+-----------------------------------+
    /// | Score | Tier                              |
    /// +=======+===================================+
    /// | 0     | Local IPC, AVAILABLE              |
    /// +-------+-----------------------------------+
    /// | 1     | Local IPC, BUSY                   |
    /// +-------+-----------------------------------+
    /// | 2     | Local TCP, AVAILABLE              |
    /// +-------+-----------------------------------+
    /// | 3     | Local TCP, BUSY                   |
    /// +-------+-----------------------------------+
    /// | 4     | Remote TCP, AVAILABLE             |
    /// +-------+-----------------------------------+
    /// | 5     | Remote TCP, BUSY                  |
    /// +-------+-----------------------------------+
    ///
    /// ``UNREACHABLE`` and ``SHUTTING_DOWN`` instances are excluded.
    ///
    /// Args:
    ///     dcc_type: DCC application type (e.g. ``"maya"``).
    ///
    /// Returns:
    ///     List of :class:`ServiceEntry` sorted by preference (best first).
    ///
    /// Raises:
    ///     RuntimeError: If no live instances are registered.
    ///
    /// Example — talk to all local Maya instances::
    ///
    ///     from dcc_mcp_core import TransportManager
    ///
    ///     mgr = TransportManager("/tmp/dcc-mcp")
    ///
    ///     # 3 Maya instances running locally
    ///     for entry in mgr.rank_services("maya"):
    ///         print(entry.instance_id, entry.status, entry.effective_address())
    ///         session_id = mgr.get_or_create_session("maya", entry.instance_id)
    ///         # ... dispatch work to each instance
    #[pyo3(name = "rank_services")]
    fn py_rank_services(&self, dcc_type: &str) -> PyResult<Vec<PyServiceEntry>> {
        self.inner
            .rank_services(dcc_type)
            .map(|v| v.iter().map(PyServiceEntry::from).collect())
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
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
