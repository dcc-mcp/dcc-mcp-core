use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── InstanceConfig ─────────────────────────────────────────────────────────

/// DCC instance registration metadata.
///
/// One of the orthogonal sub-configs that compose `McpHttpConfig`
/// (issue #852). Reported into the shared `FileRegistry` so the
/// gateway can route by DCC type / version / scene. Captured here
/// as a pure value type — every field is plain string-shaped,
/// nothing carries runtime state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstanceConfig {
    /// Optional PID of an external DCC host whose lifetime bounds this server.
    ///
    /// Embedded and standalone servers leave this unset because the registry
    /// owner sentinel already represents their lifetime. Sidecars and other
    /// out-of-process adapters set it so a surviving service cannot keep a
    /// closed DCC instance discoverable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_pid: Option<u32>,

    /// DCC application type (e.g. `"maya"`, `"blender"`). Reported in
    /// the shared `FileRegistry` so the gateway can route by DCC
    /// type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,

    /// DCC application version (e.g. `"2025.1"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcc_version: Option<String>,

    /// Currently open scene/file. Improves routing accuracy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,

    /// Arbitrary instance metadata recorded in `FileRegistry`.
    ///
    /// Rez/package launchers use this for context-bundle fields such
    /// as `context_bundle`, `production_domain`, `context_kind`,
    /// `project`, `task`, `toolset_profile`, and `package_provenance`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub instance_metadata: HashMap<String, String>,

    /// Capabilities declared by the DCC adapter hosting this server
    /// (issue #354).
    ///
    /// Each tool may list `required_capabilities` in its sibling
    /// `tools.yaml`; on `tools/call` the server intersects the
    /// tool's requirements against this declared set. Missing
    /// capabilities surface as a `-32001 capability_missing` MCP
    /// error. Tools with unmet capabilities still appear in
    /// `tools/list` but carry `_meta.dcc.missing_capabilities = [...]`
    /// so clients can filter.
    ///
    /// The list is freeform — conventionally lowercase dotted
    /// identifiers like `"usd"`, `"scene.mutate"`,
    /// `"filesystem.read"`. Adapters hard-code it at construction
    /// time; there is no runtime introspection of the DCC.
    ///
    /// Default: empty (no capabilities declared — any tool with
    /// declared requirements will report them as missing).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_capabilities: Vec<String>,
}
