//! `server/discover` response types for MCP 2026-07-28.
//!
//! `server/discover` replaces the `initialize` / `initialized` handshake that
//! was defined in the 2025-x specs. It is stateless: the server returns its
//! capabilities and no session is created.
//!
//! Reference: ADR-010 / SEP-2575.

pub use crate::envelope::{StatelessClientInfo, StatelessRequestMeta};
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

/// Extension identifier for the MCP Skills extension.
///
/// A server that declares it MUST implement `skills/list` and `skills/get`,
/// and MUST also declare the `resources` capability.
pub const SKILLS_EXTENSION_ID: &str = "io.modelcontextprotocol/skills";

/// The `extensions` object of [`StatelessServerCapabilities`].
///
/// Keys are extension identifiers; each value is that extension's own
/// settings object. Unknown extensions deserialize into `None` here rather
/// than failing, so a server that does not implement an extension can still
/// parse a peer's capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerExtensionsCapability {
    /// `io.modelcontextprotocol/skills` — Skills over MCP (ext-skills).
    #[serde(
        rename = "io.modelcontextprotocol/skills",
        skip_serializing_if = "Option::is_none"
    )]
    pub skills: Option<SkillsExtensionCapability>,
}

impl ServerExtensionsCapability {
    /// Return `true` when no extension is declared and the object should be
    /// omitted from the wire entirely.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_none()
    }
}

/// Settings for the [`SKILLS_EXTENSION_ID`] extension.
///
/// An empty object (all defaults) indicates support with no optional
/// features.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct SkillsExtensionCapability {
    /// The server implements `resources/directory/read`. Clients MUST NOT
    /// call that method when this is `false`.
    pub directory_read: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StatelessServerCapabilities;
    use serde_json::json;

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
                extensions: None,
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

    // ── Skills extension declaration ──────────────────────────────────────

    #[test]
    fn skills_extension_serialises_under_its_dotted_identifier() {
        let caps = StatelessServerCapabilities {
            resources: Some(Discover2026ResourcesCapability::default()),
            extensions: Some(ServerExtensionsCapability {
                skills: Some(SkillsExtensionCapability {
                    directory_read: true,
                }),
            }),
            ..Default::default()
        };
        let json = serde_json::to_value(&caps).unwrap();

        let skills = &json["extensions"][SKILLS_EXTENSION_ID];
        assert_eq!(skills["directoryRead"], true, "got: {json}");
        assert!(json["extensions"].get("skills").is_none());
    }

    #[test]
    fn skills_extension_default_omits_optional_settings_object() {
        let caps = StatelessServerCapabilities {
            extensions: Some(ServerExtensionsCapability::default()),
            ..Default::default()
        };
        let json = serde_json::to_value(&caps).unwrap();
        // An empty extensions object is still a valid declaration.
        assert_eq!(json["extensions"], json!({}), "got: {json}");
        assert!(ServerExtensionsCapability::default().is_empty());
    }

    #[test]
    fn undeclared_extensions_are_omitted_from_the_wire() {
        let caps = StatelessServerCapabilities::default();
        let json = serde_json::to_value(&caps).unwrap();
        assert!(json.get("extensions").is_none(), "got: {json}");
    }

    #[test]
    fn skills_extension_round_trips() {
        let original = ServerExtensionsCapability {
            skills: Some(SkillsExtensionCapability {
                directory_read: true,
            }),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: ServerExtensionsCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.skills, original.skills);
    }

    #[test]
    fn unknown_extensions_deserialize_without_failing() {
        let caps: StatelessServerCapabilities = serde_json::from_str(
            r#"{"extensions": {"io.modelcontextprotocol/ui": {"version": "1"}}}"#,
        )
        .expect("unknown extensions must not break parsing");
        assert!(caps.extensions.is_some());
        assert!(caps.extensions.unwrap().skills.is_none());
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
                extensions: None,
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
}
