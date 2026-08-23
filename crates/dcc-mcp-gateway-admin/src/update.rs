//! Instance update version resolution for admin endpoints.

use dcc_mcp_transport::discovery::types::{SERVER_BINARY_VERSION_METADATA_KEY, ServiceEntry};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminInstanceUpdateVersion {
    Known {
        current: String,
        display: Option<String>,
        source: &'static str,
    },
    MissingCurrentVersion,
}

#[must_use]
pub fn admin_instance_update_version(
    instance: &ServiceEntry,
    binary_name: &str,
    requested_current_version: Option<&str>,
) -> AdminInstanceUpdateVersion {
    if let Some(version) = requested_current_version
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return AdminInstanceUpdateVersion::Known {
            current: version.to_string(),
            display: Some(version.to_string()),
            source: "request",
        };
    }
    if binary_name == "dcc-mcp-server"
        && let Some((version, source)) = instance_server_binary_version(instance)
    {
        return AdminInstanceUpdateVersion::Known {
            current: version.clone(),
            display: Some(version),
            source,
        };
    }
    AdminInstanceUpdateVersion::MissingCurrentVersion
}

#[must_use]
pub fn instance_server_binary_version(instance: &ServiceEntry) -> Option<(String, &'static str)> {
    const SERVER_VERSION_KEYS: [&str; 3] = [
        SERVER_BINARY_VERSION_METADATA_KEY,
        "server_binary_version",
        "server_version",
    ];

    for key in SERVER_VERSION_KEYS {
        if let Some(version) = instance
            .metadata
            .get(key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some((version.to_string(), "instance_metadata"));
        }
    }
    for key in SERVER_VERSION_KEYS {
        if let Some(version) = instance
            .extras
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some((version.to_string(), "instance_extras"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_server_version_is_used_but_dcc_versions_are_not() {
        let mut instance = ServiceEntry::new("maya", "127.0.0.1", 18813);
        instance.version = Some("2024.0".to_string());
        instance.adapter_version = Some("0.3.0".to_string());
        assert_eq!(
            admin_instance_update_version(&instance, "dcc-mcp-server", None),
            AdminInstanceUpdateVersion::MissingCurrentVersion
        );
        instance.metadata.insert(
            SERVER_BINARY_VERSION_METADATA_KEY.to_string(),
            "0.18.0".to_string(),
        );
        assert_eq!(
            admin_instance_update_version(&instance, "dcc-mcp-server", None),
            AdminInstanceUpdateVersion::Known {
                current: "0.18.0".to_string(),
                display: Some("0.18.0".to_string()),
                source: "instance_metadata",
            }
        );
    }

    #[test]
    fn explicit_request_wins_over_registry_metadata() {
        let mut instance = ServiceEntry::new("blender", "127.0.0.1", 18814);
        instance.metadata.insert(
            SERVER_BINARY_VERSION_METADATA_KEY.to_string(),
            "0.18.0".to_string(),
        );
        assert_eq!(
            admin_instance_update_version(&instance, "dcc-mcp-server", Some(" 0.20.8 ")),
            AdminInstanceUpdateVersion::Known {
                current: "0.20.8".to_string(),
                display: Some("0.20.8".to_string()),
                source: "request",
            }
        );
    }
}
