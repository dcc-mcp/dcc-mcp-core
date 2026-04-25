//! Global bridge context registry and helper functions.

use super::*;
use dcc_mcp_utils::filesystem::get_app_skill_paths_from_env;
use std::sync::OnceLock;

/// Global bridge context registry (for gateway mode).
///
/// This singleton stores bridge connections that skill scripts can query.
static BRIDGE_REGISTRY: OnceLock<crate::BridgeRegistry> = OnceLock::new();

// ── PyBridgeContext ──────────────────────────────────────────────────────

/// Python-facing bridge connection context.
///
/// Example::
///
///     from dcc_mcp_core import get_bridge_context, register_bridge
///
///     register_bridge("photoshop", "ws://localhost:9001")
///     ctx = get_bridge_context("photoshop")
///     if ctx:
///         print(ctx.dcc_type, ctx.bridge_url, ctx.connected)
#[pyclass(name = "BridgeContext", get_all, skip_from_py_object)]
#[derive(Debug, Clone)]
pub struct PyBridgeContext {
    pub dcc_type: String,
    pub bridge_url: String,
    pub connected: bool,
}

#[pymethods]
impl PyBridgeContext {
    fn __repr__(&self) -> String {
        format!(
            "BridgeContext(dcc_type={}, url={}, connected={})",
            self.dcc_type, self.bridge_url, self.connected
        )
    }
}

impl From<crate::BridgeContext> for PyBridgeContext {
    fn from(ctx: crate::BridgeContext) -> Self {
        Self {
            dcc_type: ctx.dcc_type,
            bridge_url: ctx.bridge_url,
            connected: ctx.connected,
        }
    }
}

// ── PyBridgeRegistry ─────────────────────────────────────────────────────

/// Python-facing bridge connection registry.
///
/// Thread-safe registry for bridge connections available in gateway mode.
/// Bridge plugins register their connection info, and skill scripts query
/// it to discover available bridges.
///
/// Example::
///
///     from dcc_mcp_core import BridgeRegistry
///
///     registry = BridgeRegistry()
///     registry.register("photoshop", "ws://localhost:9001")
///     registry.register("zbrush", "http://localhost:8765")
///
///     ctx = registry.get("photoshop")
///     print(ctx.bridge_url, ctx.connected)
///
///     for ctx in registry.list_all():
///         print(ctx.dcc_type, ctx.connected)
///
///     registry.set_disconnected("photoshop")
///     registry.unregister("zbrush")
#[pyclass(name = "BridgeRegistry", skip_from_py_object)]
#[derive(Debug, Clone)]
pub struct PyBridgeRegistry {
    inner: crate::BridgeRegistry,
}

#[pymethods]
impl PyBridgeRegistry {
    #[new]
    fn new() -> Self {
        Self {
            inner: crate::BridgeRegistry::new(),
        }
    }

    /// Register or update a bridge connection.
    ///
    /// Args:
    ///     dcc_type: DCC type identifier (e.g., ``"photoshop"``).
    ///     url: Bridge endpoint URL (e.g., ``"ws://localhost:9001"``).
    ///
    /// Raises:
    ///     ValueError: If ``dcc_type`` or ``url`` is empty.
    fn register(&self, dcc_type: String, url: String) -> PyResult<()> {
        self.inner
            .register(dcc_type, url)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Get bridge context for a specific DCC type.
    ///
    /// Returns ``None`` if no bridge is registered for the given DCC type.
    fn get(&self, dcc_type: &str) -> Option<PyBridgeContext> {
        self.inner.get(dcc_type).map(PyBridgeContext::from)
    }

    /// Get bridge URL for a specific DCC type (convenience method).
    ///
    /// Returns ``None`` if no bridge is registered.
    fn get_url(&self, dcc_type: &str) -> Option<String> {
        self.inner.get_url(dcc_type)
    }

    /// List all registered bridges.
    fn list_all(&self) -> Vec<PyBridgeContext> {
        self.inner
            .list_all()
            .into_iter()
            .map(PyBridgeContext::from)
            .collect()
    }

    /// Mark a bridge as disconnected without removing it from the registry.
    ///
    /// Raises:
    ///     ValueError: If the bridge is not found.
    fn set_disconnected(&self, dcc_type: &str) -> PyResult<()> {
        self.inner
            .set_disconnected(dcc_type)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Remove a bridge from the registry.
    ///
    /// Raises:
    ///     ValueError: If the bridge is not found.
    fn unregister(&self, dcc_type: &str) -> PyResult<()> {
        self.inner
            .unregister(dcc_type)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Clear all registered bridges.
    fn clear(&self) {
        self.inner.clear();
    }

    /// Check if a bridge is registered for the given DCC type.
    fn contains(&self, dcc_type: &str) -> bool {
        self.inner.contains(dcc_type)
    }

    /// Get the number of registered bridges.
    fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the registry is empty.
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn __repr__(&self) -> String {
        format!("BridgeRegistry(count={})", self.inner.len())
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }
}

// ── Global bridge functions ──────────────────────────────────────────────

/// Get bridge context for a specific DCC type.
///
/// In gateway mode, external bridge plugins register their connection info
/// via :func:`register_bridge`, allowing skill scripts to access bridges from
/// other processes.
///
/// Args:
///     dcc_type: DCC type identifier (e.g., ``"photoshop"``, ``"zbrush"``).
///
/// Returns:
///     A :class:`BridgeContext` if registered, or ``None``.
///
/// Example::
///
///     from dcc_mcp_core import get_bridge_context, register_bridge
///
///     register_bridge("photoshop", "ws://localhost:9001")
///     ctx = get_bridge_context("photoshop")
///     if ctx:
///         print(ctx.bridge_url, ctx.connected)
///     else:
///         raise PhotoshopNotAvailableError("Bridge not connected")
#[pyfunction]
#[pyo3(name = "get_bridge_context")]
pub fn py_get_bridge_context(dcc_type: &str) -> Option<PyBridgeContext> {
    let registry = BRIDGE_REGISTRY.get_or_init(crate::BridgeRegistry::new);
    registry.get(dcc_type).map(PyBridgeContext::from)
}

/// Register a bridge connection in the global registry.
///
/// Called by bridge plugins to register their connection info so that
/// skill scripts can discover and use them via :func:`get_bridge_context`.
///
/// Args:
///     dcc_type: DCC type identifier (e.g., ``"photoshop"``).
///     url: Bridge endpoint URL (e.g., ``"ws://localhost:9001"``).
///
/// Raises:
///     ValueError: If ``dcc_type`` or ``url`` is empty.
///
/// Example::
///
///     from dcc_mcp_core import register_bridge
///
///     register_bridge("photoshop", "ws://localhost:9001")
///     register_bridge("zbrush", "http://localhost:8765")
#[pyfunction]
#[pyo3(name = "register_bridge")]
pub fn py_register_bridge(dcc_type: String, url: String) -> PyResult<()> {
    let registry = BRIDGE_REGISTRY.get_or_init(crate::BridgeRegistry::new);
    registry
        .register(dcc_type, url)
        .map_err(pyo3::exceptions::PyValueError::new_err)
}

/// Register a bridge connection (internal/gateway use).
///
/// Called by bridge plugins to register their connection info.
#[doc(hidden)]
pub fn register_bridge_internal(dcc_type: String, url: String) -> Result<(), String> {
    let registry = BRIDGE_REGISTRY.get_or_init(crate::BridgeRegistry::new);
    registry.register(dcc_type, url)
}

// ── register_classes ─────────────────────────────────────────────────────

/// Register all Python classes in this module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyMcpHttpConfig>()?;
    m.add_class::<PyMcpHttpServer>()?;
    m.add_class::<PyServerHandle>()?;
    m.add_class::<PyBridgeContext>()?;
    m.add_class::<PyBridgeRegistry>()?;
    m.add_class::<PyWorkspaceRoots>()?;
    m.add_function(wrap_pyfunction!(py_create_skill_server, m)?)?;
    m.add_function(wrap_pyfunction!(py_get_bridge_context, m)?)?;
    m.add_function(wrap_pyfunction!(py_register_bridge, m)?)?;
    Ok(())
}

// ── py_create_skill_server ───────────────────────────────────────────────

/// Create a pre-configured `McpHttpServer` for a specific DCC application.
///
/// This is the recommended entry-point for the **Skills-First** workflow.
/// It automatically:
///
/// 1. Creates an `ActionRegistry` and `ActionDispatcher`.
/// 2. Creates a `SkillCatalog` wired to the dispatcher.
/// 3. Discovers skills from **both** env vars (per-app + global):
///    - ``DCC_MCP_{APP}_SKILL_PATHS`` — e.g. ``DCC_MCP_MAYA_SKILL_PATHS``
///    - ``DCC_MCP_SKILL_PATHS`` — global fallback
/// 4. Returns a ready-to-start ``McpHttpServer``.
///
/// Args:
///     app_name: DCC application name (e.g. ``"maya"``, ``"blender"``).
///               Used to derive the per-app env var and as the MCP server name.
///     config:   Optional ``McpHttpConfig``; defaults to port 8765.
///     extra_paths: Extra skill directories to scan in addition to env var paths.
///     dcc_name: Override the DCC filter for skill scanning (defaults to ``app_name``).
///
/// Example::
///
///     import os
///     os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"
///
///     from dcc_mcp_core import create_skill_manager, McpHttpConfig
///
///     server = create_skill_manager("maya", McpHttpConfig(port=8765))
///     handle = server.start()
///     print(f"Maya MCP server at {handle.mcp_url()}")
///     # Agents connect, call search_skills() and load_skill() to discover tools.
///
/// .. note::
///
///     The returned server's ``SkillCatalog`` is pre-populated with discovered
///     skills but none are *loaded* yet. Use ``server.load_skill(name)`` or
///     the ``load_skill`` MCP tool to load skills on demand.
#[pyfunction]
#[pyo3(name = "create_skill_server")]
#[pyo3(signature = (app_name, config=None, extra_paths=None, dcc_name=None))]
pub fn py_create_skill_server(
    app_name: &str,
    config: Option<&PyMcpHttpConfig>,
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<PyMcpHttpServer> {
    // Determine DCC filter — default to app_name
    let effective_dcc = dcc_name.unwrap_or(app_name);

    // Build config with app_name as default server name
    let mut cfg = config.map(|c| c.inner.clone()).unwrap_or_default();
    if cfg.server_name == "dcc-mcp-server" || cfg.server_name.is_empty() {
        cfg.server_name = format!("{app_name}-mcp");
    }
    // Issue #303: force Dedicated mode for PyO3 callers, which matches
    // what PyMcpHttpConfig's constructor picks when called from Python.
    // Callers that really know what they're doing (i.e. running inside a
    // persistent #[tokio::main] driver) can still set spawn_mode back to
    // "ambient" on the config before passing it in.
    if matches!(cfg.spawn_mode, ServerSpawnMode::Ambient) {
        cfg.spawn_mode = ServerSpawnMode::Dedicated;
    }

    let runtime =
        Runtime::new().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

    let reg = Arc::new(ActionRegistry::new());
    let dispatcher = Arc::new(ActionDispatcher::new((*reg).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        reg.clone(),
        dispatcher.clone(),
    ));

    // Collect paths: explicit extra_paths + per-app env var + global env var
    let mut all_paths: Vec<String> = extra_paths.unwrap_or_default();
    all_paths.extend(get_app_skill_paths_from_env(app_name));
    let discover_paths = if all_paths.is_empty() {
        None
    } else {
        Some(all_paths)
    };

    // Discover skills (lenient — missing deps are skipped, not errors)
    let discovered = catalog.discover(discover_paths.as_deref(), Some(effective_dcc));
    tracing::info!("create_skill_server({app_name}): discovered {discovered} skill(s)");

    let live_meta = Arc::new(RwLock::new(LiveMetaInner {
        scene: cfg.scene.clone(),
        version: cfg.dcc_version.clone(),
        ..Default::default()
    }));
    Ok(PyMcpHttpServer {
        registry: reg,
        dispatcher,
        catalog,
        config: cfg,
        runtime: Arc::new(runtime),
        live_meta,
    })
}
