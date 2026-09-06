//! Parse per-request client metadata from the `_meta` field (MCP 2026-07-28).
//!
//! In the 2026-07-28 stateless protocol each request is self-contained.
//! Client identity, protocol version, and capabilities that were previously
//! exchanged during `initialize` are now conveyed in `_meta` on every
//! request.

use serde::Deserialize;
use serde_json::Value;

/// Client information extracted from `_meta` on a 2026-07-28 request.
#[derive(Debug, Clone, Default)]
pub struct RequestMeta {
    /// Protocol version declared by the client (`_meta.protocolVersion`).
    pub protocol_version: Option<String>,
    /// Human-readable client name (`_meta.clientInfo.name`).
    pub client_name: Option<String>,
    /// Client version string (`_meta.clientInfo.version`).
    pub client_version: Option<String>,
    /// Progress token for long-running tool calls (`_meta.progressToken`).
    pub progress_token: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMeta {
    #[serde(default)]
    protocol_version: Option<String>,
    #[serde(default)]
    client_info: Option<RawClientInfo>,
    #[serde(default)]
    progress_token: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RawClientInfo {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

impl RequestMeta {
    /// Extract [`RequestMeta`] from a JSON-RPC `params._meta` value.
    ///
    /// Returns a zeroed struct if `meta` is `None` or cannot be decoded —
    /// stateless requests are valid even without a populated `_meta`.
    pub fn from_value(meta: Option<&Value>) -> Self {
        let Some(meta) = meta else {
            return Self::default();
        };
        let Ok(raw) = serde_json::from_value::<RawMeta>(meta.clone()) else {
            return Self::default();
        };
        Self {
            protocol_version: raw.protocol_version,
            client_name: raw.client_info.as_ref().and_then(|c| c.name.clone()),
            client_version: raw.client_info.as_ref().and_then(|c| c.version.clone()),
            progress_token: raw.progress_token,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_full_meta() {
        let meta = json!({
            "protocolVersion": "2026-07-28",
            "clientInfo": {"name": "TestClient", "version": "1.2.3"},
            "progressToken": "tok-1"
        });
        let m = RequestMeta::from_value(Some(&meta));
        assert_eq!(m.protocol_version.as_deref(), Some("2026-07-28"));
        assert_eq!(m.client_name.as_deref(), Some("TestClient"));
        assert_eq!(m.client_version.as_deref(), Some("1.2.3"));
        assert_eq!(m.progress_token, Some(json!("tok-1")));
    }

    #[test]
    fn handles_missing_meta() {
        let m = RequestMeta::from_value(None);
        assert!(m.protocol_version.is_none());
        assert!(m.client_name.is_none());
    }

    #[test]
    fn handles_partial_meta() {
        let meta = json!({"protocolVersion": "2026-07-28"});
        let m = RequestMeta::from_value(Some(&meta));
        assert_eq!(m.protocol_version.as_deref(), Some("2026-07-28"));
        assert!(m.client_name.is_none());
    }
}
