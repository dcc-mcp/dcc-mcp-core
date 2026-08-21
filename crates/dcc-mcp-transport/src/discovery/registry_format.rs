//! On-disk service-registry parsing and legacy normalization.

use uuid::Uuid;

use super::types::{
    GATEWAY_SENTINEL_DCC_TYPE, SERVICE_ENTRY_LEGACY_SCHEMA_VERSION, SERVICE_ENTRY_SCHEMA_VERSION,
    ServiceEntry,
};

#[derive(Debug)]
pub(super) enum RegistryParseError {
    Json(serde_json::Error),
    UnsupportedSchema { received: u64, supported: u16 },
}

pub(super) fn parse_registry_entries(
    content: &str,
) -> Result<Vec<ServiceEntry>, RegistryParseError> {
    let values = serde_json::from_str::<Vec<serde_json::Value>>(content)
        .map_err(RegistryParseError::Json)?;
    values
        .into_iter()
        .map(|value| {
            if let Some(received) = value
                .as_object()
                .and_then(|row| row.get("schema_version"))
                .and_then(serde_json::Value::as_u64)
                && received > u64::from(SERVICE_ENTRY_SCHEMA_VERSION)
            {
                return Err(RegistryParseError::UnsupportedSchema {
                    received,
                    supported: SERVICE_ENTRY_SCHEMA_VERSION,
                });
            }

            serde_json::from_value(value.clone())
                .or_else(|error| legacy_gateway_sentinel(&value).ok_or(error))
                .map_err(RegistryParseError::Json)
        })
        .collect()
}

fn legacy_gateway_sentinel(value: &serde_json::Value) -> Option<ServiceEntry> {
    let row = value.as_object()?;
    (row.get("dcc_type")?.as_str()? == GATEWAY_SENTINEL_DCC_TYPE).then_some(())?;
    let host = row.get("host")?.as_str()?;
    let port = u16::try_from(row.get("port")?.as_u64()?).ok()?;
    let mut entry = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, host, port);
    entry.instance_id = Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("dcc-mcp://gateway/{host}:{port}").as_bytes(),
    );
    let defaults = serde_json::to_value(entry).ok()?;
    let mut normalized = value.clone();
    let normalized_row = normalized.as_object_mut()?;
    normalized_row
        .entry("schema_version".to_string())
        .or_insert(serde_json::json!(SERVICE_ENTRY_LEGACY_SCHEMA_VERSION));
    for (key, default) in defaults.as_object()? {
        normalized_row.entry(key.clone()).or_insert(default.clone());
    }
    serde_json::from_value(normalized).ok()
}
