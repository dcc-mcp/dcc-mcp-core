//! PyO3 bindings for the artefact crate (issue #349).
//!
//! Exposes [`FileRef`](crate::FileRef) as a `#[pyclass]` plus helpers
//! `artefact_put_file` and `artefact_get_bytes`. The default store used by
//! the helpers lives as a process-global `OnceLock<FilesystemArtefactStore>`
//! under `<temp_dir>/dcc-mcp-artefacts` so callers outside an
//! `McpHttpServer` can still round-trip artefacts — the MCP server owns its
//! own store and wires it into the `artefact://` resource producer.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes};

use crate::{ArtefactError, ArtefactStore, FileRef, FilesystemArtefactStore, SharedArtefactStore};

/// Python wrapper for [`crate::FileRef`].
#[pyclass(name = "FileRef", module = "dcc_mcp_core._core", skip_from_py_object)]
#[derive(Clone)]
pub struct PyFileRef {
    pub(crate) inner: FileRef,
}

#[pymethods]
impl PyFileRef {
    /// Canonical URI, e.g. ``artefact://sha256/<hex>``.
    #[getter]
    fn uri(&self) -> &str {
        &self.inner.uri
    }

    /// Optional MIME type (``image/png``, ``application/json``, …).
    #[getter]
    fn mime(&self) -> Option<&str> {
        self.inner.mime.as_deref()
    }

    /// Size of the artefact body in bytes, if known.
    #[getter]
    fn size_bytes(&self) -> Option<u64> {
        self.inner.size_bytes
    }

    /// Canonical digest, e.g. ``sha256:<hex>``.
    #[getter]
    fn digest(&self) -> Option<&str> {
        self.inner.digest.as_deref()
    }

    /// UUID of the job that produced the artefact (when known).
    #[getter]
    fn producer_job_id(&self) -> Option<String> {
        self.inner.producer_job_id.map(|u| u.to_string())
    }

    /// RFC-3339 creation timestamp.
    #[getter]
    fn created_at(&self) -> String {
        self.inner.created_at.to_rfc3339()
    }

    /// Tool-defined metadata as a JSON string.
    #[getter]
    fn metadata_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner.metadata)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "FileRef(uri={}, mime={:?}, size_bytes={:?})",
            self.inner.uri, self.inner.mime, self.inner.size_bytes
        )
    }
}

impl From<FileRef> for PyFileRef {
    fn from(inner: FileRef) -> Self {
        Self { inner }
    }
}

// ── Global default store ──────────────────────────────────────────────────

static DEFAULT_STORE: OnceLock<RwLock<SharedArtefactStore>> = OnceLock::new();

fn default_store() -> SharedArtefactStore {
    let cell = DEFAULT_STORE.get_or_init(|| {
        let path = std::env::temp_dir().join("dcc-mcp-artefacts");
        let fs_store =
            FilesystemArtefactStore::new_in(path).expect("create default artefact store");
        RwLock::new(Arc::new(fs_store) as SharedArtefactStore)
    });
    cell.read().clone()
}

/// Replace the process-global default artefact store.
///
/// Not exposed to Python directly — used by `dcc-mcp-http` to point the
/// helpers at the server's own store when one is configured.
pub fn set_default_store(store: SharedArtefactStore) {
    let cell = DEFAULT_STORE.get_or_init(|| RwLock::new(store.clone()));
    *cell.write() = store;
}

fn map_err(e: ArtefactError) -> PyErr {
    match e {
        ArtefactError::NotFound(msg) => PyIOError::new_err(format!("artefact not found: {msg}")),
        ArtefactError::InvalidUri(msg) => PyValueError::new_err(format!("invalid uri: {msg}")),
        ArtefactError::Io(err) => PyIOError::new_err(err.to_string()),
        ArtefactError::Serde(err) => PyValueError::new_err(err.to_string()),
    }
}

/// Store the file at ``path`` and return a :class:`FileRef`.
///
/// The default store is a content-addressed filesystem store under
/// ``<temp_dir>/dcc-mcp-artefacts``. ``McpHttpServer`` installs its own
/// store on start, so inside a server process this helper routes to the
/// server-owned store automatically.
#[pyfunction(name = "artefact_put_file")]
#[pyo3(signature = (path, mime=None))]
pub fn py_artefact_put_file(path: &str, mime: Option<String>) -> PyResult<PyFileRef> {
    let store = default_store();
    let fr = crate::put_file(store.as_ref(), PathBuf::from(path), mime).map_err(map_err)?;
    Ok(PyFileRef::from(fr))
}

/// Store raw ``bytes`` and return a :class:`FileRef`.
#[pyfunction(name = "artefact_put_bytes")]
#[pyo3(signature = (data, mime=None))]
pub fn py_artefact_put_bytes(data: &[u8], mime: Option<String>) -> PyResult<PyFileRef> {
    let store = default_store();
    let fr = crate::put_bytes(store.as_ref(), data.to_vec(), mime).map_err(map_err)?;
    Ok(PyFileRef::from(fr))
}

/// Read back the raw bytes for an ``artefact://`` URI. Raises ``IOError``
/// when the URI is unknown.
#[pyfunction(name = "artefact_get_bytes")]
pub fn py_artefact_get_bytes(py: Python<'_>, uri: &str) -> PyResult<Py<PyAny>> {
    let store = default_store();
    let body = store
        .get(uri)
        .map_err(map_err)?
        .ok_or_else(|| PyIOError::new_err(format!("artefact not found: {uri}")))?;
    let bytes = body
        .into_bytes()
        .map_err(|e| PyIOError::new_err(e.to_string()))?;
    Ok(PyBytes::new(py, &bytes).unbind().into_any())
}

/// List every known artefact, returning a list of :class:`FileRef`.
#[pyfunction(name = "artefact_list")]
pub fn py_artefact_list() -> PyResult<Vec<PyFileRef>> {
    let store = default_store();
    let refs = store
        .list(crate::ArtefactFilter::default())
        .map_err(map_err)?;
    Ok(refs.into_iter().map(PyFileRef::from).collect())
}

/// Register artefact Python classes and helpers on `m`.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyFileRef>()?;
    m.add_function(wrap_pyfunction!(py_artefact_put_file, m)?)?;
    m.add_function(wrap_pyfunction!(py_artefact_put_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(py_artefact_get_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(py_artefact_list, m)?)?;
    Ok(())
}
