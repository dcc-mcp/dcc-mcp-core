//! Typed `workspace://` URI resolver.

use super::*;

/// Typed `workspace://` URI resolver built from the client-advertised MCP
/// roots (issue #354).
///
/// Example::
///
///     from dcc_mcp_core import WorkspaceRoots
///     roots = WorkspaceRoots(["/projects/hero"])
///     assert roots.resolve("workspace://scenes/a.usd").endswith("scenes/a.usd")
///     assert roots.roots == ["file:///projects/hero"]
#[pyclass(name = "WorkspaceRoots", skip_from_py_object)]
#[derive(Clone, Default)]
pub struct PyWorkspaceRoots {
    pub(crate) inner: crate::workspace::WorkspaceRoots,
}

#[pymethods]
impl PyWorkspaceRoots {
    /// Build from a list of filesystem roots, URI strings, or a mix.
    ///
    /// Each entry that already starts with a scheme (``file://``,
    /// ``custom://``) is kept verbatim; bare paths are converted into a
    /// ``file://`` URI.
    #[new]
    #[pyo3(signature = (roots = None))]
    fn new(roots: Option<Vec<String>>) -> Self {
        let raw = roots.unwrap_or_default();
        let mut client_roots = Vec::with_capacity(raw.len());
        for r in raw {
            let uri = if r.contains("://") {
                r
            } else {
                let normalised = r.replace('\\', "/");
                if normalised.starts_with('/') {
                    format!("file://{normalised}")
                } else {
                    format!("file:///{normalised}")
                }
            };
            client_roots.push(crate::protocol::ClientRoot { uri, name: None });
        }
        Self {
            inner: crate::workspace::WorkspaceRoots::from_client_roots(&client_roots),
        }
    }

    /// All roots (as URI strings) in declaration order.
    #[getter]
    fn roots(&self) -> Vec<String> {
        self.inner.roots().to_vec()
    }

    /// Resolve a typed path against the workspace.
    ///
    /// Rules:
    ///
    /// * ``workspace://<rest>`` → joined against the first advertised
    ///   ``file://`` root. Raises ``ValueError`` (MCP error code
    ///   ``-32602``) when no roots are advertised.
    /// * Absolute platform paths are returned unchanged.
    /// * Relative paths are joined against the first root when one is
    ///   available; otherwise returned unchanged.
    fn resolve(&self, path: &str) -> PyResult<String> {
        match self.inner.resolve(path) {
            Ok(p) => Ok(p.to_string_lossy().into_owned()),
            Err(e) => Err(pyo3::exceptions::PyValueError::new_err(e.to_string())),
        }
    }

    fn __repr__(&self) -> String {
        format!("WorkspaceRoots(roots={:?})", self.inner.roots())
    }
}
