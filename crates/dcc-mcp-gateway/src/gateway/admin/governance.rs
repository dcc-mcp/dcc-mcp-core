//! Admin governance projection for traffic capture, privacy, policy, and pressure controls.

use std::collections::{BTreeMap, BTreeSet};
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};

use super::state::{AdminAuditRecord, AdminState};
use crate::gateway::middleware::MiddlewareGovernanceSnapshot;
use crate::gateway::traffic::TrafficCaptureDecision;

const SCHEMA_VERSION: &str = "dcc-mcp.admin.governance.v1";

/// Build a read-only governance payload for Admin and `/v1/debug/governance`.
pub async fn build_governance_payload(state: &AdminState, limit: usize) -> Value {
    let limit = limit.clamp(1, 1_000);
    let policy = policy_snapshot(&state.gateway.policy);
    let middleware = state.gateway.middleware_chain.governance_snapshot();
    let traffic_capture = state.gateway.traffic_capture.governance_snapshot();
    let recent_decisions = recent_request_decisions(
        collect_recent_audits(state, limit).await,
        traffic_capture.recent_decisions.clone(),
        &middleware,
        limit,
    );
    let stats =
        governance_stats_from_decisions(&recent_decisions, &traffic_capture.recent_decisions);

    json!({
        "schema_version": SCHEMA_VERSION,
        "generated_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "mode": {
            "admin_mutations": "disabled",
            "reason": "Admin has no authentication by default, so governance is exposed as an operator-readable control plane.",
        },
        "policy": policy,
        "traffic_capture": traffic_capture,
        "middleware": middleware,
        "stats": stats,
        "recent_decisions": recent_decisions,
    })
}

/// Compact governance counters that can be embedded in `/admin/api/stats`.
pub fn build_governance_stats(state: &AdminState) -> Value {
    let capture = state.gateway.traffic_capture.governance_snapshot();
    let decisions = recent_request_decisions(
        collect_recent_audits_sync(state, 1_000),
        capture.recent_decisions.clone(),
        &state.gateway.middleware_chain.governance_snapshot(),
        1_000,
    );
    governance_stats_from_decisions(&decisions, &capture.recent_decisions)
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

async fn collect_recent_audits(state: &AdminState, limit: usize) -> Vec<AdminAuditRecord> {
    collect_recent_audits_sync(state, limit)
}

fn collect_recent_audits_sync(state: &AdminState, limit: usize) -> Vec<AdminAuditRecord> {
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

fn recent_request_decisions(
    audits: Vec<AdminAuditRecord>,
    capture_decisions: Vec<TrafficCaptureDecision>,
    middleware: &MiddlewareGovernanceSnapshot,
    limit: usize,
) -> Vec<Value> {
    let mut capture_by_request: BTreeMap<String, Vec<TrafficCaptureDecision>> = BTreeMap::new();
    let mut capture_only = Vec::new();
    for decision in capture_decisions {
        if let Some(request_id) = &decision.request_id {
            capture_by_request
                .entry(request_id.clone())
                .or_default()
                .push(decision);
        } else {
            capture_only.push(decision);
        }
    }

    let mut rows = Vec::new();
    let redaction_active = middleware
        .controls
        .iter()
        .any(|control| control.kind == "redaction");
    let quota_active = middleware
        .controls
        .iter()
        .any(|control| control.kind == "quota");

    for audit in audits {
        let capture = capture_by_request
            .remove(&audit.request_id)
            .unwrap_or_default();
        let capture_summary = capture_summary(&capture);
        rows.push(json!({
            "timestamp": timestamp_string(audit.timestamp),
            "request_id": audit.request_id,
            "trace_id": audit.trace_id,
            "session_id": audit.session_id,
            "transport": audit.transport,
            "agent_id": audit.agent_id,
            "agent_name": audit.agent_name,
            "agent_model": audit.agent_model,
            "parent_request_id": audit.parent_request_id,
            "tool": audit.action,
            "dcc_type": audit.dcc_type,
            "outcome": request_outcome(&audit),
            "success": audit.success,
            "reason": request_reason(&audit),
            "duration_ms": audit.duration_ms,
            "policy": {
                "read_only": audit_error_text(&audit).contains("read-only"),
                "denied": is_policy_denied(&audit),
                "reason": policy_reason(&audit),
            },
            "traffic_capture": capture_summary,
            "privacy": {
                "redaction_middleware_active": redaction_active,
                "redacted_paths": redacted_paths(&capture),
            },
            "pressure": {
                "quota_active": quota_active,
                "throttled": is_throttled(&audit),
            },
        }));
    }

    for (_request_id, capture) in capture_by_request {
        if let Some(first) = capture.first() {
            rows.push(json!({
                "timestamp": timestamp_string(first.timestamp),
                "request_id": first.request_id,
                "trace_id": first.trace_id,
                "session_id": first.session_id,
                "transport": first.transport,
                "tool": first.mcp_method,
                "outcome": "capture-only",
                "success": Value::Null,
                "reason": "traffic-capture-frame-without-audit-row",
                "traffic_capture": capture_summary(&capture),
                "privacy": {
                    "redaction_middleware_active": redaction_active,
                    "redacted_paths": redacted_paths(&capture),
                },
                "pressure": {
                    "quota_active": quota_active,
                    "throttled": false,
                },
            }));
        }
    }

    for decision in capture_only {
        rows.push(json!({
            "timestamp": timestamp_string(decision.timestamp),
            "request_id": Value::Null,
            "trace_id": decision.trace_id,
            "session_id": decision.session_id,
            "transport": decision.transport,
            "tool": decision.mcp_method,
            "outcome": "capture-only",
            "success": Value::Null,
            "reason": decision.reason,
            "traffic_capture": capture_summary(std::slice::from_ref(&decision)),
            "privacy": {
                "redaction_middleware_active": redaction_active,
                "redacted_paths": decision.redacted_paths,
            },
            "pressure": {
                "quota_active": quota_active,
                "throttled": false,
            },
        }));
    }

    rows.sort_by(|a, b| {
        a.get("timestamp")
            .and_then(Value::as_str)
            .cmp(&b.get("timestamp").and_then(Value::as_str))
    });
    let overflow = rows.len().saturating_sub(limit);
    if overflow > 0 {
        rows.drain(0..overflow);
    }
    rows
}

fn capture_summary(decisions: &[TrafficCaptureDecision]) -> Value {
    let captured = decisions
        .iter()
        .filter(|decision| decision.outcome == "captured")
        .count();
    let skipped = decisions
        .iter()
        .filter(|decision| decision.outcome == "skipped")
        .count();
    let reasons: BTreeSet<String> = decisions
        .iter()
        .filter_map(|decision| decision.reason.clone())
        .collect();
    json!({
        "frame_count": decisions.len(),
        "captured": captured,
        "skipped": skipped,
        "reasons": reasons.into_iter().collect::<Vec<_>>(),
    })
}

fn redacted_paths(decisions: &[TrafficCaptureDecision]) -> Vec<String> {
    decisions
        .iter()
        .flat_map(|decision| decision.redacted_paths.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn governance_stats_from_decisions(
    decisions: &[Value],
    capture_decisions: &[TrafficCaptureDecision],
) -> Value {
    let policy_denied = decisions
        .iter()
        .filter(|row| row.pointer("/policy/denied").and_then(Value::as_bool) == Some(true))
        .count();
    let throttled = decisions
        .iter()
        .filter(|row| row.pointer("/pressure/throttled").and_then(Value::as_bool) == Some(true))
        .count();
    let allowed = decisions
        .iter()
        .filter(|row| row.get("outcome").and_then(Value::as_str) == Some("allowed"))
        .count();
    let captured_frames = capture_decisions
        .iter()
        .filter(|decision| decision.outcome == "captured")
        .count();
    let skipped_frames = capture_decisions
        .iter()
        .filter(|decision| decision.outcome == "skipped")
        .count();
    let redacted_paths = redacted_paths(capture_decisions);
    json!({
        "recent_allowed": allowed,
        "recent_policy_denied": policy_denied,
        "recent_throttled": throttled,
        "captured_frames": captured_frames,
        "skipped_capture_frames": skipped_frames,
        "redacted_path_count": redacted_paths.len(),
        "redacted_paths": redacted_paths,
    })
}

fn request_outcome(audit: &AdminAuditRecord) -> &'static str {
    if audit.success {
        "allowed"
    } else if is_throttled(audit) {
        "throttled"
    } else if is_policy_denied(audit) {
        "denied"
    } else {
        "failed"
    }
}

fn request_reason(audit: &AdminAuditRecord) -> Option<String> {
    if audit.success {
        return Some("allowed".to_string());
    }
    audit.error.clone()
}

fn policy_reason(audit: &AdminAuditRecord) -> Option<String> {
    let text = audit_error_text(audit);
    for reason in [
        "read-only",
        "dcc-allowlist",
        "skill-allowlist",
        "tool-allowlist",
    ] {
        if text.contains(reason) {
            return Some(reason.to_string());
        }
    }
    None
}

fn is_policy_denied(audit: &AdminAuditRecord) -> bool {
    let text = audit_error_text(audit);
    text.contains("policy-denied") || text.contains("gateway policy denied")
}

fn is_throttled(audit: &AdminAuditRecord) -> bool {
    let text = audit_error_text(audit);
    text.contains("quota exceeded") || text.contains("throttled")
}

fn audit_error_text(audit: &AdminAuditRecord) -> String {
    audit.error.clone().unwrap_or_default().to_ascii_lowercase()
}

fn timestamp_string(timestamp: std::time::SystemTime) -> String {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|_| chrono::DateTime::<chrono::Utc>::from(timestamp).to_rfc3339())
        .unwrap_or_default()
}
