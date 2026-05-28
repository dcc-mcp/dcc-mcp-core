//! Native Rust semantic embeddings via `fastembed-rs` (issue #1395).
//!
//! This crate is the **Rust-native opt-in** companion to the Python-side
//! `OnnxEmbedder` in `dcc-mcp-core`. It exposes a single [`NativeEmbedder`]
//! pyclass that wraps `fastembed::TextEmbedding` and releases the GIL during
//! ONNX inference via [`pyo3::Python::allow_threads`].
//!
//! Shipped as a separate PyPI wheel `dcc-mcp-core-semantic` that the
//! `dcc-mcp-core[semantic]` extra pulls in. The main `dcc-mcp-core` wheel
//! stays free of ONNX Runtime and `fastembed` so adapters that do not need
//! dense semantic recall pay no wheel-size cost.
//!
//! The Python side (`OnnxEmbedder._load_backend`) tries this crate first,
//! falls back to the Python `fastembed` package, and finally raises
//! `EmbedderError` with the install hint.

#[cfg(feature = "fastembed-backend")]
mod embedder;
#[cfg(feature = "fastembed-backend")]
mod model_registry;

#[cfg(feature = "fastembed-backend")]
pub use embedder::EmbedderError;

#[cfg(feature = "python-bindings")]
mod py;

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

/// PyO3 module entrypoint registered as `dcc_mcp_core_semantic._native` by
/// maturin (see `pkg/dcc-mcp-core-semantic/pyproject.toml`).
#[cfg(feature = "python-bindings")]
#[pymodule]
fn _native(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<py::PyNativeEmbedder>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("DEFAULT_MODEL", model_registry::DEFAULT_MODEL_NAME)?;
    let supported: Vec<&'static str> = model_registry::supported_model_names().collect();
    m.add("SUPPORTED_MODELS", supported)?;
    Ok(())
}
