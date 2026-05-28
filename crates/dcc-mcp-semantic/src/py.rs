//! PyO3 wrapper around [`crate::embedder::NativeEmbedder`].
//!
//! Two responsibilities:
//!
//! 1. Convert between Python types (`Option<String>`, `Vec<String>`,
//!    `Vec<f32>`) and the Rust core.
//! 2. Release the GIL via [`pyo3::Python::allow_threads`] around every
//!    fastembed call so concurrent Python threads can drive multiple
//!    embedders (or other Python work) without serialising on the GIL
//!    during ONNX inference.

use std::path::PathBuf;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::embedder::{EmbedderError, NativeEmbedder};

fn map_err(err: EmbedderError) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}

/// `dcc_mcp_core_semantic._native.NativeEmbedder` — opaque handle holding
/// a loaded fastembed model. Wrap once and reuse for many embeddings;
/// model load + dimension probe is the expensive bit and only runs at
/// construction.
#[pyclass(name = "NativeEmbedder", module = "dcc_mcp_core_semantic._native")]
pub struct PyNativeEmbedder {
    inner: NativeEmbedder,
}

#[pymethods]
impl PyNativeEmbedder {
    /// Construct an embedder from a model name plus an optional cache
    /// directory. Both default to fastembed's defaults (BGE-small EN v1.5
    /// and `~/.cache/fastembed/`).
    #[new]
    #[pyo3(signature = (model_name=None, cache_dir=None))]
    fn new(model_name: Option<String>, cache_dir: Option<String>) -> PyResult<Self> {
        let resolved_name =
            model_name.unwrap_or_else(|| crate::model_registry::DEFAULT_MODEL_NAME.to_string());
        let resolved_cache = cache_dir.map(PathBuf::from);
        let inner = NativeEmbedder::try_new(resolved_name, resolved_cache).map_err(map_err)?;
        Ok(Self { inner })
    }

    /// Output vector dimensionality. Property to match the Python
    /// `Embedder` Protocol shape.
    #[getter]
    fn dim(&self) -> usize {
        self.inner.dim()
    }

    /// HuggingFace-style model name this embedder was loaded with.
    #[getter]
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    /// Cache directory the model was loaded from, or `None` for fastembed's
    /// platform default.
    #[getter]
    fn cache_dir(&self) -> Option<String> {
        self.inner
            .cache_dir()
            .and_then(|p| p.to_str().map(str::to_owned))
    }

    /// Embed a single string. Returns a Python list of floats so the
    /// Python-side wrapper can convert to `array.array("d", ...)` without
    /// going through numpy.
    fn embed(&self, py: Python<'_>, text: &str) -> PyResult<Vec<f32>> {
        py.allow_threads(|| self.inner.embed_one(text))
            .map_err(map_err)
    }

    /// Batch embed. Equivalent to calling :meth:`embed` per input but
    /// invokes the ONNX session once for the whole batch.
    fn embed_batch(&self, py: Python<'_>, texts: Vec<String>) -> PyResult<Vec<Vec<f32>>> {
        py.allow_threads(|| self.inner.embed_batch(&texts))
            .map_err(map_err)
    }

    /// Friendly repr for diagnostics; mirrors the Python OnnxEmbedder shape.
    fn __repr__(&self) -> String {
        format!(
            "NativeEmbedder(model_name={:?}, dim={}, cache_dir={:?})",
            self.inner.model_name(),
            self.inner.dim(),
            self.inner.cache_dir(),
        )
    }
}
