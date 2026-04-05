//! PyO3 Python bindings for dcc-mcp-shm.
//!
//! Exposes:
//!  - `PySharedBuffer`   — wraps [`SharedBuffer`]
//!  - `PyBufferPool`     — wraps [`BufferPool`]
//!  - `PySceneDataKind`  — enum mirror of [`SceneDataKind`]
//!  - `PySharedSceneBuffer` — wraps [`SharedSceneBuffer`]

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::buffer::{BufferDescriptor, SharedBuffer};
use crate::error::ShmError;
use crate::pool::{BufferPool, PooledBuffer};
use crate::scene::{SceneDataKind, SharedSceneBuffer};

// ── Error conversion ─────────────────────────────────────────────────────────

fn to_py(e: ShmError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

// ── PySharedBuffer ────────────────────────────────────────────────────────────

/// A named, fixed-capacity shared memory buffer backed by a memory-mapped
/// file.
///
/// Usage::
///
///     from dcc_mcp_core import PySharedBuffer
///
///     buf = PySharedBuffer.create(capacity=1024 * 1024)  # 1 MiB
///     buf.write(b"vertex data")
///     data = buf.read()
#[pyclass(name = "PySharedBuffer")]
pub struct PySharedBuffer {
    inner: SharedBuffer,
    /// Keeps the pool slot marked as in-use until this Python object is GC'd.
    _pool_guard: Option<PooledBuffer>,
}

#[pymethods]
impl PySharedBuffer {
    /// Create a new buffer with the given capacity (bytes).
    #[staticmethod]
    fn create(capacity: usize) -> PyResult<Self> {
        SharedBuffer::create(capacity)
            .map(|inner| Self {
                inner,
                _pool_guard: None,
            })
            .map_err(to_py)
    }

    /// Open an existing buffer from a file path and id.
    #[staticmethod]
    fn open(path: &str, id: &str) -> PyResult<Self> {
        SharedBuffer::open(path, id)
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

    /// File path of the backing memory-mapped file.
    fn path(&self) -> String {
        self.inner.path().to_string_lossy().into_owned()
    }

    /// Return a JSON descriptor string for cross-process handoff.
    fn descriptor_json(&self) -> PyResult<String> {
        BufferDescriptor::from_buffer(&self.inner)
            .to_json()
            .map_err(to_py)
    }

    fn __repr__(&self) -> String {
        format!(
            "PySharedBuffer(id={:?}, capacity={})",
            self.inner.id,
            self.inner.capacity()
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

/// Register all PyO3 classes from this crate into `m`.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySharedBuffer>()?;
    m.add_class::<PyBufferPool>()?;
    m.add_class::<PySceneDataKind>()?;
    m.add_class::<PySharedSceneBuffer>()?;
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
