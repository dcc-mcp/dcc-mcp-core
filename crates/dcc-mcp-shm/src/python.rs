//! PyO3 Python bindings for dcc-mcp-shm.
//!
//! Exposes:
//!  - `PySharedBuffer`   — wraps [`SharedBuffer`] (with TTL support)
//!  - `gc_orphans`        — module-level function to clean up stale segments
//!  - `PyBufferPool`     — wraps [`BufferPool`]
//!  - `PySceneDataKind`  — enum mirror of [`SceneDataKind`]
//!  - `PySharedSceneBuffer` — wraps [`SharedSceneBuffer`]

use std::time::Duration;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::buffer::{BufferDescriptor, SharedBuffer, gc_orphans};
use crate::error::ShmError;
use crate::pool::{BufferPool, PooledBuffer};
use crate::scene::{SceneDataKind, SharedSceneBuffer};
use uuid::Uuid;

// ── Error conversion ─────────────────────────────────────────────────────────

fn to_py(e: ShmError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

// ── PySharedBuffer ────────────────────────────────────────────────────────────

/// A named, fixed-capacity shared memory buffer backed by an ipckit
/// shared memory segment.
///
/// Usage::
///
///     from dcc_mcp_core import PySharedBuffer
///
///     buf = PySharedBuffer.create(capacity=1024 * 1024)  # 1 MiB
///     buf.write(b"vertex data")
///     data = buf.read()
///
///     # With TTL (auto-expire after 60 seconds)
///     buf = PySharedBuffer.create(capacity=1024, ttl_secs=60)
///     if buf.is_expired():
///         print("buffer has expired")
#[pyclass(name = "PySharedBuffer")]
pub struct PySharedBuffer {
    inner: SharedBuffer,
    /// Keeps the pool slot marked as in-use until this Python object is GC'd.
    _pool_guard: Option<PooledBuffer>,
}

#[pymethods]
impl PySharedBuffer {
    /// Create a new buffer with the given capacity (bytes).
    ///
    /// If ``ttl_secs`` is > 0 the buffer will be considered expired after
    /// that many seconds since creation, enabling automatic cleanup via
    /// :func:`gc_orphans`.
    #[staticmethod]
    #[pyo3(signature = (capacity, ttl_secs=0))]
    fn create(capacity: usize, ttl_secs: u64) -> PyResult<Self> {
        let ttl = if ttl_secs > 0 {
            Some(Duration::from_secs(ttl_secs))
        } else {
            None
        };
        SharedBuffer::create_with_ttl(Uuid::new_v4().to_string(), capacity, ttl)
            .map(|inner| Self {
                inner,
                _pool_guard: None,
            })
            .map_err(to_py)
    }

    /// Open an existing buffer from an ipckit segment name and id.
    #[staticmethod]
    fn open(name: &str, id: &str) -> PyResult<Self> {
        SharedBuffer::open(name, id)
            .map(|inner| Self {
                inner,
                _pool_guard: None,
            })
            .map_err(to_py)
    }

    /// Write bytes into the buffer. Returns the number of bytes written.
    fn write(&self, data: &[u8]) -> PyResult<usize> {
        self.inner.write(data).map_err(to_py)
    }

    /// Read the current data from the buffer.
    fn read(&self) -> PyResult<Vec<u8>> {
        self.inner.read().map_err(to_py)
    }

    /// Return the number of bytes currently stored.
    fn data_len(&self) -> PyResult<usize> {
        self.inner.data_len().map_err(to_py)
    }

    /// Return the maximum number of bytes this buffer can hold.
    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Clear the buffer (reset data_len to 0).
    fn clear(&self) -> PyResult<()> {
        self.inner.clear().map_err(to_py)
    }

    /// Buffer id (string).
    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }

    /// ipckit segment name of the backing shared memory.
    fn name(&self) -> String {
        self.inner.name()
    }

    /// Returns True if this buffer's TTL has expired.
    ///
    /// Buffers without a TTL (``ttl_secs == 0``) never expire.
    fn is_expired(&self) -> PyResult<bool> {
        self.inner.is_expired().map_err(to_py)
    }

    /// Return a JSON descriptor string for cross-process handoff.
    fn descriptor_json(&self) -> PyResult<String> {
        BufferDescriptor::from_buffer(&self.inner)
            .map_err(to_py)?
            .to_json()
            .map_err(to_py)
    }

    fn __repr__(&self) -> String {
        format!(
            "PySharedBuffer(id={:?}, capacity={}, ttl={})",
            self.inner.id,
            self.inner.capacity(),
            "?" // TTL is behind a lock; skip in repr for speed
        )
    }
}

// ── PyBufferPool ──────────────────────────────────────────────────────────────

/// A fixed-capacity pool of reusable shared memory buffers.
///
/// Usage::
///
///     pool = PyBufferPool(capacity=4, buffer_size=1024 * 1024)
///     buf = pool.acquire()
///     buf.write(b"scene snapshot")
///     # buf automatically returned on GC / explicit del
#[pyclass(name = "PyBufferPool")]
pub struct PyBufferPool {
    inner: BufferPool,
}

#[pymethods]
impl PyBufferPool {
    #[new]
    fn new(capacity: usize, buffer_size: usize) -> PyResult<Self> {
        BufferPool::new(capacity, buffer_size)
            .map(|inner| Self { inner })
            .map_err(to_py)
    }

    /// Acquire a free buffer.  Raises ``RuntimeError`` if all slots are in use.
    ///
    /// The pool slot remains marked as in-use until the returned buffer object
    /// is garbage-collected by Python.
    fn acquire(&self) -> PyResult<PySharedBuffer> {
        let guard = self.inner.acquire().map_err(to_py)?;
        let inner = guard.buffer.clone();
        Ok(PySharedBuffer {
            inner,
            _pool_guard: Some(guard),
        })
    }

    /// Number of currently available (free) slots.
    fn available(&self) -> usize {
        self.inner.available()
    }

    /// Total pool capacity.
    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Per-buffer size in bytes.
    fn buffer_size(&self) -> usize {
        self.inner.buffer_size()
    }

    fn __repr__(&self) -> String {
        format!(
            "PyBufferPool(capacity={}, available={}, buffer_size={})",
            self.inner.capacity(),
            self.inner.available(),
            self.inner.buffer_size()
        )
    }
}

// ── PySceneDataKind ───────────────────────────────────────────────────────────

/// Kind of DCC scene data stored in a shared scene buffer.
#[pyclass(name = "PySceneDataKind", eq, eq_int, from_py_object)]
#[derive(Clone, PartialEq)]
pub enum PySceneDataKind {
    Geometry = 0,
    AnimationCache = 1,
    Screenshot = 2,
    Arbitrary = 3,
}

impl From<PySceneDataKind> for SceneDataKind {
    fn from(k: PySceneDataKind) -> Self {
        match k {
            PySceneDataKind::Geometry => SceneDataKind::Geometry,
            PySceneDataKind::AnimationCache => SceneDataKind::AnimationCache,
            PySceneDataKind::Screenshot => SceneDataKind::Screenshot,
            PySceneDataKind::Arbitrary => SceneDataKind::Arbitrary,
        }
    }
}

// ── PySharedSceneBuffer ───────────────────────────────────────────────────────

/// High-level shared scene buffer for zero-copy DCC ↔ Agent data exchange.
///
/// Usage::
///
///     ssb = PySharedSceneBuffer.write(
///         data=vertex_bytes,
///         kind=PySceneDataKind.Geometry,
///         source_dcc="Maya",
///         use_compression=True,
///     )
///     desc_json = ssb.descriptor_json()
///     # Send desc_json to the consumer via IPC…
///
///     # Consumer side:
///     recovered = ssb.read()
#[pyclass(name = "PySharedSceneBuffer")]
pub struct PySharedSceneBuffer {
    inner: SharedSceneBuffer,
}

#[pymethods]
impl PySharedSceneBuffer {
    /// Write data into a new shared scene buffer.
    ///
    /// Parameters
    /// ----------
    /// data : bytes
    ///     Raw payload to store.
    /// kind : PySceneDataKind
    ///     Semantic kind of the data.
    /// source_dcc : str | None
    ///     Name of the originating DCC application.
    /// use_compression : bool
    ///     Whether to apply LZ4 compression before writing.
    #[staticmethod]
    #[pyo3(signature = (data, kind=PySceneDataKind::Arbitrary, source_dcc=None, use_compression=false))]
    fn write(
        data: &[u8],
        kind: PySceneDataKind,
        source_dcc: Option<String>,
        use_compression: bool,
    ) -> PyResult<Self> {
        SharedSceneBuffer::write(data, kind.into(), source_dcc, use_compression)
            .map(|inner| Self { inner })
            .map_err(to_py)
    }

    /// Read the stored data back (decompresses automatically if needed).
    fn read(&self) -> PyResult<Vec<u8>> {
        self.inner.read().map_err(to_py)
    }

    /// Transfer id (UUID string).
    #[getter]
    fn id(&self) -> &str {
        &self.inner.meta.id
    }

    /// Total original byte count.
    #[getter]
    fn total_bytes(&self) -> usize {
        self.inner.meta.total_bytes
    }

    /// Whether data is stored in a single inline buffer.
    #[getter]
    fn is_inline(&self) -> bool {
        self.inner.is_inline()
    }

    /// Whether data spans multiple chunks.
    #[getter]
    fn is_chunked(&self) -> bool {
        self.inner.is_chunked()
    }

    /// JSON descriptor for cross-process handoff.
    fn descriptor_json(&self) -> PyResult<String> {
        self.inner.to_descriptor_json().map_err(to_py)
    }

    fn __repr__(&self) -> String {
        format!(
            "PySharedSceneBuffer(id={:?}, total_bytes={}, inline={})",
            self.inner.meta.id,
            self.inner.meta.total_bytes,
            self.inner.is_inline()
        )
    }
}

// ── Module registration ───────────────────────────────────────────────────────

/// Scan for and remove stale ``dcc_shm_*`` shared memory segments.
///
/// On Linux this scans ``/dev/shm``; on macOS it scans ``/tmp``;
/// on Windows it is a no-op (the OS reclaims named file-mappings on
/// last close).
///
/// Returns the number of segments removed.
///
/// Parameters
/// ----------
/// max_age_secs : float
///     Minimum age (in seconds) for a segment to be considered stale.
///     Segments whose TTL has expired **or** whose creation time is older
///     than ``max_age_secs`` are removed.
#[pyfunction]
#[pyo3(signature = (max_age_secs,), name = "gc_orphans")]
fn py_gc_orphans(max_age_secs: f64) -> usize {
    gc_orphans(Duration::from_secs_f64(max_age_secs))
}

/// Register all PyO3 classes from this crate into `m`.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySharedBuffer>()?;
    m.add_class::<PyBufferPool>()?;
    m.add_class::<PySceneDataKind>()?;
    m.add_class::<PySharedSceneBuffer>()?;
    m.add_function(wrap_pyfunction!(py_gc_orphans, m)?)?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // PyO3 classes require an interpreter; structural tests only.
    use super::*;

    #[test]
    fn test_py_scene_data_kind_conversion() {
        assert_eq!(
            SceneDataKind::from(PySceneDataKind::Geometry),
            SceneDataKind::Geometry
        );
        assert_eq!(
            SceneDataKind::from(PySceneDataKind::AnimationCache),
            SceneDataKind::AnimationCache
        );
        assert_eq!(
            SceneDataKind::from(PySceneDataKind::Screenshot),
            SceneDataKind::Screenshot
        );
        assert_eq!(
            SceneDataKind::from(PySceneDataKind::Arbitrary),
            SceneDataKind::Arbitrary
        );
    }
}
