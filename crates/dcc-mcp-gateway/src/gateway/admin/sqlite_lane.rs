//! SQLite-backed admin persistence (traces, audits, custom skill paths).
//!
//! When the `admin-persist-sqlite` feature is off, this module exposes no-op
//! stubs so `admin`-only test builds keep compiling.
//!
//! The writer thread and schema live in `dcc-mcp-db` (`gateway-admin-sqlite`);
//! this module is a thin type-preserving façade over [`DispatchTrace`] /
//! [`AdminAuditRecord`].

use std::path::PathBuf;

#[cfg(not(feature = "admin-persist-sqlite"))]
use std::path::Path;
use std::time::SystemTime;

#[cfg(not(feature = "admin-persist-sqlite"))]
use super::state::AdminAuditRecord;
#[cfg(not(feature = "admin-persist-sqlite"))]
use super::trace::DispatchTrace;

#[cfg(feature = "admin-persist-sqlite")]
use std::time::{Duration, UNIX_EPOCH};

#[cfg(feature = "admin-persist-sqlite")]
use super::state::AdminAuditRecord;
#[cfg(feature = "admin-persist-sqlite")]
use super::trace::DispatchTrace;
#[cfg(feature = "admin-persist-sqlite")]
use dcc_mcp_db::{
    GatewayAdminAuditPersistedJson, GatewayAdminSqliteLane as InnerLane,
    GatewayAdminSqliteReader as InnerReader, GatewayDeregisteredInstanceJson,
};

#[cfg(feature = "admin-persist-sqlite")]
#[derive(Clone)]
pub struct AdminSqliteReader {
    inner: InnerReader,
}

#[cfg(feature = "admin-persist-sqlite")]
impl AdminSqliteReader {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            inner: InnerReader::new(path),
        }
    }

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

    pub fn get_trace(&self, request_id: &str) -> Option<DispatchTrace> {
        let s = self.inner.get_trace_json(request_id)?;
        serde_json::from_str(&s).ok()
    }

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

    pub fn list_custom_skill_paths(&self) -> Vec<(i64, String)> {
        self.inner.list_custom_skill_paths()
    }

    pub fn list_deregistered_instances(&self, limit: usize) -> Vec<serde_json::Value> {
        self.inner
            .list_deregistered_instances_json(limit)
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }
}

#[cfg(feature = "admin-persist-sqlite")]
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
        token_accounting: p
            .token_accounting
            .and_then(|value| serde_json::from_value(value).ok()),
        llm_usage: p
            .llm_usage
            .and_then(|value| serde_json::from_value(value).ok()),
    }
}

#[cfg(feature = "admin-persist-sqlite")]
#[derive(Clone)]
pub struct AdminSqliteLane {
    inner: InnerLane,
}

#[cfg(feature = "admin-persist-sqlite")]
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

    pub fn try_add_skill_path(&self, path: String) -> bool {
        self.inner.try_add_skill_path(path)
    }

    pub fn try_delete_skill_path(&self, id: i64) -> bool {
        self.inner.try_delete_skill_path(id)
    }
}

#[cfg(feature = "admin-persist-sqlite")]
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

#[cfg(feature = "admin-persist-sqlite")]
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

#[cfg(feature = "admin-persist-sqlite")]
pub use dcc_mcp_db::read_custom_skill_paths_for_startup;

#[cfg(not(feature = "admin-persist-sqlite"))]
#[derive(Clone, Default)]
pub struct AdminSqliteReader;

#[cfg(not(feature = "admin-persist-sqlite"))]
impl AdminSqliteReader {
    #[must_use]
    pub fn new(_path: PathBuf) -> Self {
        Self
    }

    pub fn list_traces_since(
        &self,
        _cutoff: Option<SystemTime>,
        _limit: usize,
    ) -> Vec<DispatchTrace> {
        vec![]
    }

    pub fn get_trace(&self, _request_id: &str) -> Option<DispatchTrace> {
        None
    }

    pub fn list_audits_recent(&self, _limit: usize) -> Vec<AdminAuditRecord> {
        vec![]
    }

    pub fn list_custom_skill_paths(&self) -> Vec<(i64, String)> {
        vec![]
    }

    pub fn list_deregistered_instances(&self, _limit: usize) -> Vec<serde_json::Value> {
        vec![]
    }
}

#[cfg(not(feature = "admin-persist-sqlite"))]
#[derive(Clone)]
pub struct AdminSqliteLane;

#[cfg(not(feature = "admin-persist-sqlite"))]
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

    pub fn try_add_skill_path(&self, _: String) -> bool {
        false
    }

    pub fn try_delete_skill_path(&self, _: i64) -> bool {
        false
    }
}

#[cfg(not(feature = "admin-persist-sqlite"))]
pub fn read_custom_skill_paths_for_startup(_: &Path) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(all(test, feature = "admin-persist-sqlite"))]
mod tests {
    use super::{AdminSqliteLane, AdminSqliteReader};
    use crate::gateway::admin::trace::DispatchTrace;
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
            token_accounting: None,
            llm_usage: None,
        };
        lane.try_persist_trace(&t);
        drop(lane);
        let r = AdminSqliteReader::new(db);
        let list = r.list_traces_since(None, 10);
        assert!(list.iter().any(|x| x.request_id == "r1"));
    }
}
