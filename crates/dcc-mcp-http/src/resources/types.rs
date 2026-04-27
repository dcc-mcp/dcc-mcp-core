//! Trait + value types shared by [`crate::resources`] producers.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use crate::protocol::{McpResource, ResourceContents};

/// Content returned by a [`ResourceProducer`].
pub enum ProducerContent {
    /// UTF-8 text payload (stored in `text`). Typically `application/json`.
    Text {
        uri: String,
        mime_type: String,
        text: String,
    },
    /// Binary payload — serialized as base64 under `blob`.
    Blob {
        uri: String,
        mime_type: String,
        bytes: Vec<u8>,
    },
}

impl ProducerContent {
    pub(crate) fn into_contents(self) -> ResourceContents {
        match self {
            ProducerContent::Text {
                uri,
                mime_type,
                text,
            } => ResourceContents {
                uri,
                mime_type: Some(mime_type),
                text: Some(text),
                blob: None,
            },
            ProducerContent::Blob {
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

/// Error type returned by [`ResourceProducer::read`].
#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("resource not enabled: {0}")]
    NotEnabled(String),
    #[error("resource read failed: {0}")]
    Read(String),
}

pub type ResourceResult<T> = Result<T, ResourceError>;

/// A URI-scheme-keyed producer of MCP resources.
///
/// Implementations must be `Send + Sync` because the MCP server calls
/// them from any tokio worker thread.
pub trait ResourceProducer: Send + Sync {
    /// Human-readable URI scheme (e.g. `"scene"`, `"capture"`). Used to
    /// dispatch `resources/read` by scheme.
    fn scheme(&self) -> &str;

    /// Resources this producer surfaces in `resources/list`. May return an
    /// empty vector to hide the producer while keeping the scheme
    /// registered (useful for feature-flagged producers).
    fn list(&self) -> Vec<McpResource>;

    /// Read a resource by full URI.
    fn read(&self, uri: &str) -> ResourceResult<ProducerContent>;
}

/// Extract the scheme portion (everything before the first `:`) of a URI.
pub(crate) fn uri_scheme(uri: &str) -> Option<&str> {
    let idx = uri.find(':')?;
    Some(&uri[..idx])
}
