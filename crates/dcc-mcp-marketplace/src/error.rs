//! Unified error types for marketplace operations.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MarketplaceError {
    #[error("marketplace source config path could not be resolved: {0}")]
    ConfigPath(String),

    #[error("marketplace source config I/O error for '{0}': {1}")]
    ConfigIo(String, #[source] std::io::Error),

    #[error("marketplace source config parse error for '{0}': {1}")]
    ConfigParse(String, #[source] serde_json::Error),

    #[error("marketplace source fetch failed for '{0}': {1}")]
    Fetch(String, #[source] reqwest::Error),

    #[error("marketplace source read failed for '{0}': {1}")]
    Read(String, #[source] std::io::Error),

    #[error(transparent)]
    Catalog(#[from] dcc_mcp_catalog::CatalogError),

    #[error("marketplace entry '{0}' was not found")]
    NotFound(String),

    #[error("marketplace entry '{0}' does not declare install metadata")]
    MissingInstall(String),

    #[error("marketplace entry '{name}' targets multiple DCCs; pass --dcc")]
    AmbiguousDcc { name: String },

    #[error("marketplace entry '{name}' does not target DCC '{dcc}'")]
    DccMismatch { name: String, dcc: String },

    #[error("marketplace install type '{0}' is not supported yet")]
    UnsupportedInstallType(String),

    #[error("marketplace package '{name}' is already installed for DCC '{dcc}' at '{path}'")]
    AlreadyInstalled {
        name: String,
        dcc: String,
        path: String,
    },

    #[error("marketplace install command failed: {0}")]
    CommandFailed(String),

    #[error("installed package does not contain SKILL.md at '{0}'")]
    MissingSkill(String),

    #[error("marketplace archive SHA-256 mismatch for '{url}': expected {expected}, got {actual}")]
    HashMismatch {
        url: String,
        expected: String,
        actual: String,
    },

    #[error("marketplace archive error for '{0}': {1}")]
    Archive(String, String),

    #[error(
        "invalid marketplace {kind} '{value}'; use only ASCII letters, numbers, '.', '_' or '-'"
    )]
    InvalidPathComponent { kind: String, value: String },
}
