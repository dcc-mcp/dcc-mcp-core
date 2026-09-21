//! SQLite-backed admin persistence (traces, audits, custom skill paths).
//!
//! When the `persist-sqlite` feature is off, this module exposes no-op
//! stubs so `admin`-only test builds keep compiling.
//!
//! The writer thread and schema live in `dcc-mcp-db` (`gateway-admin-sqlite`);
//! this module is a thin type-preserving façade over [`DispatchTrace`] /
//! [`AdminAuditRecord`].

use std::path::PathBuf;

use std::path::Path;
use std::time::SystemTime;

use crate::{AdminAuditRecord, DispatchTrace, FeedbackReportRow};

#[cfg(feature = "persist-sqlite")]
use std::time::{Duration, UNIX_EPOCH};

#[cfg(feature = "persist-sqlite")]
use dcc_mcp_db::{
    FeedbackReportInsert, FeedbackReportRow, GatewayAdminAuditPersistedJson,
    GatewayAdminSqliteLane as InnerLane, GatewayAdminSqliteReader as InnerReader,
    GatewayDeregisteredInstanceJson, ScriptPromotionBumpJson, ScriptPromotionCounter,
};

// #2297-A3: the counter value object is pure data, so the no-op facade can
// name it even when no SQLite driver is compiled in.
#[cfg(not(feature = "persist-sqlite"))]
use dcc_mcp_db::ScriptPromotionCounter;

#[cfg(feature = "persist-sqlite")]
#[derive(Clone)]
pub struct AdminSqliteReader {
    inner: InnerReader,
}

#[cfg(feature = "persist-sqlite")]
impl AdminSqliteReader {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            inner: InnerReader::new(path),
        }
    }

    #[must_use]
    pub fn list_traces_since(
        &self,
        cutoff: Option<SystemTime>,
        limit: usize,
    ) -> Vec<DispatchTrace> {
        self.inner
            .list_traces_since_json(cutoff, limit)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    #[must_use]
    pub fn get_trace(&self, request_id: &str) -> Option<DispatchTrace> {
        let s = self.inner.get_trace_json(request_id)?;
        serde_json::from_str(&s).ok()
    }

    #[must_use]
    pub fn list_audits_recent(&self, limit: usize) -> Vec<AdminAuditRecord> {
        self.inner
            .list_audits_recent_json(limit)
            .into_iter()
            .filter_map(|s| {
                let p: GatewayAdminAuditPersistedJson = serde_json::from_str(&s).ok()?;
                Some(admin_audit_from_persisted(p))
            })
            .collect()
    }

    /// Read persisted audit rows in a time range, newest first (for analytics aggregation).
    #[must_use]
    pub fn list_audits_since(
        &self,
        cutoff: Option<SystemTime>,
        limit: usize,
    ) -> Vec<AdminAuditRecord> {
        self.inner
            .list_audits_since_json(cutoff, limit)
            .into_iter()
            .filter_map(|s| {
                let p: GatewayAdminAuditPersistedJson = serde_json::from_str(&s).ok()?;
                Some(admin_audit_from_persisted(p))
            })
            .collect()
    }

    #[must_use]
    pub fn list_custom_skill_paths(&self) -> Vec<(i64, String)> {
        self.inner.list_custom_skill_paths()
    }

    #[must_use]
    pub fn list_deregistered_instances(&self, limit: usize) -> Vec<serde_json::Value> {
        self.inner
            .list_deregistered_instances_json(limit)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    #[must_use]
    pub fn list_agent_memory(
        &self,
        layer: Option<&str>,
        dcc_name: Option<&str>,
        session_id: Option<&str>,
        key_prefix: Option<&str>,
        limit: usize,
    ) -> Vec<serde_json::Value> {
        self.inner
            .list_agent_memory_json(layer, dcc_name, session_id, key_prefix, limit)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    /// PIP-2751: List sessions with optional filters.
    #[must_use]
    pub fn list_sessions(
        &self,
        limit: usize,
        dcc_type: Option<&str>,
        status: Option<&str>,
    ) -> Vec<serde_json::Value> {
        self.inner
            .list_sessions_json(limit, dcc_type, status)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    /// PIP-2751: Get a single session by id.
    #[must_use]
    pub fn get_session(&self, session_id: &str) -> Option<serde_json::Value> {
        let s = self.inner.get_session_json(session_id)?;
        serde_json::from_str(&s).ok()
    }

    /// PIP-2751: List session events.
    #[must_use]
    pub fn list_session_events(&self, session_id: &str, limit: usize) -> Vec<serde_json::Value> {
        self.inner
            .list_session_events_json(session_id, limit)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    #[must_use]
    pub fn list_recording_events(
        &self,
        session_id: &str,
        recording_id: &str,
        limit: usize,
    ) -> Vec<serde_json::Value> {
        self.inner
            .list_recording_events_json(session_id, recording_id, limit)
            .into_iter()
            .filter_map(|value| serde_json::from_str(&value).ok())
            .collect()
    }

    #[must_use]
    pub fn list_unfinished_recording_starts(&self, limit: usize) -> Vec<serde_json::Value> {
        self.inner
            .list_unfinished_recording_starts_json(limit)
            .into_iter()
            .filter_map(|value| serde_json::from_str(&value).ok())
            .collect()
    }

    #[must_use]
    pub fn list_experiments(&self, limit: usize) -> Vec<serde_json::Value> {
        self.inner
            .list_experiments_json(limit)
            .into_iter()
            .filter_map(|value| serde_json::from_str(&value).ok())
            .collect()
    }

    #[must_use]
    pub fn list_experiment_events(
        &self,
        experiment_id: &str,
        limit: usize,
    ) -> Vec<serde_json::Value> {
        self.inner
            .list_experiment_events_json(experiment_id, limit)
            .into_iter()
            .filter_map(|value| serde_json::from_str(&value).ok())
            .collect()
    }

    /// PIP-2751: List tool calls for a session.
    #[must_use]
    pub fn list_tool_calls(&self, session_id: &str, limit: usize) -> Vec<serde_json::Value> {
        self.inner
            .list_tool_calls_json(session_id, limit)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    /// PIP-2751: List all tool calls with optional session filter.
    #[must_use]
    pub fn list_all_tool_calls(
        &self,
        limit: usize,
        session_id: Option<&str>,
    ) -> Vec<serde_json::Value> {
        self.inner
            .list_all_tool_calls_json(limit, session_id)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    /// #2253-E1: List persisted feedback reports, newest first.
    ///
    /// `cutoff` filters on the report timestamp; `dcc` / `severity` are
    /// case-insensitive equality filters. Returns an empty vector when the
    /// `feedback_reports` table is absent or unreadable so callers can fall
    /// back to the per-DCC JSONL mirror.
    #[must_use]
    pub fn list_feedback_reports(
        &self,
        cutoff_ms: Option<i64>,
        dcc: Option<&str>,
        severity: Option<&str>,
        limit: usize,
    ) -> Vec<serde_json::Value> {
        self.inner
            .list_feedback_reports_json(cutoff_ms, dcc, severity, limit)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    /// #2297-A3: Read one persisted repeat counter, if the key was observed.
    #[must_use]
    pub fn get_script_promotion_counter(
        &self,
        sha256: &str,
        dcc_type: &str,
        tool_name: &str,
    ) -> Option<ScriptPromotionCounter> {
        self.inner
            .get_script_promotion_counter_json(sha256, dcc_type, tool_name)
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    /// #2297-A3: Read repeat counters, most recently bumped first.
    #[must_use]
    pub fn list_script_promotion_counters(&self, limit: usize) -> Vec<ScriptPromotionCounter> {
        self.inner
            .list_script_promotion_counters_json(limit)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }
}

#[cfg(feature = "persist-sqlite")]
fn admin_audit_from_persisted(p: GatewayAdminAuditPersistedJson) -> AdminAuditRecord {
    AdminAuditRecord {
        timestamp: UNIX_EPOCH + Duration::from_millis(p.timestamp_ms),
        request_id: p.request_id,
        trace_id: p.trace_id,
        span_id: p.span_id,
        parent_span_id: p.parent_span_id,
        method: p.method,
        instance_id: p.instance_id,
        session_id: p.session_id,
        transport: p.transport,
        agent_id: p.agent_id,
        agent_name: p.agent_name,
        agent_model: p.agent_model,
        actor_id: p.actor_id,
        actor_name: p.actor_name,
        actor_email_hash: p.actor_email_hash,
        client_platform: p.client_platform,
        client_os: p.client_os,
        client_host: p.client_host,
        auth_subject: p.auth_subject,
        source_ip: p.source_ip,
        attribution_trust: p
            .attribution_trust
            .and_then(|value| serde_json::from_value(value).ok()),
        parent_request_id: p.parent_request_id,
        action: p.action,
        dcc_type: p.dcc_type,
        success: p.success,
        error: p.error,
        duration_ms: p.duration_ms,
        script_execution: p
            .script_execution
            .and_then(|value| serde_json::from_value(value).ok()),
        token_accounting: p
            .token_accounting
            .and_then(|value| serde_json::from_value(value).ok()),
        llm_usage: p
            .llm_usage
            .and_then(|value| serde_json::from_value(value).ok()),
    }
}

#[cfg(feature = "persist-sqlite")]
#[derive(Clone)]
pub struct AdminSqliteLane {
    inner: InnerLane,
}

#[cfg(feature = "persist-sqlite")]
impl AdminSqliteLane {
    pub fn spawn(path: PathBuf, retention_days: u32) -> Result<Self, String> {
        Ok(Self {
            inner: InnerLane::spawn(path, retention_days)?,
        })
    }

    #[must_use]
    pub fn reader(&self) -> AdminSqliteReader {
        AdminSqliteReader {
            inner: self.inner.reader(),
        }
    }

    pub fn try_persist_trace(&self, t: &DispatchTrace) {
        if let Ok(json) = serde_json::to_string(t) {
            self.inner.try_persist_trace_json(&json);
        }
    }

    pub fn try_persist_audit(&self, r: &AdminAuditRecord) {
        let row = audit_to_persisted(r);
        if let Ok(json) = serde_json::to_string(&row) {
            self.inner.try_persist_audit_json(&json);
        }
    }

    pub fn try_persist_deregistered_instance(
        &self,
        entry: &dcc_mcp_transport::discovery::types::ServiceEntry,
        reason: &str,
    ) {
        let row = deregistered_to_persisted(entry, reason);
        if let Ok(json) = serde_json::to_string(&row) {
            self.inner.try_persist_deregistered_instance_json(&json);
        }
    }

    #[must_use]
    pub fn try_add_skill_path(&self, path: String) -> bool {
        self.inner.try_add_skill_path(path)
    }

    #[must_use]
    pub fn try_delete_skill_path(&self, id: i64) -> bool {
        self.inner.try_delete_skill_path(id)
    }

    pub fn try_persist_tool_call_event(&self, event: &dcc_mcp_models::ToolCallEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            self.inner.try_persist_tool_call_event_json(&json);
        }
    }

    /// #2297-A3: Record one repeat observation of a materialised script.
    ///
    /// Idempotent per `(sha256, dcc_type, tool_name)`: repeat observations
    /// bump the existing row instead of creating a new one.
    pub fn try_bump_script_promotion_counter(&self, bump: &ScriptPromotionBumpJson) {
        if let Ok(json) = serde_json::to_string(bump) {
            self.inner.try_bump_script_promotion_counter_json(&json);
        }
    }

    /// Persist a bounded recording projection in the existing session timeline.
    pub fn try_persist_session_event(&self, event: &serde_json::Value) {
        if let Ok(json) = serde_json::to_string(event) {
            self.inner.try_persist_session_event_json(&json);
        }
    }

    #[must_use]
    pub fn try_delete_agent_memory(
        &self,
        id: Option<i64>,
        layer: Option<String>,
        dcc_name: Option<String>,
        session_id: Option<String>,
        key_prefix: Option<String>,
    ) -> bool {
        self.inner
            .try_delete_agent_memory(id, layer, dcc_name, session_id, key_prefix)
    }

    /// #2253-E1: Persist an agent feedback report.
    pub fn try_persist_feedback_report(&self, report: &FeedbackReportRow) {
        if let Ok(json) = serde_json::to_string(report) {
            self.inner.try_persist_feedback_report_json(&json);
        }
    }

    /// #2253-E2: Collapse one finding onto its `(repo, fingerprint)` row.
    ///
    /// Synchronous because the caller needs the resulting id and
    /// `occurrence_count` to answer the ingest request.
    pub fn upsert_feedback_report(
        &self,
        finding: &FeedbackReportInsert,
    ) -> Result<FeedbackReportRow, dcc_mcp_db::DbError> {
        self.inner.upsert_feedback_report(finding)
    }

    /// #2253-E2: Look up one dedup row by its `(repo, fingerprint)` key.
    #[must_use]
    pub fn get_feedback_report(
        &self,
        repo: &str,
        fingerprint: &str,
    ) -> Option<FeedbackReportRow> {
        self.inner.get_feedback_report(repo, fingerprint)
    }

    /// #2253-E2: Most recently seen dedup rows, newest first.
    #[must_use]
    pub fn list_feedback_reports(&self, limit: usize) -> Vec<FeedbackReportRow> {
        self.inner.list_feedback_reports(limit)
    }
}

#[cfg(feature = "persist-sqlite")]
fn audit_to_persisted(r: &AdminAuditRecord) -> GatewayAdminAuditPersistedJson {
    GatewayAdminAuditPersistedJson {
        timestamp_ms: r
            .timestamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64,
        request_id: r.request_id.clone(),
        trace_id: r.trace_id.clone(),
        span_id: r.span_id.clone(),
        parent_span_id: r.parent_span_id.clone(),
        method: r.method.clone(),
        instance_id: r.instance_id.clone(),
        session_id: r.session_id.clone(),
        transport: r.transport.clone(),
        agent_id: r.agent_id.clone(),
        agent_name: r.agent_name.clone(),
        agent_model: r.agent_model.clone(),
        actor_id: r.actor_id.clone(),
        actor_name: r.actor_name.clone(),
        actor_email_hash: r.actor_email_hash.clone(),
        client_platform: r.client_platform.clone(),
        client_os: r.client_os.clone(),
        client_host: r.client_host.clone(),
        auth_subject: r.auth_subject.clone(),
        source_ip: r.source_ip.clone(),
        attribution_trust: r
            .attribution_trust
            .as_ref()
            .and_then(|value| serde_json::to_value(value).ok()),
        parent_request_id: r.parent_request_id.clone(),
        action: r.action.clone(),
        dcc_type: r.dcc_type.clone(),
        success: r.success,
        error: r.error.clone(),
        duration_ms: r.duration_ms,
        script_execution: r
            .script_execution
            .as_ref()
            .and_then(|value| serde_json::to_value(value).ok()),
        token_accounting: r
            .token_accounting
            .as_ref()
            .and_then(|value| serde_json::to_value(value).ok()),
        llm_usage: r
            .llm_usage
            .as_ref()
            .and_then(|value| serde_json::to_value(value).ok()),
    }
}

#[cfg(feature = "persist-sqlite")]
fn deregistered_to_persisted(
    entry: &dcc_mcp_transport::discovery::types::ServiceEntry,
    reason: &str,
) -> GatewayDeregisteredInstanceJson {
    GatewayDeregisteredInstanceJson {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64,
        reason: reason.to_string(),
        dcc_type: entry.dcc_type.clone(),
        instance_id: entry.instance_id.to_string(),
        entry: serde_json::to_value(entry).unwrap_or(serde_json::Value::Null),
    }
}

#[cfg(feature = "persist-sqlite")]
pub use dcc_mcp_db::read_custom_skill_paths_for_startup;

#[cfg(not(feature = "persist-sqlite"))]
#[derive(Clone, Default)]
pub struct AdminSqliteReader;

#[cfg(not(feature = "persist-sqlite"))]
impl AdminSqliteReader {
    #[must_use]
    pub fn new(_path: PathBuf) -> Self {
        Self
    }

    #[must_use]
    pub fn list_traces_since(
        &self,
        _cutoff: Option<SystemTime>,
        _limit: usize,
    ) -> Vec<DispatchTrace> {
        vec![]
    }

    #[must_use]
    pub fn get_trace(&self, _request_id: &str) -> Option<DispatchTrace> {
        None
    }

    #[must_use]
    pub fn list_audits_recent(&self, _limit: usize) -> Vec<AdminAuditRecord> {
        vec![]
    }

    #[must_use]
    pub fn list_audits_since(
        &self,
        _cutoff: Option<SystemTime>,
        _limit: usize,
    ) -> Vec<AdminAuditRecord> {
        vec![]
    }

    #[must_use]
    pub fn list_custom_skill_paths(&self) -> Vec<(i64, String)> {
        vec![]
    }

    #[must_use]
    pub fn list_deregistered_instances(&self, _limit: usize) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_agent_memory(
        &self,
        _layer: Option<&str>,
        _dcc_name: Option<&str>,
        _session_id: Option<&str>,
        _key_prefix: Option<&str>,
        _limit: usize,
    ) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_sessions(
        &self,
        _limit: usize,
        _dcc_type: Option<&str>,
        _status: Option<&str>,
    ) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn get_session(&self, _session_id: &str) -> Option<serde_json::Value> {
        None
    }

    #[must_use]
    pub fn list_session_events(&self, _session_id: &str, _limit: usize) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_recording_events(
        &self,
        _session_id: &str,
        _recording_id: &str,
        _limit: usize,
    ) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_unfinished_recording_starts(&self, _limit: usize) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_experiments(&self, _limit: usize) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_experiment_events(
        &self,
        _experiment_id: &str,
        _limit: usize,
    ) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_tool_calls(&self, _session_id: &str, _limit: usize) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_all_tool_calls(
        &self,
        _limit: usize,
        _session_id: Option<&str>,
    ) -> Vec<serde_json::Value> {
        vec![]
    }

    #[must_use]
    pub fn list_feedback_reports(
        &self,
        _cutoff_ms: Option<i64>,
        _dcc: Option<&str>,
        _severity: Option<&str>,
        _limit: usize,
    ) -> Vec<serde_json::Value> {
        vec![]
    }

    /// #2297-A3: no-op without `persist-sqlite`.
    #[must_use]
    pub fn get_script_promotion_counter(
        &self,
        _sha256: &str,
        _dcc_type: &str,
        _tool_name: &str,
    ) -> Option<ScriptPromotionCounter> {
        None
    }

    /// #2297-A3: no-op without `persist-sqlite`.
    #[must_use]
    pub fn list_script_promotion_counters(&self, _limit: usize) -> Vec<ScriptPromotionCounter> {
        vec![]
    }
}

#[cfg(not(feature = "persist-sqlite"))]
#[derive(Clone)]
pub struct AdminSqliteLane;

#[cfg(not(feature = "persist-sqlite"))]
impl AdminSqliteLane {
    pub fn spawn(_path: PathBuf, _retention_days: u32) -> Result<Self, String> {
        Ok(Self)
    }

    #[must_use]
    pub fn reader(&self) -> AdminSqliteReader {
        AdminSqliteReader::new(PathBuf::new())
    }

    pub fn try_persist_trace(&self, _: &DispatchTrace) {}

    pub fn try_persist_audit(&self, _: &AdminAuditRecord) {}

    pub fn try_persist_deregistered_instance(
        &self,
        _: &dcc_mcp_transport::discovery::types::ServiceEntry,
        _: &str,
    ) {
    }

    pub fn try_persist_session_event(&self, _: &serde_json::Value) {}

    pub fn try_bump_script_promotion_counter(&self, _: &dcc_mcp_db::ScriptPromotionBumpJson) {}

    #[must_use]
    pub fn try_add_skill_path(&self, _: String) -> bool {
        false
    }

    #[must_use]
    pub fn try_delete_skill_path(&self, _: i64) -> bool {
        false
    }

    #[must_use]
    pub fn try_delete_agent_memory(
        &self,
        _: Option<i64>,
        _: Option<String>,
        _: Option<String>,
        _: Option<String>,
        _: Option<String>,
    ) -> bool {
        false
    }

    pub fn try_persist_feedback_report(&self, _: &FeedbackReportRow) {}
}

#[cfg(not(feature = "persist-sqlite"))]
#[must_use]
pub fn read_custom_skill_paths_for_startup(_: &Path) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(all(test, feature = "persist-sqlite"))]
mod tests {
    use super::{AdminSqliteLane, AdminSqliteReader};
    use crate::DispatchTrace;
    use crate::{FeedbackReportRow, FeedbackSubmissionKind};
    use std::time::SystemTime;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_trace() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("t.sqlite");
        let lane = AdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let t = DispatchTrace {
            request_id: "r1".into(),
            trace_id: "trace-sqlite".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("x".into()),
            instance_id: None,
            session_id: None,
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: 12,
            ok: true,
            spans: vec![],
            input: None,
            output: None,
            script_execution: None,
            token_accounting: None,
            llm_usage: None,
        };
        lane.try_persist_trace(&t);
        drop(lane);
        let r = AdminSqliteReader::new(db);
        let list = r.list_traces_since(None, 10);
        assert!(list.iter().any(|x| x.request_id == "r1"));
    }

    #[test]
    fn roundtrip_recording_projection_uses_existing_session_events() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("recording.sqlite");
        let lane = AdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        lane.try_persist_session_event(&serde_json::json!({
            "session_id": "task-recording",
            "event_type": "recording.stopped",
            "created_at_ms": 42,
            "recording_id": "rec-1",
        }));
        drop(lane);

        let events = AdminSqliteReader::new(db).list_session_events("task-recording", 10);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event_type"], "recording.stopped");
        assert_eq!(events[0]["recording_id"], "rec-1");
    }
    fn feedback_row(
        id: &str,
        timestamp_ms: i64,
        dcc_type: &str,
        severity: &str,
    ) -> FeedbackReportRow {
        FeedbackReportRow {
            id: id.to_string(),
            timestamp_ms,
            recorded_at_ms: timestamp_ms,
            recorded_at: "2026-09-21T17:22:56.000Z".to_string(),
            kind: FeedbackSubmissionKind::Finding,
            schema_version: 1,
            fingerprint: Some(format!("sha256:{}", "a".repeat(64))),
            severity: severity.to_string(),
            dcc_type: dcc_type.to_string(),
            instance_id: Some("instance-1".to_string()),
            tool_slug: Some("maya_scene__save".to_string()),
            report: serde_json::json!({
                "id": id,
                "timestamp": timestamp_ms as f64 / 1000.0,
                "dcc_type": dcc_type,
                "severity": severity,
            }),
        }
    }

    #[test]
    fn roundtrip_feedback_report() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("feedback.sqlite");
        let lane = AdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        lane.try_persist_feedback_report(&feedback_row(
            "fb-1",
            1_700_000_000_000,
            "maya",
            "blocked",
        ));
        lane.try_persist_feedback_report(&feedback_row(
            "fb-2",
            1_700_000_000_500,
            "houdini",
            "degraded",
        ));
        drop(lane);

        let reader = AdminSqliteReader::new(db);
        let rows = reader.list_feedback_reports(None, None, None, 10);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "fb-2", "newest first");
        assert_eq!(rows[1]["id"], "fb-1");
        assert_eq!(rows[0]["dcc_type"], "houdini");

        let maya_only = reader.list_feedback_reports(None, Some("maya"), None, 10);
        assert_eq!(maya_only.len(), 1);
        assert_eq!(maya_only[0]["id"], "fb-1");
    }
}
