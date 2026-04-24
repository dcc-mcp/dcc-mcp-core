//! `resources/list`, `resources/read`, `resources/subscribe` message types.

use serde::{Deserialize, Serialize};

/// Single entry returned by `resources/list`.
///
/// Per MCP 2025-03-26, a resource is identified by an opaque URI and
/// carries display metadata. Actual payload is fetched on demand via
/// `resources/read`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Result payload for `resources/list`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListResourcesResult {
    pub resources: Vec<McpResource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Request params for `resources/read`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceParams {
    pub uri: String,
}

/// A single blob returned inside a `ReadResourceResult`.
///
/// Exactly one of `text` or `blob` (base64-encoded bytes) is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContents {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// Result payload for `resources/read`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContents>,
}

/// Request params for `resources/subscribe` / `resources/unsubscribe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeResourceParams {
    pub uri: String,
}

/// Issue #350 — MCP error code for resources that are recognized by
/// URI scheme but whose backing store is not enabled (e.g. `artefact://`
/// before issue #349 wires up the artefact store).
pub const RESOURCE_NOT_ENABLED_ERROR: i64 = -32002;
