//! PyO3 bindings for the MCP HTTP server.

use pyo3::prelude::*;
use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::{
    config::McpHttpConfig,
    error::HttpError,
    server::{McpHttpServer, ServerHandle},
};
use dcc_mcp_actions::ActionRegistry;

/// Python-visible MCP HTTP server configuration.
///
/// Example::
///
///     from dcc_mcp_core import McpHttpConfig
///     config = McpHttpConfig(port=8765, server_name="my-dcc")
#[pyclass(name = "McpHttpConfig", skip_from_py_object)]
#[derive(Clone)]
pub struct PyMcpHttpConfig {
    pub(crate) inner: McpHttpConfig,
}

#[pymethods]
impl PyMcpHttpConfig {
    /// Create a new config. ``port=0`` binds to any available port.
    #[new]
    #[pyo3(signature = (port=8765, server_name=None, server_version=None, enable_cors=false, request_timeout_ms=30000))]
    fn new(
        port: u16,
        server_name: Option<String>,
        server_version: Option<String>,
        enable_cors: bool,
        request_timeout_ms: u64,
    ) -> Self {
        let mut cfg = McpHttpConfig::new(port);
        if let Some(name) = server_name {
            cfg.server_name = name;
        }
        if let Some(ver) = server_version {
            cfg.server_version = ver;
        }
        cfg.enable_cors = enable_cors;
        cfg.request_timeout_ms = request_timeout_ms;
        Self { inner: cfg }
    }

    #[getter]
    fn port(&self) -> u16 {
        self.inner.port
    }

    #[getter]
    fn server_name(&self) -> &str {
        &self.inner.server_name
    }

    #[getter]
    fn server_version(&self) -> &str {
        &self.inner.server_version
    }

    fn __repr__(&self) -> String {
        format!(
            "McpHttpConfig(port={}, name={})",
            self.inner.port, self.inner.server_name
        )
    }
}

/// Handle returned by `McpHttpServer.start()`.
///
/// Example::
///
///     handle = server.start()
///     # ... later ...
///     handle.shutdown()
#[pyclass(name = "ServerHandle", skip_from_py_object)]
pub struct PyServerHandle {
    inner: Option<ServerHandle>,
    runtime: Arc<Runtime>,
    pub port: u16,
    pub bind_addr: String,
}

#[pymethods]
impl PyServerHandle {
    /// The actual port the server is listening on.
    #[getter]
    fn port(&self) -> u16 {
        self.port
    }

    /// The bind address (e.g. ``127.0.0.1:8765``).
    #[getter]
    fn bind_addr(&self) -> &str {
        &self.bind_addr
    }

    /// The full MCP endpoint URL.
    fn mcp_url(&self) -> String {
        format!("http://{}/mcp", self.bind_addr)
    }

    /// Gracefully shut down the server.
    fn shutdown(&mut self) {
        if let Some(handle) = self.inner.take() {
            self.runtime.block_on(handle.shutdown());
        }
    }

    /// Signal shutdown without blocking.
    fn signal_shutdown(&self) {
        if let Some(handle) = &self.inner {
            handle.signal_shutdown();
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ServerHandle(addr={}, running={})",
            self.bind_addr,
            self.inner.is_some()
        )
    }
}

/// MCP Streamable HTTP server for embedding in DCC software.
///
/// Example::
///
///     from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig
///
///     registry = ActionRegistry()
///     registry.register("get_scene_info", description="Get scene info", category="scene")
///
///     server = McpHttpServer(registry, McpHttpConfig(port=8765))
///     handle = server.start()
///     print(f"MCP server at {handle.mcp_url()}")
///     # MCP Host connects to http://127.0.0.1:8765/mcp
///
///     # Shutdown:
///     handle.shutdown()
#[pyclass(name = "McpHttpServer", skip_from_py_object)]
pub struct PyMcpHttpServer {
    registry: Arc<ActionRegistry>,
    config: McpHttpConfig,
    runtime: Arc<Runtime>,
}

#[pymethods]
impl PyMcpHttpServer {
    /// Create a new MCP HTTP server.
    ///
    /// Args:
    ///     registry: An ``ActionRegistry`` with registered DCC actions.
    ///     config: A ``McpHttpConfig``. If omitted, defaults to port 8765.
    #[new]
    #[pyo3(signature = (registry, config=None))]
    fn new(registry: &ActionRegistry, config: Option<&PyMcpHttpConfig>) -> PyResult<Self> {
        let cfg = config.map(|c| c.inner.clone()).unwrap_or_default();

        let runtime =
            Runtime::new().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        Ok(Self {
            registry: Arc::new(registry.clone()),
            config: cfg,
            runtime: Arc::new(runtime),
        })
    }

    /// Start the server and return a :class:`ServerHandle`.
    ///
    /// This call returns immediately; the server runs in a background thread.
    fn start(&self) -> PyResult<PyServerHandle> {
        let server = McpHttpServer::new(self.registry.clone(), self.config.clone());
        let handle = self
            .runtime
            .block_on(server.start())
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let port = handle.port;
        let bind_addr = handle.bind_addr.clone();

        Ok(PyServerHandle {
            inner: Some(handle),
            runtime: self.runtime.clone(),
            port,
            bind_addr,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "McpHttpServer(name={}, port={})",
            self.config.server_name, self.config.port
        )
    }
}

/// Register all Python classes in this module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyMcpHttpConfig>()?;
    m.add_class::<PyMcpHttpServer>()?;
    m.add_class::<PyServerHandle>()?;
    Ok(())
}
