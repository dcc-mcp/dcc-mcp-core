//! Shared capability projection for registry instance rows.

use std::collections::BTreeSet;

use dcc_mcp_gateway_core::capability::IndexSnapshot;
use dcc_mcp_gateway_core::policy::{GatewayPolicy, GatewayPolicyOperation};
use dcc_mcp_transport::discovery::types::ServiceEntry;
use serde_json::{Value, json};

const CAPABILITY_INDEX_SOURCE: &str = "capability_index";

#[must_use]
pub fn capabilities_json(
    entry: &ServiceEntry,
    snapshot: &IndexSnapshot,
    policy: &GatewayPolicy,
) -> Value {
    let mut loaded_skills = BTreeSet::new();
    let mut tool_count = 0_usize;
    for record in snapshot
        .records
        .iter()
        .filter(|record| record.instance_id == entry.instance_id)
        .filter(|record| {
            policy
                .enforce_record(GatewayPolicyOperation::Search, record)
                .is_ok()
        })
        .filter(|record| record.loaded && !record.discovery_only)
    {
        tool_count += 1;
        if let Some(skill_name) = record.skill_name.as_deref()
            && !skill_name.trim().is_empty()
        {
            loaded_skills.insert(skill_name.to_owned());
        }
    }

    let loaded_skills: Vec<String> = loaded_skills.into_iter().collect();
    json!({
        "loaded_skills": loaded_skills,
        "loaded_skill_count": loaded_skills.len(),
        "tool_count": tool_count,
        "source": CAPABILITY_INDEX_SOURCE,
    })
}

#[must_use]
pub fn transport_ready_json(row: &Value) -> Value {
    let endpoint = row
        .get("mcp_url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let transport_ready = matches!(
        row.get("status").and_then(Value::as_str),
        Some("available" | "busy")
    );
    let ready = endpoint.is_some()
        && transport_ready
        && row.get("stale").and_then(Value::as_bool) == Some(false);

    json!({
        "ready": ready,
        "has_endpoint": endpoint.is_some(),
        "status": row.get("status").cloned().unwrap_or(Value::Null),
        "stale": row.get("stale").cloned().unwrap_or(Value::Null),
    })
}

#[must_use]
pub fn dispatch_ready_json(row: &Value) -> Value {
    row.get("dispatch")
        .and_then(|dispatch| dispatch.get("ready"))
        .cloned()
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::capability::{CapabilityIndex, CapabilityRecord, InstanceFingerprint};

    fn record(
        instance_id: uuid::Uuid,
        skill: Option<&str>,
        tool: &str,
        loaded: bool,
    ) -> CapabilityRecord {
        CapabilityRecord::new(
            format!("houdini.{instance_id}.{tool}"),
            tool.to_owned(),
            tool.to_owned(),
            skill.map(str::to_owned),
            tool,
            Vec::new(),
            "houdini".to_owned(),
            instance_id,
            true,
            loaded,
            None,
        )
    }

    #[test]
    fn capabilities_are_instance_scoped_loaded_and_deduplicated() {
        let entry = ServiceEntry::new("houdini", "127.0.0.1", 18812);
        let other = ServiceEntry::new("houdini", "127.0.0.1", 18813);
        let index = CapabilityIndex::new();
        index.upsert_instance(
            entry.instance_id,
            vec![
                record(entry.instance_id, Some("fx"), "run_python", true),
                record(entry.instance_id, Some("fx"), "save_scene", true),
                record(entry.instance_id, Some("scene"), "open_scene", false),
            ],
            InstanceFingerprint(1),
        );
        index.upsert_instance(
            other.instance_id,
            vec![record(other.instance_id, Some("other"), "render", true)],
            InstanceFingerprint(2),
        );

        let projected = capabilities_json(&entry, &index.snapshot(), &GatewayPolicy::default());

        assert_eq!(projected["loaded_skills"], json!(["fx"]));
        assert_eq!(projected["loaded_skill_count"], 1);
        assert_eq!(projected["tool_count"], 2);
        assert_eq!(projected["source"], "capability_index");
    }

    #[test]
    fn capabilities_hide_policy_denied_records() {
        let entry = ServiceEntry::new("photoshop", "127.0.0.1", 18812);
        let index = CapabilityIndex::new();
        index.set_unloaded_records(vec![record(
            entry.instance_id,
            Some("render"),
            "batch_render",
            true,
        )]);
        let policy: GatewayPolicy = serde_json::from_value(json!({
            "allowed_skill_names": ["compositing"],
        }))
        .unwrap();

        let projected = capabilities_json(&entry, &index.snapshot(), &policy);

        assert_eq!(projected["loaded_skills"], json!([]));
        assert_eq!(projected["tool_count"], 0);
    }

    #[test]
    fn transport_and_dispatch_readiness_mirror_effective_row() {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let row =
            crate::gateway::state::entry_to_json(&entry, std::time::Duration::from_secs(30), None);
        let transport = transport_ready_json(&row);
        assert_eq!(transport["ready"], true);
        assert_eq!(transport["has_endpoint"], true);
        assert_eq!(dispatch_ready_json(&row), Value::Null);

        entry
            .metadata
            .insert("dispatch_status".into(), "ready".into());
        let row =
            crate::gateway::state::entry_to_json(&entry, std::time::Duration::from_secs(30), None);
        assert_eq!(dispatch_ready_json(&row), row["dispatch"]["ready"]);

        entry
            .extras
            .insert("mcp_url".into(), json!("http://127.0.0.1:19999/mcp"));
        let row =
            crate::gateway::state::entry_to_json(&entry, std::time::Duration::from_secs(30), None);
        assert_eq!(row["mcp_url"], "http://127.0.0.1:18812/mcp");
        assert_eq!(transport_ready_json(&row)["has_endpoint"], true);
    }

    #[test]
    fn transport_ready_accepts_busy_and_rejects_missing_endpoint() {
        let busy = json!({
            "mcp_url": "http://127.0.0.1:18812/mcp",
            "status": "busy",
            "stale": false,
        });
        assert_eq!(transport_ready_json(&busy)["ready"], true);

        let empty_endpoint = json!({
            "mcp_url": "   ",
            "status": "available",
            "stale": false,
        });
        assert_eq!(transport_ready_json(&empty_endpoint)["ready"], false);
        assert_eq!(transport_ready_json(&empty_endpoint)["has_endpoint"], false);
    }
}
