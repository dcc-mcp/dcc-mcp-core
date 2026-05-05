use thiserror::Error;

/// Errors returned by the catalog loader.
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog I/O error for '{0}': {1}")]
    Io(String, #[source] std::io::Error),

    #[error("catalog YAML parse error: {0}")]
    Parse(String),
}
