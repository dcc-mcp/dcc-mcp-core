//! Admin governance projections for policy, privacy, and pressure controls.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::AdminAuditRecord;

const SCHEMA_VERSION: &str = "dcc-mcp.admin.governance.v1";

/// Backend-neutral capture decision consumed by governance projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernanceCaptureDecision {
    pub timestamp: SystemTime,
    pub request_id: Option<String>,
    pub trace_id: Option<String>,
    pub session_id: Option<String>,
    pub transport: String,
    pub mcp_method: Option<String>,
    pub outcome: String,
    pub reason: Option<String>,
    pub redacted_paths: Vec<String>,
}

/// Backend-neutral middleware state consumed by governance projections.
#[derive(Debug, Clone, PartialEq)]
pub struct GovernanceMiddlewareState {
    /// Serialized middleware snapshot returned to admin clients.
    pub snapshot: Value,
    /// Whether a redaction control is active in the middleware chain.
    pub redaction_active: bool,
    /// Whether a quota control is active in the middleware chain.
    pub quota_active: bool,
}

/// Build the read-only governance payload shared by Admin and debug routes.
#[must_use]
pub fn governance_payload(
    policy: Value,
    traffic_capture: Value,
    middleware: GovernanceMiddlewareState,
    audits: Vec<AdminAuditRecord>,
    capture_decisions: Vec<GovernanceCaptureDecision>,
    limit: usize,
) -> Value {
    let limit = limit.clamp(1, 1_000);
    let recent_decisions =
        recent_request_decisions(audits, capture_decisions.clone(), &middleware, limit);
    let stats = governance_stats_from_decisions(&recent_decisions, &capture_decisions);

    json!({
        "schema_version": SCHEMA_VERSION,
        "generated_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "mode": {
            "admin_mutations": "disabled",
            "reason": "Admin has no authentication by default, so governance is exposed as an operator-readable control plane.",
        },
        "policy": policy,
        "traffic_capture": traffic_capture,
        "middleware": middleware.snapshot,
        "stats": stats,
        "recent_decisions": recent_decisions,
    })
}

/// Build compact governance counters for the Admin statistics payload.
#[must_use]
pub fn governance_stats(
    audits: Vec<AdminAuditRecord>,
    capture_decisions: Vec<GovernanceCaptureDecision>,
    middleware: &GovernanceMiddlewareState,
    limit: usize,
) -> Value {
    let decisions = recent_request_decisions(
        audits,
        capture_decisions.clone(),
        middleware,
        limit.clamp(1, 1_000),
    );
    governance_stats_from_decisions(&decisions, &capture_decisions)
}

fn recent_request_decisions(
    audits: Vec<AdminAuditRecord>,
    capture_decisions: Vec<GovernanceCaptureDecision>,
    middleware: &GovernanceMiddlewareState,
    limit: usize,
) -> Vec<Value> {
    let mut capture_by_request = BTreeMap::<String, Vec<GovernanceCaptureDecision>>::new();
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
    let redaction_active = middleware.redaction_active;
    let quota_active = middleware.quota_active;

    for audit in audits {
        let capture = capture_by_request
            .remove(&audit.request_id)
            .unwrap_or_default();
        rows.push(json!({
            "timestamp": timestamp_string(audit.timestamp),
            "request_id": audit.request_id,
            "trace_id": audit.trace_id,
            "session_id": audit.session_id,
            "transport": audit.transport,
            "agent_id": audit.agent_id,
            "agent_name": audit.agent_name,
            "agent_model": audit.agent_model,
            "actor_id": audit.actor_id,
            "actor_name": audit.actor_name,
            "client_platform": audit.client_platform,
            "source_ip": audit.source_ip,
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
            "traffic_capture": capture_summary(&capture),
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

    for capture in capture_by_request.into_values() {
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

fn capture_summary(decisions: &[GovernanceCaptureDecision]) -> Value {
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

fn redacted_paths(decisions: &[GovernanceCaptureDecision]) -> Vec<String> {
    decisions
        .iter()
        .flat_map(|decision| decision.redacted_paths.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn governance_stats_from_decisions(
    decisions: &[Value],
    capture_decisions: &[GovernanceCaptureDecision],
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

fn timestamp_string(timestamp: SystemTime) -> String {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|_| chrono::DateTime::<chrono::Utc>::from(timestamp).to_rfc3339())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit(request_id: &str, success: bool, error: Option<&str>) -> AdminAuditRecord {
        AdminAuditRecord {
            timestamp: UNIX_EPOCH + std::time::Duration::from_secs(1),
            request_id: request_id.to_string(),
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            method: None,
            instance_id: None,
            session_id: None,
            transport: Some("rest".to_string()),
            agent_id: None,
            agent_name: None,
            agent_model: None,
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: None,
            action: "maya__render".to_string(),
            dcc_type: Some("maya".to_string()),
            success,
            error: error.map(str::to_string),
            duration_ms: Some(5),
            token_accounting: None,
            llm_usage: None,
        }
    }

    fn capture(request_id: Option<&str>, outcome: &str) -> GovernanceCaptureDecision {
        GovernanceCaptureDecision {
            timestamp: UNIX_EPOCH + std::time::Duration::from_secs(2),
            request_id: request_id.map(str::to_string),
            trace_id: None,
            session_id: None,
            transport: "mcp".to_string(),
            mcp_method: Some("tools/call".to_string()),
            outcome: outcome.to_string(),
            reason: (outcome == "skipped").then(|| "filter".to_string()),
            redacted_paths: vec!["$.arguments.secret".to_string()],
        }
    }

    fn middleware() -> GovernanceMiddlewareState {
        GovernanceMiddlewareState {
            snapshot: json!({"controls": [{"kind": "redaction"}, {"kind": "quota"}]}),
            redaction_active: true,
            quota_active: true,
        }
    }

    #[test]
    fn payload_correlates_audits_and_capture_decisions() {
        let payload = governance_payload(
            json!({"read_only": true}),
            json!({"enabled": true}),
            middleware(),
            vec![
                audit("allowed", true, None),
                audit("denied", false, Some("policy-denied: read-only")),
                audit("throttled", false, Some("quota exceeded")),
            ],
            vec![
                capture(Some("allowed"), "captured"),
                capture(None, "skipped"),
            ],
            100,
        );

        assert_eq!(payload["stats"]["recent_allowed"], 1);
        assert_eq!(payload["stats"]["recent_policy_denied"], 1);
        assert_eq!(payload["stats"]["recent_throttled"], 1);
        assert_eq!(payload["stats"]["captured_frames"], 1);
        assert_eq!(payload["stats"]["skipped_capture_frames"], 1);
        assert_eq!(
            payload["recent_decisions"].as_array().map(Vec::len),
            Some(4)
        );
    }

    #[test]
    fn stats_limit_is_clamped_and_middleware_flags_are_backend_neutral() {
        let stats = governance_stats(vec![audit("allowed", true, None)], vec![], &middleware(), 0);

        assert_eq!(stats["recent_allowed"], 1);
        assert_eq!(stats["recent_throttled"], 0);
    }
}
