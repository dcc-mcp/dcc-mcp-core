//! PyO3 bindings for the artefact crate (issue #349).
//!
//! Exposes [`FileRef`](crate::FileRef) as a `#[pyclass]` plus helpers
//! `artefact_put_file` and `artefact_get_bytes`. The default store used by
//! the helpers lives as a process-global `OnceLock<FilesystemArtefactStore>`
//! under `<temp_dir>/dcc-mcp-artefacts` so callers outside an
//! `McpHttpServer` can still round-trip artefacts — the MCP server owns its
//! own store and wires it into the `artefact://` resource producer.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes};
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};

use crate::{
    ArtefactError, ArtefactPutOptions, FileRef, FilesystemArtefactStore, SharedArtefactStore,
    atomic_write_bytes, ensure_within_root, hash_bytes_sha256, hash_file_sha256,
};

/// Python wrapper for [`crate::FileRef`].
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "FileRef", module = "dcc_mcp_core._core", skip_from_py_object)]
#[derive(Clone)]
pub struct PyFileRef {
    pub(crate) inner: FileRef,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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

    /// Optional display filename/name for clients.
    #[getter]
    fn display_name(&self) -> Option<&str> {
        self.inner.display_name.as_deref()
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

    /// Tool call/request id that produced the artefact (when known).
    #[getter]
    fn tool_call_id(&self) -> Option<&str> {
        self.inner.tool_call_id.as_deref()
    }

    /// Session id that produced the artefact (when known).
    #[getter]
    fn session_id(&self) -> Option<&str> {
        self.inner.session_id.as_deref()
    }

    /// Adapter-defined correlation id (when known).
    #[getter]
    fn correlation_id(&self) -> Option<&str> {
        self.inner.correlation_id.as_deref()
    }

    /// RFC-3339 creation timestamp.
    #[getter]
    fn created_at(&self) -> String {
        self.inner.created_at.to_rfc3339()
    }

    /// RFC-3339 expiry timestamp when retention is configured.
    #[getter]
    fn expires_at(&self) -> Option<String> {
        self.inner.expires_at.map(|dt| dt.to_rfc3339())
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
        ArtefactError::LimitExceeded(msg) => {
            PyValueError::new_err(format!("artefact limit exceeded: {msg}"))
        }
        ArtefactError::Io(err) => PyIOError::new_err(err.to_string()),
        ArtefactError::Serde(err) => PyValueError::new_err(err.to_string()),
    }
}

fn map_io_err(e: std::io::Error) -> PyErr {
    PyIOError::new_err(e.to_string())
}

fn ensure_bounded(len: usize, max_bytes: Option<usize>, label: &str) -> PyResult<()> {
    if let Some(max_bytes) = max_bytes
        && len > max_bytes
    {
        return Err(PyValueError::new_err(format!(
            "{label} is {len} bytes, exceeding max_bytes={max_bytes}"
        )));
    }
    Ok(())
}

fn path_to_python_string(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
    }
    text.into_owned()
}

/// Atomically write UTF-8 ``text`` to ``path`` and return the path.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction(name = "atomic_write_text")]
#[pyo3(signature = (path, text, create_parents=true, max_bytes=None))]
pub fn py_atomic_write_text(
    path: &str,
    text: &str,
    create_parents: bool,
    max_bytes: Option<usize>,
) -> PyResult<String> {
    let bytes = text.as_bytes();
    ensure_bounded(bytes.len(), max_bytes, "text payload")?;
    atomic_write_bytes(Path::new(path), bytes, create_parents).map_err(map_io_err)?;
    Ok(path.to_string())
}

/// Atomically write ``data`` to ``path`` and return the path.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction(name = "atomic_write_bytes")]
#[pyo3(signature = (path, data, create_parents=true, max_bytes=None))]
pub fn py_atomic_write_bytes(
    path: &str,
    data: Vec<u8>,
    create_parents: bool,
    max_bytes: Option<usize>,
) -> PyResult<String> {
    ensure_bounded(data.len(), max_bytes, "byte payload")?;
    atomic_write_bytes(Path::new(path), &data, create_parents).map_err(map_io_err)?;
    Ok(path.to_string())
}

/// Hash ``data`` with SHA-256 and return the lowercase hex digest.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction(name = "bytes_digest_sha256")]
#[pyo3(signature = (data, max_bytes=None))]
pub fn py_bytes_digest_sha256(data: Vec<u8>, max_bytes: Option<usize>) -> PyResult<String> {
    ensure_bounded(data.len(), max_bytes, "byte payload")?;
    Ok(hash_bytes_sha256(&data))
}

/// Stream-hash a file with SHA-256 and return the lowercase hex digest.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction(name = "file_digest_sha256")]
#[pyo3(signature = (path, max_bytes=None))]
pub fn py_file_digest_sha256(path: &str, max_bytes: Option<u64>) -> PyResult<String> {
    if let Some(max_bytes) = max_bytes {
        let len = std::fs::metadata(path).map_err(map_io_err)?.len();
        if len > max_bytes {
            return Err(PyValueError::new_err(format!(
                "file is {len} bytes, exceeding max_bytes={max_bytes}"
            )));
        }
    }
    hash_file_sha256(Path::new(path)).map_err(map_io_err)
}

/// Resolve ``path`` under ``root`` and reject paths that escape the root.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction(name = "ensure_within_root")]
#[pyo3(signature = (root, path, must_exist=false))]
pub fn py_ensure_within_root(root: &str, path: &str, must_exist: bool) -> PyResult<String> {
    ensure_within_root(Path::new(root), Path::new(path), must_exist)
        .map(|p| path_to_python_string(&p))
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

fn build_put_options(
    mime: Option<String>,
    display_name: Option<String>,
    producer_job_id: Option<String>,
    tool_call_id: Option<String>,
    session_id: Option<String>,
    correlation_id: Option<String>,
    ttl_secs: Option<u64>,
) -> PyResult<ArtefactPutOptions> {
    let producer_job_id = producer_job_id
        .map(|raw| {
            uuid::Uuid::parse_str(&raw).map_err(|e| {
                PyValueError::new_err(format!("invalid producer_job_id UUID {raw:?}: {e}"))
            })
        })
        .transpose()?;
    Ok(ArtefactPutOptions {
        mime,
        display_name,
        producer_job_id,
        tool_call_id,
        session_id,
        correlation_id,
        ttl_secs,
        ..ArtefactPutOptions::default()
    })
}

/// Store the file at ``path`` and return a :class:`FileRef`.
///
/// The default store is a content-addressed filesystem store under
/// ``<temp_dir>/dcc-mcp-artefacts``. ``McpHttpServer`` installs its own
/// store on start, so inside a server process this helper routes to the
/// server-owned store automatically.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction(name = "artefact_put_file")]
#[pyo3(signature = (path, mime=None, display_name=None, producer_job_id=None, tool_call_id=None, session_id=None, correlation_id=None, ttl_secs=None))]
#[allow(clippy::too_many_arguments)]
pub fn py_artefact_put_file(
    path: &str,
    mime: Option<String>,
    display_name: Option<String>,
    producer_job_id: Option<String>,
    tool_call_id: Option<String>,
    session_id: Option<String>,
    correlation_id: Option<String>,
    ttl_secs: Option<u64>,
) -> PyResult<PyFileRef> {
    let store = default_store();
    let options = build_put_options(
        mime,
        display_name,
        producer_job_id,
        tool_call_id,
        session_id,
        correlation_id,
        ttl_secs,
    )?;
    let fr = crate::put_file_with_options(store.as_ref(), PathBuf::from(path), options)
        .map_err(map_err)?;
    Ok(PyFileRef::from(fr))
}

/// Store raw ``bytes`` and return a :class:`FileRef`.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction(name = "artefact_put_bytes")]
#[pyo3(signature = (data, mime=None, display_name=None, producer_job_id=None, tool_call_id=None, session_id=None, correlation_id=None, ttl_secs=None))]
#[allow(clippy::too_many_arguments)]
pub fn py_artefact_put_bytes(
    data: Vec<u8>,
    mime: Option<String>,
    display_name: Option<String>,
    producer_job_id: Option<String>,
    tool_call_id: Option<String>,
    session_id: Option<String>,
    correlation_id: Option<String>,
    ttl_secs: Option<u64>,
) -> PyResult<PyFileRef> {
    let store = default_store();
    let options = build_put_options(
        mime,
        display_name,
        producer_job_id,
        tool_call_id,
        session_id,
        correlation_id,
        ttl_secs,
    )?;
    let fr = crate::put_bytes_with_options(store.as_ref(), data, options).map_err(map_err)?;
    Ok(PyFileRef::from(fr))
}

/// Read back the raw bytes for an ``artefact://`` URI. Raises ``IOError``
/// when the URI is unknown.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
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
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
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
    m.add_function(wrap_pyfunction!(py_atomic_write_text, m)?)?;
    m.add_function(wrap_pyfunction!(py_atomic_write_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(py_bytes_digest_sha256, m)?)?;
    m.add_function(wrap_pyfunction!(py_file_digest_sha256, m)?)?;
    m.add_function(wrap_pyfunction!(py_ensure_within_root, m)?)?;
    m.add_function(wrap_pyfunction!(py_artefact_put_file, m)?)?;
    m.add_function(wrap_pyfunction!(py_artefact_put_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(py_artefact_get_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(py_artefact_list, m)?)?;
    Ok(())
}
