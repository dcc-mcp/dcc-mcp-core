//! Focused tests for the durable escape-hatch repeat counter (#2297-A3).
//!
//! These cover the wiring between `AuditMiddleware`'s audit record and the
//! `script_promotion_counters` table: three observations of the same script
//! must land on one row that reads back as a promotion candidate.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_gateway_admin::{ScriptPromotionCounter, ScriptPromotionProposalState};

use crate::gateway::admin::AdminAuditSink;
use crate::gateway::admin::sqlite_lane::AdminSqliteLane;
use crate::gateway::admin::state::{AdminAuditRecord, AuditLog};
use crate::gateway::admin::trace::{ScriptExecutionTelemetry, TraceContext};
use crate::gateway::middleware::{AuditEntry, AuditSink};

const SHA: &str = "3f9c1e0b7a2d4f6e8c0b1a3d5e7f9a2c4b6d8e0f1a3c5b7d9e1f3a5c7b9d1e3f";

fn script_telemetry() -> ScriptExecutionTelemetry {
    ScriptExecutionTelemetry {
        sha256: SHA.to_string(),
        reused: Some(false),
        reuse_key: None,
    }
}

fn audit_entry(request_id: &str, script: Option<ScriptExecutionTelemetry>) -> AuditEntry {
    AuditEntry {
        started_at: SystemTime::now(),
        timestamp: SystemTime::now(),
        method: "tools/call".to_string(),
        tool_slug: Some("maya_scripting__execute_python".to_string()),
        dcc_type: Some("maya".to_string()),
        instance_id: Some("instance-1".to_string()),
        session_id: Some("session-1".to_string()),
        transport: Some("mcp".to_string()),
        agent_context: None,
        request_id: request_id.to_string(),
        trace_context: TraceContext {
            trace_id: "00000000000000000000000000000001".to_string(),
            request_id: request_id.to_string(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
        },
        is_error: false,
        result_preview: "ok".to_string(),
        duration_ms: Some(3),
        trace_spans: Vec::new(),
        input_payload: None,
        output_payload: None,
        script_execution: script,
        token_accounting: None,
        llm_usage: None,
    }
}

fn record_three_times(sink: &dyn AuditSink) {
    for i in 0..3 {
        sink.record(audit_entry(&format!("req-{i}"), Some(script_telemetry())));
    }
}

#[test]
fn three_audits_of_one_script_promote_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("promotion.sqlite");
    let lane = AdminSqliteLane::spawn(db.clone(), 30).expect("spawn lane");
    let sink = AdminAuditSink::new(Arc::new(AuditLog::default()), 16)
        .with_sqlite_lane(lane.clone())
        .with_script_promotion_min_repeats(None);
    record_three_times(&sink);
    drop(sink);
    drop(lane);

    let counters = AdminSqliteLane::spawn(db, 30)
        .expect("reopen lane")
        .reader()
        .list_script_promotion_counters(10);
    assert_eq!(counters.len(), 1, "one row per (sha256, dcc, tool)");

    let counter: &ScriptPromotionCounter = &counters[0];
    assert_eq!(counter.sha256, SHA);
    assert_eq!(counter.dcc_type, "maya");
    assert_eq!(counter.tool_name, "maya_scripting__execute_python");
    assert_eq!(counter.count, 3);
    assert_eq!(
        counter.proposal_state,
        ScriptPromotionProposalState::Proposed
    );
    assert!(counter.first_seen_ms <= counter.last_seen_ms);
}

#[test]
fn audits_without_a_script_do_not_count() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("no-script.sqlite");
    let lane = AdminSqliteLane::spawn(db.clone(), 30).expect("spawn lane");
    let sink = AdminAuditSink::new(Arc::new(AuditLog::default()), 16)
        .with_sqlite_lane(lane.clone())
        .with_script_promotion_min_repeats(None);
    for i in 0..3 {
        sink.record(audit_entry(&format!("req-{i}"), None));
    }
    drop(sink);
    drop(lane);

    let counters = AdminSqliteLane::spawn(db, 30)
        .expect("reopen lane")
        .reader()
        .list_script_promotion_counters(10);
    assert!(counters.is_empty());
}

#[test]
fn configured_threshold_changes_when_a_script_is_proposed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("threshold.sqlite");
    let lane = AdminSqliteLane::spawn(db.clone(), 30).expect("spawn lane");
    let sink = AdminAuditSink::new(Arc::new(AuditLog::default()), 16)
        .with_sqlite_lane(lane.clone())
        .with_script_promotion_min_repeats(Some(5));
    record_three_times(&sink);
    drop(sink);
    drop(lane);

    let counter = AdminSqliteLane::spawn(db, 30)
        .expect("reopen lane")
        .reader()
        .get_script_promotion_counter(SHA, "maya", "maya_scripting__execute_python")
        .expect("counter row");
    assert_eq!(counter.count, 3);
    assert_eq!(
        counter.proposal_state,
        ScriptPromotionProposalState::Pending
    );
}

#[test]
fn audit_records_still_reach_the_ring_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("ring.sqlite");
    let lane = AdminSqliteLane::spawn(db, 30).expect("spawn lane");
    let log = Arc::new(AuditLog::default());
    let sink = AdminAuditSink::new(log.clone(), 16)
        .with_sqlite_lane(lane)
        .with_script_promotion_min_repeats(None);
    record_three_times(&sink);
    drop(sink);

    let records: Vec<AdminAuditRecord> = log.lock().clone();
    assert_eq!(records.len(), 3);
    assert!(records.iter().all(|r| r.script_execution.is_some()));
}

#[test]
fn observed_at_ms_uses_the_audit_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("timestamp.sqlite");
    let lane = AdminSqliteLane::spawn(db.clone(), 30).expect("spawn lane");
    let sink = AdminAuditSink::new(Arc::new(AuditLog::default()), 16)
        .with_sqlite_lane(lane.clone())
        .with_script_promotion_min_repeats(None);
    let before_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    sink.record(audit_entry("req-ts", Some(script_telemetry())));
    drop(sink);
    drop(lane);

    let counter = AdminSqliteLane::spawn(db, 30)
        .expect("reopen lane")
        .reader()
        .get_script_promotion_counter(SHA, "maya", "maya_scripting__execute_python")
        .expect("counter row");
    assert!(counter.first_seen_ms >= before_ms);
}
