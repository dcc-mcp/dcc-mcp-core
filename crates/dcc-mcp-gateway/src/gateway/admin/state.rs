//! Shared state for the admin UI handlers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;
use serde_json::Value;

use crate::gateway::state::GatewayState;

use super::infra::StatsAggregator;
use super::trace::TraceLog;

pub use dcc_mcp_gateway_admin::{AdminAuditRecord, AuditLog, DurableAuditStore};

/// State injected into every admin handler via axum's `State` extractor.
#[derive(Clone)]
pub struct AdminState {
    /// Live gateway state — registry, capability index, server metadata.
    pub gateway: GatewayState,
    /// Audit log ring buffer — `None` until `with_audit_log` is called.
    pub audit_log: Option<Arc<AuditLog>>,
    /// Phase 2 trace log — `None` until `with_trace_log` is called.
    pub trace_log: Option<Arc<TraceLog>>,
    /// Phase 3 stats aggregator — `None` until `with_trace_log` is called.
    pub stats: Option<Arc<StatsAggregator>>,
    /// Wall-clock time the gateway started, used for the Health card uptime.
    pub started_at: SystemTime,
    /// Skill search path snapshot (CLI / env / bundled) for the admin UI.
    pub skill_paths_snapshot: Vec<crate::gateway::SkillPathEntry>,
    /// SQLite lane for custom skill path mutations from the admin API.
    pub admin_sqlite_lane: Option<crate::gateway::admin::sqlite_lane::AdminSqliteLane>,
    /// Optional embedder hook: re-run disk skill discovery after admin SQLite path changes.
    pub skill_paths_reload: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    /// Process-local integration configuration that has been submitted through the admin UI
    /// but cannot take effect until the gateway/server process restarts.
    pub pending_integrations: Arc<Mutex<HashMap<String, Value>>>,
    /// Explicit caller-scoped demonstration recordings.
    pub recordings: crate::gateway::record_replay::RecordReplayStore,
    /// Traffic subscription active only while at least one recording is running.
    pub recording_subscription: Arc<Mutex<Option<u64>>>,
}

impl AdminState {
    /// Build an [`AdminState`] backed by the live `GatewayState`. Audit /
    /// trace / stats logs default to `None`; attach them via the
    /// `with_*` builders before mounting the admin router.
    pub fn new(gateway: GatewayState) -> Self {
        Self {
            gateway,
            audit_log: None,
            trace_log: None,
            stats: None,
            started_at: SystemTime::now(),
            skill_paths_snapshot: Vec::new(),
            admin_sqlite_lane: None,
            skill_paths_reload: None,
            pending_integrations: Arc::new(Mutex::new(HashMap::new())),
            recordings: crate::gateway::record_replay::RecordReplayStore::default(),
            recording_subscription: Arc::new(Mutex::new(None)),
        }
    }

    /// Activate redacted traffic projection while demonstrations are running.
    pub fn ensure_recording_subscription(&self) {
        let mut subscription = self.recording_subscription.lock();
        if subscription.is_some() {
            return;
        }
        let store = self.recordings.clone();
        let lane = self.admin_sqlite_lane.clone();
        *subscription = Some(self.gateway.traffic_capture.subscribe_redacted_frames(
            move |frame| {
                if let Some(event) = store.capture_frame(frame)
                    && let Some(lane) = &lane
                {
                    lane.try_persist_session_event(&event);
                }
            },
        ));
    }

    /// Release traffic projection when the last demonstration stops.
    pub fn release_recording_subscription_if_idle(&self) {
        if self.recordings.active_count() != 0 {
            return;
        }
        let Some(subscription_id) = self.recording_subscription.lock().take() else {
            return;
        };
        let _ = self
            .gateway
            .traffic_capture
            .unsubscribe_redacted_frames(subscription_id);
    }

    /// Attach the [`AuditLog`] that `GET /admin/api/calls` reads from.
    pub fn with_audit_log(mut self, log: Arc<AuditLog>) -> Self {
        self.audit_log = Some(log);
        self
    }

    /// Attach the Phase 2 [`TraceLog`]. Implicitly bootstraps a
    /// [`StatsAggregator`] (Phase 3) over the same log so the admin
    /// router can serve `GET /admin/api/stats` without extra wiring.
    pub fn with_trace_log(
        mut self,
        log: Arc<TraceLog>,
        sqlite_reader: Option<crate::gateway::admin::sqlite_lane::AdminSqliteReader>,
    ) -> Self {
        let mut agg = StatsAggregator::new(log.clone());
        if let Some(r) = sqlite_reader {
            agg = agg.with_sqlite_reader(r);
        }
        self.stats = Some(Arc::new(agg));
        self.trace_log = Some(log);
        self
    }

    /// Attach skill path snapshot rows (from CLI / env / bundled).
    pub fn with_skill_paths_snapshot(mut self, paths: Vec<crate::gateway::SkillPathEntry>) -> Self {
        self.skill_paths_snapshot = paths;
        self
    }

    /// Attach SQLite lane for admin API skill-path mutations.
    pub fn with_admin_sqlite_lane(
        mut self,
        lane: Option<crate::gateway::admin::sqlite_lane::AdminSqliteLane>,
    ) -> Self {
        self.admin_sqlite_lane = lane;
        self.recover_interrupted_recordings();
        self
    }

    pub fn recording_for_caller(
        &self,
        session_id: &str,
        recording_id: &str,
    ) -> Option<crate::gateway::record_replay::RecordingDraft> {
        if let Some(draft) = self.recordings.get(session_id, recording_id) {
            return Some(draft);
        }
        let lane = self.admin_sqlite_lane.as_ref()?;
        let rows = lane.reader().list_recording_events(
            session_id,
            recording_id,
            crate::gateway::record_replay::MAX_RECORDING_EVENTS * 2 + 4,
        );
        let (draft, newly_interrupted) =
            crate::gateway::record_replay::recover_recording(recording_id, &rows)?;
        if draft.session_id != session_id {
            return None;
        }
        if newly_interrupted {
            lane.try_persist_session_event(
                &crate::gateway::record_replay::recording_session_event(
                    "recording.interrupted",
                    &draft,
                    Some(serde_json::json!({"reason": "gateway_restart"})),
                ),
            );
        }
        self.recordings.restore(draft.clone());
        Some(draft)
    }

    fn recover_interrupted_recordings(&self) {
        let Some(lane) = &self.admin_sqlite_lane else {
            return;
        };
        let reader = lane.reader();
        for started in reader.list_unfinished_recording_starts(1_000) {
            let Some(session_id) = started
                .get("session_id")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let Some(recording_id) = started
                .get("recording_id")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let rows = reader.list_recording_events(
                session_id,
                recording_id,
                crate::gateway::record_replay::MAX_RECORDING_EVENTS * 2 + 4,
            );
            let Some((draft, true)) =
                crate::gateway::record_replay::recover_recording(recording_id, &rows)
            else {
                continue;
            };
            lane.try_persist_session_event(
                &crate::gateway::record_replay::recording_session_event(
                    "recording.interrupted",
                    &draft,
                    Some(serde_json::json!({"reason": "gateway_restart"})),
                ),
            );
        }
    }

    /// Hook invoked after SQLite-backed custom skill paths change (add/delete).
    pub fn with_skill_paths_reload(
        mut self,
        cb: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        self.skill_paths_reload = cb;
        self
    }
}
