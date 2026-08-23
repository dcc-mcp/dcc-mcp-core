//! Gateway adapters for admin governance projections.

use std::collections::BTreeMap;

use dcc_mcp_gateway_admin::{
    GovernanceCaptureDecision, GovernanceMiddlewareState, governance_payload, governance_stats,
};
use serde_json::{Value, json};

use super::super::state::{AdminAuditRecord, AdminState};
use crate::gateway::traffic::{TrafficCaptureDecision, TrafficCaptureSnapshot};

/// Build a read-only governance payload for Admin and `/v1/debug/governance`.
pub async fn build_governance_payload(state: &AdminState, limit: usize) -> Value {
    let limit = limit.clamp(1, 1_000);
    let middleware = state.gateway.middleware_chain.governance_snapshot();
    let traffic_capture = state.gateway.traffic_capture.governance_snapshot();
    let capture_decisions = governance_capture_decisions(&traffic_capture);

    governance_payload(
        policy_snapshot(&state.gateway.policy),
        serde_json::to_value(&traffic_capture)
            .expect("gateway traffic governance snapshot must serialize"),
        governance_middleware_state(&middleware),
        collect_recent_audits(state, limit),
        capture_decisions,
        limit,
    )
}

/// Compact governance counters that can be embedded in `/admin/api/stats`.
pub fn build_governance_stats(state: &AdminState) -> Value {
    let middleware = state.gateway.middleware_chain.governance_snapshot();
    let traffic_capture = state.gateway.traffic_capture.governance_snapshot();
    governance_stats(
        collect_recent_audits(state, 1_000),
        governance_capture_decisions(&traffic_capture),
        &governance_middleware_state(&middleware),
        1_000,
    )
}

fn policy_snapshot(policy: &crate::gateway::GatewayPolicy) -> Value {
    json!({
        "read_only": policy.read_only,
        "unrestricted": policy.is_unrestricted(),
        "allowlists_active": {
            "dcc_types": !policy.allowed_dcc_types.is_empty(),
            "skill_names": !policy.allowed_skill_names.is_empty(),
            "skill_families": !policy.allowed_skill_families.is_empty(),
            "tool_slugs": !policy.allowed_tool_slugs.is_empty(),
            "tool_slug_prefixes": !policy.allowed_tool_slug_prefixes.is_empty(),
        },
        "allowed_dcc_types": policy.allowed_dcc_types,
        "allowed_skill_names": policy.allowed_skill_names,
        "allowed_skill_families": policy.allowed_skill_families,
        "allowed_tool_slugs": policy.allowed_tool_slugs,
        "allowed_tool_slug_prefixes": policy.allowed_tool_slug_prefixes,
    })
}

fn collect_recent_audits(state: &AdminState, limit: usize) -> Vec<AdminAuditRecord> {
    let mut rows = BTreeMap::<String, AdminAuditRecord>::new();
    if let Some(lane) = &state.admin_sqlite_lane {
        for row in lane
            .reader()
            .list_audits_recent(limit.saturating_mul(2).max(200))
        {
            rows.insert(row.request_id.clone(), row);
        }
    }
    if let Some(log) = &state.audit_log {
        for row in log.lock().iter().cloned() {
            rows.insert(row.request_id.clone(), row);
        }
    }
    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by_key(|row| row.timestamp);
    let overflow = rows.len().saturating_sub(limit);
    if overflow > 0 {
        rows.drain(0..overflow);
    }
    rows
}

fn governance_capture_decisions(
    snapshot: &TrafficCaptureSnapshot,
) -> Vec<GovernanceCaptureDecision> {
    snapshot
        .recent_decisions
        .iter()
        .map(governance_capture_decision)
        .collect()
}

pub(super) fn governance_capture_decision(
    decision: &TrafficCaptureDecision,
) -> GovernanceCaptureDecision {
    GovernanceCaptureDecision {
        timestamp: decision.timestamp,
        request_id: decision.request_id.clone(),
        trace_id: decision.trace_id.clone(),
        session_id: decision.session_id.clone(),
        transport: decision.transport.clone(),
        mcp_method: decision.mcp_method.clone(),
        outcome: decision.outcome.clone(),
        reason: decision.reason.clone(),
        redacted_paths: decision.redacted_paths.clone(),
    }
}

fn governance_middleware_state(
    snapshot: &crate::gateway::middleware::MiddlewareGovernanceSnapshot,
) -> GovernanceMiddlewareState {
    GovernanceMiddlewareState {
        snapshot: serde_json::to_value(snapshot)
            .expect("gateway middleware governance snapshot must serialize"),
        redaction_active: snapshot
            .controls
            .iter()
            .any(|control| control.kind == "redaction"),
        quota_active: snapshot
            .controls
            .iter()
            .any(|control| control.kind == "quota"),
    }
}
