//! `server/discover` response types for MCP 2026-07-28.
//!
//! `server/discover` replaces the `initialize` / `initialized` handshake that
//! was defined in the 2025-x specs. It is stateless: the server returns its
//! capabilities and no session is created.
//!
//! Reference: ADR-010 / SEP-2575.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::modern::{CacheScope, CompleteResultType};

// Keep the historical public names without maintaining duplicate wire types.
pub use crate::lifecycle::{
    PromptsCapability as Discover2026PromptsCapability,
    ResourcesCapability as Discover2026ResourcesCapability, ServerInfo as DiscoverServerInfo,
    StatelessServerCapabilities as DiscoverCapabilities,
    ToolsCapability as Discover2026ToolsCapability,
};

/// Result payload for `server/discover` (MCP 2026-07-28).
///
/// Returned verbatim as the `result` field of a JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    /// Protocol versions supported by this endpoint, newest first.
    pub supported_versions: Vec<String>,
    pub capabilities: DiscoverCapabilities,
    /// Optional natural-language instructions for MCP clients / agents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Discovery always returns a complete result, never an input request.
    pub result_type: CompleteResultType,
    /// Conservative defaults do not permit stale cross-principal discovery.
    pub ttl_ms: u64,
    pub cache_scope: CacheScope,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Map<String, Value>>,
}

/// Compatibility name for the single canonical final-revision discovery type.
pub type ServerDiscoverResult = DiscoverResult;

/// `tasks` capability — new in MCP 2026-07-28, SEP-2663.
///
/// An empty struct signals that `tasks/get` and `tasks/cancel` are supported.
/// There are no sub-fields in the initial spec.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TasksCapability {}

/// Per-request `_meta` block for MCP 2026-07-28 stateless requests.
///
/// In 2026-07-28, every request is self-contained: session context (client
/// info, capabilities, protocol version) is carried in `params._meta` rather
/// than being established once during `initialize`. All fields are optional so
/// that older clients that do not send them continue to parse successfully.
///
/// SEP-2575.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatelessRequestMeta {
    /// Declared protocol version of the client for this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    /// Free-form client identification (name + version).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_info: Option<StatelessClientInfo>,
    /// Client capabilities for this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_capabilities: Option<Value>,
    /// Progress token for streaming / `InputRequiredResult` callbacks (SEP-2260).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<Value>,
    /// W3C Trace Context `traceparent` header value (SEP-414).
    ///
    /// When present the server SHOULD propagate it to downstream calls and
    /// include it in diagnostic logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    /// W3C Trace Context `tracestate` header value (SEP-414).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracestate: Option<String>,
}

/// Minimal client identification carried in `_meta.clientInfo` (2026-07-28).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatelessClientInfo {
    pub name: String,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    // ── ServerDiscoverResult round-trip ─────────────────────────────────────

    #[test]
    fn server_discover_result_serialises_to_camel_case() {
        let result = ServerDiscoverResult {
            supported_versions: vec!["2026-07-28".to_string()],
            capabilities: DiscoverCapabilities {
                tools: Some(Discover2026ToolsCapability { list_changed: true }),
                resources: Some(Discover2026ResourcesCapability {
                    subscribe: true,
                    list_changed: true,
                }),
                prompts: Some(Discover2026PromptsCapability { list_changed: true }),
                tasks: Some(TasksCapability {}),
                experimental: None,
            },
            instructions: Some("Direct DCC workflow".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_value(&result).unwrap();

        // Top-level keys must be camelCase.
        assert!(
            json.get("supportedVersions").is_some(),
            "expected supportedVersions, got: {json}"
        );
        assert_eq!(json["supportedVersions"], json!(["2026-07-28"]));
        assert!(json.get("protocolVersion").is_none());
        assert!(json.get("serverInfo").is_none());
        assert_eq!(json["resultType"], "complete");
        assert_eq!(json["ttlMs"], 0);
        assert_eq!(json["cacheScope"], "private");
        assert!(json.get("capabilities").is_some(), "expected capabilities");
        assert!(json.get("instructions").is_some(), "expected instructions");

        // Capabilities sub-keys.
        let caps = &json["capabilities"];
        assert!(caps.get("tools").is_some());
        assert!(caps.get("tasks").is_some());
        assert_eq!(caps["tools"]["listChanged"], true);
    }

    #[test]
    fn server_discover_result_optional_fields_omitted_when_none() {
        let result = ServerDiscoverResult {
            supported_versions: vec!["2026-07-28".to_string()],
            capabilities: DiscoverCapabilities::default(),
            instructions: None,
            ..Default::default()
        };

        let json = serde_json::to_value(&result).unwrap();
        // `instructions` must be absent (not null) when None.
        assert!(
            json.get("instructions").is_none(),
            "instructions should be omitted, got: {json}"
        );
        // Empty capabilities should produce an empty object.
        let caps = &json["capabilities"];
        assert_eq!(*caps, json!({}), "empty caps must be {{}}");
    }

    #[test]
    fn server_discover_result_roundtrip() {
        let original = ServerDiscoverResult {
            supported_versions: vec!["2026-07-28".to_string()],
            capabilities: DiscoverCapabilities {
                tools: Some(Discover2026ToolsCapability { list_changed: true }),
                resources: None,
                prompts: None,
                tasks: Some(TasksCapability {}),
                experimental: Some(json!({"dcc-mcp": {"compactResponses": true}})),
            },
            instructions: None,
            ..Default::default()
        };
        let json_str = serde_json::to_string(&original).unwrap();
        let recovered: ServerDiscoverResult = serde_json::from_str(&json_str).unwrap();
        assert_eq!(recovered.supported_versions, original.supported_versions);
        assert_eq!(recovered.result_type, CompleteResultType::Complete);
        assert!(recovered.capabilities.tools.is_some());
        assert!(recovered.capabilities.tasks.is_some());
        assert!(recovered.capabilities.resources.is_none());
    }

    // ── StatelessRequestMeta round-trip ─────────────────────────────────────

    #[test]
    fn stateless_request_meta_all_optional_omitted_by_default() {
        let meta = StatelessRequestMeta::default();
        let json = serde_json::to_value(&meta).unwrap();
        // All fields are None, so the serialised object must be empty.
        assert_eq!(json, json!({}), "default meta must serialise to {{}}");
    }

    #[test]
    fn stateless_request_meta_roundtrip_with_all_fields() {
        let meta = StatelessRequestMeta {
            protocol_version: Some("2026-07-28".to_string()),
            client_info: Some(StatelessClientInfo {
                name: "test-client".to_string(),
                version: "1.0".to_string(),
            }),
            client_capabilities: Some(json!({"sampling": {}})),
            progress_token: Some(Value::String("tok-abc".to_string())),
            traceparent: Some("00-trace-id-span-00".to_string()),
            tracestate: Some("vendor=abc".to_string()),
        };

        let json_str = serde_json::to_string(&meta).unwrap();
        let recovered: StatelessRequestMeta = serde_json::from_str(&json_str).unwrap();

        assert_eq!(recovered.protocol_version.as_deref(), Some("2026-07-28"));
        assert_eq!(
            recovered.client_info.as_ref().map(|i| i.name.as_str()),
            Some("test-client")
        );
        assert_eq!(
            recovered.traceparent.as_deref(),
            Some("00-trace-id-span-00")
        );
    }

    #[test]
    fn stateless_request_meta_serialises_client_info_as_camel_case() {
        let meta = StatelessRequestMeta {
            protocol_version: Some("2026-07-28".to_string()),
            client_info: Some(StatelessClientInfo {
                name: "MyCLI".to_string(),
                version: "2.0".to_string(),
            }),
            ..Default::default()
        };
        let json = serde_json::to_value(&meta).unwrap();
        // Top-level key must be camelCase.
        assert!(
            json.get("clientInfo").is_some(),
            "expected clientInfo key, got: {json}"
        );
        assert_eq!(json["protocolVersion"], "2026-07-28");
    }
}
