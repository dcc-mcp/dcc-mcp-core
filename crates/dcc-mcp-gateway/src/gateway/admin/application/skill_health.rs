//! Gateway adapters for backend-neutral skill health projections.

use std::sync::Arc;

use dcc_mcp_gateway_admin::{
    SkillCapabilityInput, SkillSearchFollowupInput, SkillSearchHitInput, SkillSearchInput,
    skill_inventory_payload, skill_path_hash, skill_path_row,
};
use serde_json::{Value, json};

use crate::gateway::admin::activity::{collect_audits, collect_traces};
use crate::gateway::admin::state::AdminState;
use crate::gateway::capability::{CapabilityRecord, is_backend_job_tool};

pub(super) async fn build_skill_inventory_payload(
    state: &AdminState,
    records: Arc<[CapabilityRecord]>,
) -> Value {
    let search_snapshot = state.gateway.search_telemetry.snapshot(1_000);
    let audits = collect_audits(state, 1_000).await;
    let traces = collect_traces(state, 1_000).await;
    // Adapter-owned job polling is an instance lifecycle transport, not a
    // domain or marketplace Skill. Keep it in capability search and call
    // telemetry, but do not let it inflate Skill inventory or adoption health.
    let skill_records = records
        .iter()
        .filter(|record| !is_backend_job_tool(&record.backend_tool))
        .map(|record| SkillCapabilityInput {
            tool_slug: record.tool_slug.clone(),
            backend_tool: record.backend_tool.clone(),
            skill_name: record.skill_name.clone(),
            summary: record.summary.clone(),
            dcc_type: record.dcc_type.clone(),
            instance_id: record.instance_id.to_string(),
            loaded: record.loaded,
        })
        .collect();
    let searches = search_snapshot
        .recent
        .iter()
        .map(|record| SkillSearchInput {
            timestamp_ms: record.timestamp_ms,
            hits: record
                .hits
                .iter()
                .map(|hit| SkillSearchHitInput {
                    tool_slug: hit.tool_slug.clone(),
                    skill_name: hit.skill_name.clone(),
                    dcc_type: hit.dcc_type.clone(),
                    rank: hit.rank,
                })
                .collect(),
            followups: record
                .followups
                .iter()
                .map(|followup| SkillSearchFollowupInput {
                    kind: followup.kind.clone(),
                    timestamp_ms: followup.timestamp_ms,
                    request_id: followup.request_id.clone(),
                    tool_slug: followup.tool_slug.clone(),
                    skill_name: followup.skill_name.clone(),
                    selected_rank: followup.selected_rank,
                    success: followup.success,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let path_rows = build_skill_path_rows(state);
    let missing_path_count = path_rows
        .iter()
        .filter(|row| row.get("status").and_then(Value::as_str) == Some("missing"))
        .count();

    skill_inventory_payload(
        skill_records,
        &searches,
        &audits,
        &traces,
        path_rows.len(),
        missing_path_count,
    )
}

pub(in crate::gateway::admin) fn build_skill_paths_payload(state: &AdminState) -> Value {
    let paths = build_skill_path_rows(state);
    let missing = paths
        .iter()
        .filter(|row| row.get("status").and_then(Value::as_str) == Some("missing"))
        .count();
    json!({
        "paths": paths,
        "summary": {
            "total": paths.len(),
            "missing": missing,
            "present": paths.len().saturating_sub(missing),
            "path_redaction": "alias",
        }
    })
}

fn build_skill_path_rows(state: &AdminState) -> Vec<Value> {
    let mut rows: Vec<Value> = state
        .skill_paths_snapshot
        .iter()
        .enumerate()
        .map(|(idx, entry)| skill_path_row(&entry.path, &entry.source, None, idx + 1))
        .collect();
    if let Some(ref lane) = state.admin_sqlite_lane {
        let reader = lane.reader();
        for (id, path) in reader.list_custom_skill_paths() {
            if !rows.iter().any(|value| {
                value.get("path_hash").and_then(Value::as_str)
                    == Some(skill_path_hash(&path).as_str())
            }) {
                rows.push(skill_path_row(
                    &path,
                    "admin_custom",
                    Some(id),
                    rows.len() + 1,
                ));
            }
        }
    }
    rows
}
