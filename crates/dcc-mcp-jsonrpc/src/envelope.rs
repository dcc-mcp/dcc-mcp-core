//! Canonical, validated per-request metadata for MCP 2026-07-28.

use serde::{Deserialize, Deserializer, Serialize, de::Error};
use serde_json::{Map, Value};

use crate::envelope_validation::validate_optional_fields;

pub const PROTOCOL_VERSION_META_KEY: &str = "io.modelcontextprotocol/protocolVersion";
pub const CLIENT_INFO_META_KEY: &str = "io.modelcontextprotocol/clientInfo";
pub const CLIENT_CAPABILITIES_META_KEY: &str = "io.modelcontextprotocol/clientCapabilities";
pub const LOG_LEVEL_META_KEY: &str = "io.modelcontextprotocol/logLevel";
pub const REQUEST_ENVELOPE_KEYS: &[&str] = &[
    PROTOCOL_VERSION_META_KEY,
    CLIENT_INFO_META_KEY,
    CLIENT_CAPABILITIES_META_KEY,
    LOG_LEVEL_META_KEY,
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnvelopeIssue {
    pub key: String,
    pub problem: String,
}

impl EnvelopeIssue {
    pub(crate) fn new(key: impl Into<String>, problem: &str) -> Self {
        Self {
            key: key.into(),
            problem: problem.into(),
        }
    }
}

/// Required protocol version and capabilities, plus optional client identity.
///
/// Deserialization and `parse` share the same final-revision validation. The
/// unqualified RC keys do not satisfy required namespaced fields. Unknown
/// metadata is retained without being interpreted as a protocol capability.
#[derive(Debug, Clone, Serialize)]
pub struct StatelessRequestMeta {
    #[serde(rename = "io.modelcontextprotocol/protocolVersion")]
    pub protocol_version: String,
    #[serde(rename = "io.modelcontextprotocol/clientCapabilities")]
    pub client_capabilities: Map<String, Value>,
    #[serde(
        rename = "io.modelcontextprotocol/clientInfo",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_info: Option<StatelessClientInfo>,
    #[serde(
        rename = "io.modelcontextprotocol/logLevel",
        skip_serializing_if = "Option::is_none"
    )]
    pub log_level: Option<String>,
    #[serde(rename = "progressToken", skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatelessClientInfo {
    pub name: String,
    pub version: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl StatelessRequestMeta {
    pub fn parse(value: Option<&Value>) -> Result<Self, EnvelopeIssue> {
        let meta = value
            .and_then(Value::as_object)
            .ok_or_else(|| EnvelopeIssue::new("_meta", "expected an object"))?;
        // Required-key errors precede present-but-malformed optional fields.
        for key in [PROTOCOL_VERSION_META_KEY, CLIENT_CAPABILITIES_META_KEY] {
            if !meta.contains_key(key) {
                return Err(EnvelopeIssue::new(key, "missing"));
            }
        }
        let protocol_version = meta[PROTOCOL_VERSION_META_KEY]
            .as_str()
            .ok_or_else(|| EnvelopeIssue::new(PROTOCOL_VERSION_META_KEY, "expected a string"))?;
        let capabilities = meta[CLIENT_CAPABILITIES_META_KEY]
            .as_object()
            .ok_or_else(|| {
                EnvelopeIssue::new(CLIENT_CAPABILITIES_META_KEY, "expected an object")
            })?;
        validate_optional_fields(meta, capabilities)?;
        let client_info = meta
            .get(CLIENT_INFO_META_KEY)
            .map(|value| {
                serde_json::from_value(value.clone())
                    .map_err(|_| EnvelopeIssue::new(CLIENT_INFO_META_KEY, "invalid implementation"))
            })
            .transpose()?;
        let mut extra = meta.clone();
        for key in REQUEST_ENVELOPE_KEYS
            .iter()
            .copied()
            .chain(["progressToken"])
        {
            extra.remove(key);
        }
        Ok(Self {
            protocol_version: protocol_version.into(),
            client_capabilities: capabilities.clone(),
            client_info,
            log_level: meta
                .get(LOG_LEVEL_META_KEY)
                .and_then(Value::as_str)
                .map(str::to_owned),
            progress_token: meta.get("progressToken").cloned(),
            extra,
        })
    }
}

impl<'de> Deserialize<'de> for StatelessRequestMeta {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::parse(Some(&value))
            .map_err(|issue| D::Error::custom(format!("{}: {}", issue.key, issue.problem)))
    }
}

/// Detect only the reserved version claim, including a malformed claim value.
pub fn has_modern_envelope_claim(body: &Value) -> bool {
    body.get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(Value::as_object)
        .is_some_and(|meta| meta.contains_key(PROTOCOL_VERSION_META_KEY))
}

/// Lift reserved protocol context away from business metadata, preserving all
/// vendor keys (including progress and tracing). Does not traverse arguments.
pub fn strip_request_envelope(params: &mut Value) {
    if let Some(meta) = params.get_mut("_meta").and_then(Value::as_object_mut) {
        for key in REQUEST_ENVELOPE_KEYS {
            meta.remove(*key);
        }
    }
}
