use thiserror::Error;

/// Errors returned by the catalog loader.
#[must_use]
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog I/O error for '{0}': {1}")]
    Io(String, #[source] std::io::Error),

    #[error("catalog YAML parse error: {0}")]
    Parse(String),
}

/// Errors returned when catalog entries fail JSON Schema validation.
#[must_use]
#[derive(Debug, Error)]
pub enum CatalogValidationError {
    /// A single entry failed validation.
    #[error("entry '{name}' failed validation:\n{message}")]
    ValidationFailed { name: String, message: String },

    /// Multiple entries failed validation.
    #[error("schema validation failed for {count} entry(s)")]
    MultipleFailures {
        count: usize,
        failures: Vec<CatalogValidationError>,
    },

    /// The marketplace.json document itself caused a schema error.
    #[error("schema validation error: {0}")]
    SchemaError(String),
}
