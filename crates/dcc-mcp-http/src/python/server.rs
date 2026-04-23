//! Handle returned by `McpHttpServer.start()`.

use super::*;

/// Handle returned by `McpHttpServer.start()`.
///
/// Example::
///
///     handle = server.start()
///     # ... later ...
///     handle.shutdown()
#[pyclass(name = "McpServerHandle", skip_from_py_object)]
pub struct PyServerHandle {
    pub(crate) inner: Option<McpServerHandle>,
    pub(crate) runtime: Arc<Runtime>,
    pub port: u16,
    pub bind_addr: String,
    /// ``True`` if this process won the gateway port competition.
    pub is_gateway: bool,
    /// Shared live metadata — mirrors `McpHttpServer::live_meta` so Python
    /// can push scene/version/documents updates that flow into FileRegistry
    /// on the next heartbeat tick.
    pub(crate) live_meta: Arc<RwLock<LiveMetaInner>>,
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

    /// ``True`` if this process won the gateway port competition.
    #[getter]
    fn is_gateway(&self) -> bool {
        self.is_gateway
    }

    /// Update the live instance metadata in the gateway registry.
    ///
    /// Works for both single-document DCCs (Maya, Blender — pass ``scene``
    /// only) and multi-document DCCs (Photoshop, After Effects — also pass
    /// ``documents`` with the full list of open files and optionally
    /// ``display_name`` to label the instance).
    ///
    /// Values are written into the shared live-metadata store and propagated
    /// to ``FileRegistry`` on the next heartbeat tick (≤ 5 s).  After the
    /// update, ``list_dcc_instances`` reflects the change so AI agents and
    /// users can identify the correct instance without restarting.
    ///
    /// Pass ``None`` to leave a field unchanged; pass ``""`` / ``[]`` to
    /// clear it.
    ///
    /// Examples::
    ///
    ///     # Maya — single active scene:
    ///     handle.update_scene("C:/projects/hero/rig.ma")
    ///
    ///     # Photoshop — active document + all open docs + instance label:
    ///     handle.update_scene(
    ///         scene="hero_comp.psd",
    ///         documents=["hero_comp.psd", "bg_plate.psd", "overlay.psd"],
    ///         display_name="PS-Marketing",
    ///     )
    ///
    ///     # Clear the document list (single-doc mode again):
    ///     handle.update_scene(documents=[])
    ///
    /// Args:
    ///     scene: Active/focused scene or document path.
    ///             ``None`` = no change, ``""`` = clear.
    ///     version: DCC application version string.
    ///              ``None`` = no change, ``""`` = clear.
    ///     documents: Full list of open documents (multi-doc DCCs).
    ///                ``None`` = no change, ``[]`` = clear list.
    ///     display_name: Human-readable instance label (e.g. ``"PS-Marketing"``).
    ///                   ``None`` = no change, ``""`` = clear.
    #[pyo3(signature = (scene=None, version=None, documents=None, display_name=None))]
    fn update_scene(
        &self,
        scene: Option<String>,
        version: Option<String>,
        documents: Option<Vec<String>>,
        display_name: Option<String>,
    ) {
        let mut guard = self.live_meta.write();
        if let Some(s) = scene {
            guard.scene = if s.is_empty() { None } else { Some(s) };
        }
        if let Some(v) = version {
            guard.version = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(docs) = documents {
            guard.documents = docs.into_iter().filter(|d| !d.is_empty()).collect();
        }
        if let Some(name) = display_name {
            guard.display_name = if name.is_empty() { None } else { Some(name) };
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "McpServerHandle(addr={}, running={}, is_gateway={})",
            self.bind_addr,
            self.inner.is_some(),
            self.is_gateway,
        )
    }
}
