//! Pure-Rust embedder wrapper around `fastembed::TextEmbedding`.
//!
//! Sits below the PyO3 layer so the core logic (model resolution, init,
//! batched inference) is testable in plain Rust without the Python
//! interpreter. The PyO3 wrapper in `src/py.rs` is a thin shim that
//! forwards to this type and releases the GIL during inference.

use std::path::PathBuf;

use fastembed::{InitOptions, TextEmbedding};
use thiserror::Error;

use crate::model_registry;

/// Errors surfaced to the Python side.
///
/// PyO3 maps this to a Python `RuntimeError` so the higher-level
/// `OnnxEmbedder` wrapper in `dcc-mcp-core` can wrap it in its own
/// `EmbedderError`. Keep variants stable — Python tests pattern-match
/// on the messages.
#[derive(Debug, Error)]
pub enum EmbedderError {
    #[error("unknown embedding model {requested:?}; supported models: {supported}")]
    UnknownModel {
        requested: String,
        supported: String,
    },
    #[error("fastembed load failed for {model:?}: {source}")]
    Load {
        model: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("embed failed: {source}")]
    Embed {
        #[source]
        source: anyhow::Error,
    },
}

/// Native embedder wrapping a single loaded `fastembed::TextEmbedding`.
///
/// Thread-safe by way of `fastembed::TextEmbedding`'s own `Send + Sync`
/// guarantees — the PyO3 wrapper releases the GIL before each call so
/// concurrent embeddings from multiple Python threads do not serialise
/// on the interpreter.
pub struct NativeEmbedder {
    model: TextEmbedding,
    model_name: String,
    cache_dir: Option<PathBuf>,
    dim: usize,
}

impl NativeEmbedder {
    /// Construct an embedder from a HuggingFace-style model name plus an
    /// optional cache directory. When `cache_dir` is `None`, fastembed
    /// writes to its own platform-default (`~/.cache/fastembed/`).
    ///
    /// Runs a one-off probe embedding to discover the model's output
    /// dimensionality; failures during the probe surface as
    /// [`EmbedderError::Embed`] so the caller can decide to fall back.
    pub fn try_new(
        model_name: impl Into<String>,
        cache_dir: Option<PathBuf>,
    ) -> Result<Self, EmbedderError> {
        let model_name = model_name.into();
        let resolved = model_registry::lookup(&model_name).ok_or_else(|| {
            let supported: Vec<&str> = model_registry::supported_model_names().collect();
            EmbedderError::UnknownModel {
                requested: model_name.clone(),
                supported: supported.join(", "),
            }
        })?;

        let mut opts = InitOptions::new(resolved).with_show_download_progress(false);
        if let Some(dir) = cache_dir.as_ref() {
            opts = opts.with_cache_dir(dir.clone());
        }

        let model = TextEmbedding::try_new(opts).map_err(|err| EmbedderError::Load {
            model: model_name.clone(),
            source: anyhow::Error::msg(err.to_string()),
        })?;

        let probe = model
            .embed(vec!["dimension probe".to_string()], None)
            .map_err(|err| EmbedderError::Embed {
                source: anyhow::Error::msg(err.to_string()),
            })?;
        let dim = probe.first().map(Vec::len).unwrap_or(0);

        Ok(Self {
            model,
            model_name,
            cache_dir,
            dim,
        })
    }

    /// Output vector dimensionality. Determined by the loaded model.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// HuggingFace-style model identifier this embedder was loaded with.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Cache directory the model was loaded from (or `None` for fastembed's
    /// platform default).
    pub fn cache_dir(&self) -> Option<&PathBuf> {
        self.cache_dir.as_ref()
    }

    /// Embed a single text. Empty / whitespace-only inputs short-circuit
    /// to a zero vector of length [`Self::dim`] without invoking the model.
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedderError> {
        if text.trim().is_empty() {
            return Ok(vec![0.0; self.dim]);
        }
        let mut out = self
            .model
            .embed(vec![text.to_string()], None)
            .map_err(|err| EmbedderError::Embed {
                source: anyhow::Error::msg(err.to_string()),
            })?;
        Ok(out.pop().unwrap_or_default())
    }

    /// Batch embed. Empty inputs in the batch are padded with zero vectors
    /// in the corresponding output positions; non-empty inputs are sent to
    /// fastembed in one batch call so the ONNX session is invoked once.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedderError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        // Filter empties so fastembed sees only non-empty rows; remember
        // the original indices to restore zero padding afterwards.
        let mut nonempty: Vec<String> = Vec::with_capacity(texts.len());
        let mut nonempty_idx: Vec<usize> = Vec::with_capacity(texts.len());
        for (idx, text) in texts.iter().enumerate() {
            if !text.trim().is_empty() {
                nonempty.push(text.clone());
                nonempty_idx.push(idx);
            }
        }

        let mut out = vec![vec![0.0; self.dim]; texts.len()];
        if nonempty.is_empty() {
            return Ok(out);
        }

        let raw = self
            .model
            .embed(nonempty, None)
            .map_err(|err| EmbedderError::Embed {
                source: anyhow::Error::msg(err.to_string()),
            })?;
        for (vec, original_idx) in raw.into_iter().zip(nonempty_idx) {
            out[original_idx] = vec;
        }
        Ok(out)
    }
}
