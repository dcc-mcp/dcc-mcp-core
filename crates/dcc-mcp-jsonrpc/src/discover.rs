//! `server/discover` JSON-RPC method for stateless MCP clients (ADR-010).
//!
//! Stateless clients do not perform the session lifecycle (`initialize`
//! → negotiate protocol → session management). Instead, they call
//! `server/discover` once to learn the server's capabilities and endpoint
//! routing table. The protocol version is carried in the
//! `MCP-Protocol-Version` HTTP header, not in the request body.

use serde::{Deserialize, Serialize};

/// JSON-RPC method name for `server/discover`.
pub const DISCOVER_METHOD: &str = "server/discover";

/// Parameters for the `server/discover` request (currently empty — protocol
/// version is negotiated via the `MCP-Protocol-Version` header).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoverParams {}

/// Result of `server/discover` — returns available endpoints and capabilities.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoverResult {
    /// The protocol version the server is speaking.
    pub protocol_version: String,

    /// Available JSON-RPC method names.
    pub methods: Vec<String>,

    /// Server information (mirrors `initialize` ServerInfo).
    pub server_info: DiscoverServerInfo,
}

/// Server identity returned by `server/discover`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoverServerInfo {
    pub name: String,
    pub version: String,
}
