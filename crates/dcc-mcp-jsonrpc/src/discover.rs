//! `server/discover` response types for MCP 2026-07-28.
//!
//! `server/discover` replaces the `initialize` / `initialized` handshake that
//! was defined in the 2025-x specs. It is stateless: the server returns its
//! capabilities and no session is created.
//!
//! Reference: ADR-010 / SEP-2575.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Result payload for `server/discover` (MCP 2026-07-28).
///
/// Returned verbatim as the `result` field of a JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDiscoverResult {
    /// Protocol version the server will use for this request.
    pub protocol_version: String,
    pub server_info: DiscoverServerInfo,
    pub capabilities: DiscoverCapabilities,
    /// Optional natural-language instructions for MCP clients / agents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Abbreviated `serverInfo` block (name + version), same shape as the 2025 spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverServerInfo {
    pub name: String,
    pub version: String,
}

/// 2026-07-28 capabilities block.
///
/// The structure is intentionally a superset of the 2025-06-18 `ServerCapabilities`
/// so that new fields can be added without breaking old deserializers. Fields that
/// existed in older specs (`tools`, `resources`, `prompts`, `logging`) keep the
/// same JSON key names; new fields (`tasks`, `tracing`, `caching`) are additions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscoverCapabilities {
    /// Tools capability (unchanged from 2025 spec).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Discover2026ToolsCapability>,
    /// Resources capability (unchanged from 2025 spec).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Discover2026ResourcesCapability>,
    /// Prompts capability (unchanged from 2025 spec).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Discover2026PromptsCapability>,
    /// Tasks — first-class in 2026-07-28 (SEP-2663).
    ///
    /// Present when the server supports `tasks/get` and `tasks/cancel`.
    /// `tasks/list` was removed from the spec; do **not** advertise it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<TasksCapability>,
    /// Vendor-extension capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

/// `tools` capability for 2026-07-28 sessions (same shape as 2025 `ToolsCapability`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Discover2026ToolsCapability {
    pub list_changed: bool,
}

/// `resources` capability for 2026-07-28 sessions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Discover2026ResourcesCapability {
    pub subscribe: bool,
    pub list_changed: bool,
}

/// `prompts` capability for 2026-07-28 sessions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Discover2026PromptsCapability {
    pub list_changed: bool,
}

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
            protocol_version: "2026-07-28".to_string(),
            server_info: DiscoverServerInfo {
                name: "dcc-mcp-core".to_string(),
                version: "0.19.0".to_string(),
            },
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
        };

        let json = serde_json::to_value(&result).unwrap();

        // Top-level keys must be camelCase.
        assert!(
            json.get("protocolVersion").is_some(),
            "expected protocolVersion, got: {json}"
        );
        assert_eq!(json["protocolVersion"], "2026-07-28");
        assert!(json.get("serverInfo").is_some(), "expected serverInfo");
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
            protocol_version: "2026-07-28".to_string(),
            server_info: DiscoverServerInfo {
                name: "test".to_string(),
                version: "0.0.1".to_string(),
            },
            capabilities: DiscoverCapabilities::default(),
            instructions: None,
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
            protocol_version: "2026-07-28".to_string(),
            server_info: DiscoverServerInfo {
                name: "dcc-mcp-core".to_string(),
                version: "0.19.0".to_string(),
            },
            capabilities: DiscoverCapabilities {
                tools: Some(Discover2026ToolsCapability { list_changed: true }),
                resources: None,
                prompts: None,
                tasks: Some(TasksCapability {}),
                experimental: Some(json!({"dcc-mcp": {"compactResponses": true}})),
            },
            instructions: None,
        };
        let json_str = serde_json::to_string(&original).unwrap();
        let recovered: ServerDiscoverResult = serde_json::from_str(&json_str).unwrap();
        assert_eq!(recovered.protocol_version, original.protocol_version);
        assert_eq!(recovered.server_info.name, original.server_info.name);
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
