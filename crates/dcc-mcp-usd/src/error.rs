//! Error types for the `dcc-mcp-usd` crate.

use thiserror::Error;

/// Errors that can occur when working with USD scenes.
#[derive(Debug, Error)]
pub enum UsdError {
    /// A required prim path was not found in the stage.
    #[error("prim not found: {0}")]
    PrimNotFound(String),

    /// An attribute was not found on the prim.
    #[error("attribute not found: {attr} on prim {prim}")]
    AttributeNotFound { prim: String, attr: String },

    /// Layer not found.
    #[error("layer not found: {0}")]
    LayerNotFound(String),

    /// Invalid path format.
    #[error("invalid USD path: {0}")]
    InvalidPath(String),

    /// Serialization / deserialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Scene is read-only.
    #[error("scene is read-only")]
    ReadOnly,

    /// Invalid USDA syntax when parsing a layer string.
    #[error("USDA parse error at line {line}: {message}")]
    ParseError { line: usize, message: String },

    /// Conversion between DCC formats failed.
    #[error("scene conversion error: {0}")]
    ConversionError(String),

    /// Generic I/O error.
    #[error("I/O error: {0}")]
    Io(String),
}

/// Result alias for USD operations.
pub type UsdResult<T> = Result<T, UsdError>;

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_display {
        use super::*;

        #[test]
        fn prim_not_found_display() {
            let err = UsdError::PrimNotFound("/World/Sphere".to_string());
            let s = err.to_string();
            assert!(s.contains("/World/Sphere"), "{s}");
        }

        #[test]
        fn attribute_not_found_display() {
            let err = UsdError::AttributeNotFound {
                prim: "/World/Mesh".to_string(),
                attr: "points".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("/World/Mesh"), "{s}");
            assert!(s.contains("points"), "{s}");
        }

        #[test]
        fn layer_not_found_display() {
            let err = UsdError::LayerNotFound("anon:0x1".to_string());
            let s = err.to_string();
            assert!(s.contains("anon:0x1"), "{s}");
        }

        #[test]
        fn invalid_path_display() {
            let err = UsdError::InvalidPath("not/a/valid/path".to_string());
            let s = err.to_string();
            assert!(s.contains("not/a/valid/path"), "{s}");
        }

        #[test]
        fn serialization_display() {
            let json_err: serde_json::Error =
                serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
            let err = UsdError::Serialization(json_err);
            let s = err.to_string();
            assert!(!s.is_empty(), "display must not be empty");
        }

        #[test]
        fn read_only_display() {
            let err = UsdError::ReadOnly;
            let s = err.to_string();
            assert!(s.contains("read-only"), "{s}");
        }

        #[test]
        fn parse_error_display() {
            let err = UsdError::ParseError {
                line: 42,
                message: "unexpected token".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("42"), "{s}");
            assert!(s.contains("unexpected token"), "{s}");
        }

        #[test]
        fn conversion_error_display() {
            let err = UsdError::ConversionError("unsupported geometry type".to_string());
            let s = err.to_string();
            assert!(s.contains("unsupported geometry type"), "{s}");
        }

        #[test]
        fn io_display() {
            let err = UsdError::Io("file not found".to_string());
            let s = err.to_string();
            assert!(s.contains("file not found"), "{s}");
        }
    }

    mod test_from {
        use super::*;

        #[test]
        fn from_serde_json_error() {
            let json_err: serde_json::Error =
                serde_json::from_str::<serde_json::Value>("[unclosed").unwrap_err();
            let err: UsdError = json_err.into();
            assert!(matches!(err, UsdError::Serialization(_)));
        }
    }

    mod test_debug {
        use super::*;

        #[test]
        fn all_variants_are_debug() {
            let json_err: serde_json::Error =
                serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
            let variants = vec![
                UsdError::PrimNotFound("/a".to_string()),
                UsdError::AttributeNotFound {
                    prim: "p".to_string(),
                    attr: "a".to_string(),
                },
                UsdError::LayerNotFound("l".to_string()),
                UsdError::InvalidPath("i".to_string()),
                UsdError::Serialization(json_err),
                UsdError::ReadOnly,
                UsdError::ParseError {
                    line: 1,
                    message: "m".to_string(),
                },
                UsdError::ConversionError("c".to_string()),
                UsdError::Io("i".to_string()),
            ];
            for v in &variants {
                assert!(!format!("{v:?}").is_empty());
            }
        }
    }
}
