//! Pure value types for the MCP Resources primitive.
//!
//! Runtime producer traits, registries, subscriptions, and built-in resource
//! producers stay in `dcc-mcp-http`. This module only hosts resource content
//! and error values that can be shared without depending on the HTTP runtime.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use dcc_mcp_jsonrpc::ResourceContents;

/// Content returned by a resource producer implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProducerContent {
    /// UTF-8 text payload (stored in `text`). Typically `application/json`.
    Text {
        /// Full resource URI.
        uri: String,
        /// MIME type for the text payload.
        mime_type: String,
        /// UTF-8 resource body.
        text: String,
    },
    /// Binary payload — serialized as base64 under `blob`.
    Blob {
        /// Full resource URI.
        uri: String,
        /// MIME type for the binary payload.
        mime_type: String,
        /// Raw binary resource body.
        bytes: Vec<u8>,
    },
}

impl ProducerContent {
    /// Convert this producer payload into the JSON-RPC resource content shape.
    #[must_use]
    pub fn into_contents(self) -> ResourceContents {
        match self {
            Self::Text {
                uri,
                mime_type,
                text,
            } => ResourceContents {
                uri,
                mime_type: Some(mime_type),
                text: Some(text),
                blob: None,
            },
            Self::Blob {
                uri,
                mime_type,
                bytes,
            } => ResourceContents {
                uri,
                mime_type: Some(mime_type),
                text: None,
                blob: Some(BASE64_STANDARD.encode(bytes)),
            },
        }
    }
}

/// Error type returned by resource producer implementations.
#[must_use]
#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    /// Requested resource URI is unknown.
    #[error("resource not found: {0}")]
    NotFound(String),
    /// Requested resource scheme is known but currently disabled.
    #[error("resource not enabled: {0}")]
    NotEnabled(String),
    /// Producer failed while reading resource contents.
    #[error("resource read failed: {0}")]
    Read(String),
}

/// Result alias for resource operations.
pub type ResourceResult<T> = Result<T, ResourceError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_error_display_is_stable() {
        assert_eq!(
            ResourceError::NotFound("scene://missing".to_owned()).to_string(),
            "resource not found: scene://missing"
        );
        assert_eq!(
            ResourceError::NotEnabled("artefact://x".to_owned()).to_string(),
            "resource not enabled: artefact://x"
        );
        assert_eq!(
            ResourceError::Read("io".to_owned()).to_string(),
            "resource read failed: io"
        );
    }

    #[test]
    fn text_content_maps_to_resource_contents_text() {
        let contents = ProducerContent::Text {
            uri: "scene://current".to_owned(),
            mime_type: "application/json".to_owned(),
            text: "{}".to_owned(),
        }
        .into_contents();

        assert_eq!(contents.uri, "scene://current");
        assert_eq!(contents.mime_type.as_deref(), Some("application/json"));
        assert_eq!(contents.text.as_deref(), Some("{}"));
        assert_eq!(contents.blob, None);
    }

    #[test]
    fn blob_content_maps_to_base64_resource_contents_blob() {
        let contents = ProducerContent::Blob {
            uri: "capture://current_window".to_owned(),
            mime_type: "image/png".to_owned(),
            bytes: b"png".to_vec(),
        }
        .into_contents();

        assert_eq!(contents.uri, "capture://current_window");
        assert_eq!(contents.mime_type.as_deref(), Some("image/png"));
        assert_eq!(contents.text, None);
        assert_eq!(contents.blob.as_deref(), Some("cG5n"));
    }
}
