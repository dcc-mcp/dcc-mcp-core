use super::{
    SERVICE_ENTRY_LEGACY_SCHEMA_VERSION, SERVICE_ENTRY_SCHEMA_VERSION, ServiceEntry, ServiceStatus,
};
use crate::error::TransportError;

#[test]
fn service_entry_new_uses_current_schema() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    assert_eq!(entry.schema_version, SERVICE_ENTRY_SCHEMA_VERSION);
    assert_eq!(entry.dcc_type, "maya");
    assert_eq!(entry.host, "127.0.0.1");
    assert_eq!(entry.port, 18812);
    assert_eq!(entry.status, ServiceStatus::Available);
    assert!(entry.version.is_none());
    assert!(entry.scene.is_none());
    assert!(entry.transport_address.is_none());
    assert!(entry.extras.is_empty());
    assert_eq!(entry.pid, Some(std::process::id()));
}

#[test]
fn legacy_json_defaults_to_schema_zero() {
    let entry = ServiceEntry::new("photoshop", "127.0.0.1", 18813);
    let mut value = serde_json::to_value(entry).unwrap();
    value.as_object_mut().unwrap().remove("schema_version");

    let parsed: ServiceEntry = serde_json::from_value(value).unwrap();

    assert_eq!(parsed.schema_version, SERVICE_ENTRY_LEGACY_SCHEMA_VERSION);
    assert!(parsed.validate_schema_version().is_ok());
}

#[test]
fn current_schema_roundtrips() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: ServiceEntry = serde_json::from_str(&json).unwrap();

    assert!(json.contains("\"schema_version\":1"));
    assert_eq!(parsed.schema_version, SERVICE_ENTRY_SCHEMA_VERSION);
    assert!(parsed.validate_schema_version().is_ok());
}

#[test]
fn future_schema_is_rejected() {
    let mut entry = ServiceEntry::new("blender", "127.0.0.1", 18814);
    entry.schema_version = SERVICE_ENTRY_SCHEMA_VERSION + 1;

    assert!(matches!(
        entry.validate_schema_version(),
        Err(TransportError::UnsupportedServiceEntrySchemaVersion {
            received: 2,
            supported: SERVICE_ENTRY_SCHEMA_VERSION,
        })
    ));
}
