//! Runtime resource producer trait and compatibility re-exports.
//!
//! Pure resource value types live in `dcc-mcp-http-types` so clients can share
//! the resource content/error contract without depending on the HTTP runtime
//! crate. The runtime `ResourceProducer` trait stays here because producers are
//! invoked by the tokio-backed MCP server.

use dcc_mcp_jsonrpc::McpResource;

pub use dcc_mcp_http_types::resources::{ProducerContent, ResourceError, ResourceResult};

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
