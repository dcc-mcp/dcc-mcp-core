//! Final MCP 2026-07-28 result encoding, separate from legacy wire models.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::ServerInfo;

pub const SERVER_INFO_META_KEY: &str = "io.modelcontextprotocol/serverInfo";

/// Closed set of final-revision `CacheableResult` methods (SEP-2549).
pub const CACHEABLE_RESULT_METHODS: &[&str] = &[
    "server/discover",
    "tools/list",
    "prompts/list",
    "resources/list",
    "resources/templates/list",
    "resources/read",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompleteResultType {
    #[default]
    Complete,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheScope {
    Public,
    #[default]
    Private,
}

/// Complete a modern result without changing business content or vendor metadata.
///
/// Call only at a modern success-response boundary, never for JSON-RPC errors
/// or legacy responses. A tool's `isError: true` is still a completed result.
/// This codec does not implement multi-round-trip input requests; reject a
/// different result kind rather than incorrectly reporting it as complete.
/// Valid handler-authored cache hints and server identity take precedence.
/// Missing or malformed identity falls back to the configured server without
/// changing the completed business result or unrelated vendor metadata.
pub fn complete_modern_result(
    method: &str,
    result: &mut Map<String, Value>,
    server_info: &ServerInfo,
) -> Result<(), &'static str> {
    if result
        .get("resultType")
        .is_some_and(|kind| kind != "complete")
    {
        return Err("Unsupported modern result type");
    }
    if result.get("_meta").is_some_and(|meta| !meta.is_object()) {
        return Err("Modern result metadata must be an object");
    }

    result.insert(
        "resultType".to_string(),
        json!(CompleteResultType::Complete),
    );
    if CACHEABLE_RESULT_METHODS.contains(&method) {
        // JSON clients cannot safely represent integers outside this range.
        const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
        let ttl_ms = result
            .get("ttlMs")
            .and_then(Value::as_u64)
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .unwrap_or(0);
        let scope = match result.get("cacheScope").and_then(Value::as_str) {
            Some("public") => CacheScope::Public,
            _ => CacheScope::Private,
        };
        result.insert("ttlMs".to_string(), json!(ttl_ms));
        result.insert("cacheScope".to_string(), json!(scope));
    }
    let meta = result.entry("_meta").or_insert_with(|| json!({}));
    if let Some(meta) = meta.as_object_mut() {
        let valid_identity = meta
            .get(SERVER_INFO_META_KEY)
            .is_some_and(|value| serde_json::from_value::<ServerInfo>(value.clone()).is_ok());
        if !valid_identity {
            meta.insert(SERVER_INFO_META_KEY.to_string(), json!(server_info));
        }
    }
    Ok(())
}
