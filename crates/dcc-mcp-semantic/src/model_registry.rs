//! Static mapping from string model names to `fastembed::EmbeddingModel`.
//!
//! `fastembed-rs` exposes its catalogue as a Rust enum (`EmbeddingModel`).
//! Adapters configure models by string (env var, MCP arguments, SKILL.md),
//! so we need a deterministic string → enum bridge. Kept in one file so
//! adding a new model is one row + one match arm.
//!
//! The set is intentionally a curated subset of fastembed's full catalogue
//! — the entries that make sense for short technical text (skill names,
//! summaries, tool descriptions) where ~25-100 MB models pay off but
//! multi-GB rerankers do not.

use fastembed::EmbeddingModel;

/// Default model when no override is provided.
///
/// 384-dim, English-focused, ~25 MB quantised on disk. Matches the default
/// used by the Python `OnnxEmbedder` so behaviour is consistent across
/// backends.
pub const DEFAULT_MODEL_NAME: &str = "BAAI/bge-small-en-v1.5";

/// Curated list of supported `(huggingface-name, EmbeddingModel)` pairs.
///
/// Order matters: the first entry is reported as `DEFAULT_MODEL` to Python.
const REGISTRY: &[(&str, EmbeddingModel)] = &[
    ("BAAI/bge-small-en-v1.5", EmbeddingModel::BGESmallENV15),
    ("BAAI/bge-base-en-v1.5", EmbeddingModel::BGEBaseENV15),
    ("BAAI/bge-large-en-v1.5", EmbeddingModel::BGELargeENV15),
    ("BAAI/bge-small-zh-v1.5", EmbeddingModel::BGESmallZHV15),
    (
        "sentence-transformers/all-MiniLM-L6-v2",
        EmbeddingModel::AllMiniLML6V2,
    ),
    (
        "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2",
        EmbeddingModel::ParaphraseMLMiniLML12V2,
    ),
    (
        "nomic-ai/nomic-embed-text-v1.5",
        EmbeddingModel::NomicEmbedTextV15,
    ),
    (
        "intfloat/multilingual-e5-small",
        EmbeddingModel::MultilingualE5Small,
    ),
];

/// Resolve a HuggingFace-style name to fastembed's enum. Returns `None`
/// when the model is not in the curated registry — callers should surface
/// the supported list so studios know what is available.
pub fn lookup(name: &str) -> Option<EmbeddingModel> {
    REGISTRY
        .iter()
        .find(|(canonical, _)| canonical.eq_ignore_ascii_case(name))
        .map(|(_, model)| model.clone())
}

/// Iterator of every supported HuggingFace name in stable order.
pub fn supported_model_names() -> impl Iterator<Item = &'static str> {
    REGISTRY.iter().map(|(name, _)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_present_in_registry() {
        assert!(lookup(DEFAULT_MODEL_NAME).is_some());
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert!(lookup("baai/bge-small-en-v1.5").is_some());
        assert!(lookup("BAAI/BGE-SMALL-EN-V1.5").is_some());
    }

    #[test]
    fn lookup_rejects_unknown_models() {
        assert!(lookup("does-not-exist/model").is_none());
        assert!(lookup("").is_none());
    }

    #[test]
    fn supported_model_names_starts_with_default() {
        let first = supported_model_names().next().expect("registry not empty");
        assert_eq!(first, DEFAULT_MODEL_NAME);
    }
}
