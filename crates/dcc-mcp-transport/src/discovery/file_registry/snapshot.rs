//! Non-mutating registry inspection; runtime repair remains in FileRegistry.

use std::{fs, io::ErrorKind, path::Path};

use super::super::registry_format::{RegistryParseError, parse_registry_entries};
use super::{ServiceEntry, TransportError, TransportResult};

pub(super) fn read_snapshot(path: &Path) -> TransportResult<Vec<ServiceEntry>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    parse_registry_entries(&content).map_err(|error| match error {
        // Parser messages can echo private registry values. Keep location and category only.
        RegistryParseError::Json(error) => TransportError::Serialization(format!(
            "invalid registry JSON ({:?}) at line {}, column {}",
            error.classify(),
            error.line(),
            error.column()
        )),
        RegistryParseError::UnsupportedSchema {
            received,
            supported,
        } => TransportError::UnsupportedServiceEntrySchemaVersion {
            received,
            supported,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_never_creates_a_missing_registry() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("missing");
        assert!(
            super::super::FileRegistry::read_snapshot(&dir)
                .unwrap()
                .is_empty()
        );
        assert!(!dir.exists());
    }

    #[test]
    fn snapshot_preserves_invalid_files_without_quarantine_or_lock() {
        for content in [
            "private-broken-json",
            "\0\0\0",
            "[{\"dcc_type\":\"private-value\"}]",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("services.json");
            fs::write(&path, content).unwrap();
            let error = read_snapshot(&path).unwrap_err();
            assert!(!error.to_string().contains("private-"));
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn snapshot_reports_io_failure_instead_of_empty_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("services.json");
        fs::create_dir(&path).unwrap();
        assert!(matches!(read_snapshot(&path), Err(TransportError::Io(_))));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn snapshot_keeps_dead_rows_and_legacy_rows_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("services.json");
        let mut entry = ServiceEntry::new("houdini", "127.0.0.1", 18080);
        entry.pid = Some(u32::MAX);
        let mut row = serde_json::to_value(entry).unwrap();
        row.as_object_mut().unwrap().remove("schema_version");
        let content = serde_json::to_string(&vec![row]).unwrap();
        fs::write(&path, &content).unwrap();
        assert_eq!(read_snapshot(&path).unwrap().len(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn snapshot_does_not_open_the_write_lock_and_rejects_future_schema() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("services.lock")).unwrap();
        let path = dir.path().join("services.json");
        fs::write(&path, "[]").unwrap();
        assert!(read_snapshot(&path).unwrap().is_empty());
        let content = "[{\"schema_version\":18446744073709551615}]";
        fs::write(&path, content).unwrap();
        assert!(matches!(
            read_snapshot(&path),
            Err(TransportError::UnsupportedServiceEntrySchemaVersion { .. })
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
