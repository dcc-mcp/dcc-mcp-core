//! Local FileRegistry inventory for `dcc-mcp-cli list`.
//!
//! This is the local-mode inventory path: it reads the same core default
//! registry directory used by sidecars and gateway runners, prunes dead rows
//! via `FileRegistry::read_alive`, and formats the result like the gateway
//! `/v1/instances` payload.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::application::local_instance;

pub fn list_local_instances(registry_dir: PathBuf) -> anyhow::Result<Value> {
    let (entries, evicted) = local_instance::live_dcc_entries(&registry_dir)?;
    let mut instances: Vec<_> = entries
        .into_iter()
        .map(local_instance::instance_to_value)
        .collect::<anyhow::Result<Vec<_>>>()?;
    instances.sort_by(|left, right| {
        let left_key = list_sort_key(left);
        let right_key = list_sort_key(right);
        left_key.cmp(&right_key)
    });

    Ok(json!({
        "total": instances.len(),
        "instances": instances,
        "source": "local_registry",
        "registry_dir": registry_dir,
        "evicted": evicted,
        "gateway": {
            "current": {
                "name": "local",
                "role": "local",
                "registry_dir": registry_dir,
            },
            "candidates": [],
            "source": "local_registry"
        }
    }))
}

fn list_sort_key(value: &Value) -> (String, String) {
    let dcc = value
        .get("dcc_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let id = value
        .get("instance_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    (dcc, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;
    use dcc_mcp_transport::discovery::types::ServiceEntry;

    #[test]
    fn local_list_formats_registry_rows() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
        entry.display_name = Some("Maya-Rig".to_string());
        registry.register(entry).unwrap();

        let payload = list_local_instances(dir.path().to_path_buf()).unwrap();

        assert_eq!(payload["source"], "local_registry");
        assert_eq!(payload["total"], 1);
        assert_eq!(payload["instances"][0]["dcc_type"], "maya");
        assert_eq!(
            payload["instances"][0]["mcp_url"],
            "http://127.0.0.1:18080/mcp"
        );
    }
}
