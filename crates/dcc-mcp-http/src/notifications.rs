//! Job and workflow lifecycle notifications (#326).
//!
//! Three SSE channels share a single [`JobNotifier`] service:
//!
//! | Channel | Method | Fires when |
//! |---------|--------|-----------|
//! | A | `notifications/progress` (MCP 2025-03-26) | A job the caller tagged with `_meta.progressToken` transitions state. |
//! | B | `notifications/$/dcc.jobUpdated` | A job transitions state AND `McpHttpConfig::enable_job_notifications` is `true`. |
//! | C | `notifications/$/dcc.workflowUpdated` | [`JobNotifier::emit_workflow_update`] is called (wired by #348 execution PR). |
//!
//! The notifier subscribes to [`crate::job::JobManager`] (see
//! [`JobManager::subscribe`](crate::job::JobManager::subscribe)) and routes each
//! [`JobEvent`](crate::job::JobEvent) to the SSE stream of the owning session.
//!
//! # Session correlation
//!
//! Jobs are created inside `handle_tools_call`; the notifier needs to know
//! which session and (optional) `progressToken` owns each job so it can
//! route the emission to the right SSE subscriber. Callers register that
//! mapping via [`JobNotifier::register_job`] immediately after
//! `JobManager::create`.

use std::sync::Arc;

use dashmap::DashMap;
use serde::Serialize;
use serde_json::{Value, json};

use crate::job::{JobEvent, JobStatus};
use crate::protocol::format_sse_event;
use crate::session::SessionManager;

// ── Workflow types ───────────────────────────────────────────────────────

/// Progress counters for a workflow-level transition.
///
/// Shape matches [`dcc_mcp_workflow::WorkflowProgress`] so the two crates
/// can be bridged trivially.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct WorkflowProgress {
    /// Number of steps that finished successfully so far.
    pub completed_steps: u32,
    /// Total number of steps in the workflow.
    pub total_steps: u32,
}

impl From<dcc_mcp_workflow::WorkflowProgress> for WorkflowProgress {
    fn from(p: dcc_mcp_workflow::WorkflowProgress) -> Self {
        Self {
            completed_steps: p.completed_steps,
            total_steps: p.total_steps,
        }
    }
}

/// Workflow-level transition event.
///
/// Emitted on three moments: step enter, step terminal, workflow terminal.
/// The payload shape is defined in
/// `docs/proposals/workflow-orchestration-gap.md` §5.
#[derive(Debug, Clone)]
pub struct WorkflowUpdate {
    /// Workflow spec id (runtime job id for workflow).
    pub workflow_id: uuid::Uuid,
    /// Correlating job id (the `WorkflowJob` outer job wrapping execution).
    pub job_id: uuid::Uuid,
    /// Aggregated workflow status after the transition.
    pub status: dcc_mcp_workflow::WorkflowStatus,
    /// Id of the step that just entered / exited, if any.
    pub current_step_id: Option<String>,
    /// Progress counters.
    pub progress: WorkflowProgress,
}

// ── Session correlation ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct SessionLink {
    session_id: String,
    progress_token: Option<Value>,
}

// ── JobNotifier ──────────────────────────────────────────────────────────

/// Bridges [`JobManager`](crate::job::JobManager) state transitions and
/// [`WorkflowUpdate`] events onto MCP SSE notifications.
///
/// Clone cheaply — internal state is `Arc`-wrapped.
#[derive(Clone)]
pub struct JobNotifier {
    sessions: SessionManager,
    /// Job id → (session_id, progressToken).
    links: Arc<DashMap<String, SessionLink>>,
    /// Session id → subscribed to `$/dcc.*` notifications.
    /// Always populated with the session id when
    /// `enable_job_notifications` is `true`; empty otherwise.
    job_sessions: Arc<DashMap<String, ()>>,
    /// Session id → subscribed to workflow updates (currently the same set
    /// as `job_sessions` — kept separate so future capability negotiation
    /// can decouple them).
    workflow_sessions: Arc<DashMap<String, ()>>,
    /// Whether `$/dcc.jobUpdated` emission is globally enabled.
    job_updates_enabled: bool,
}

impl std::fmt::Debug for JobNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobNotifier")
            .field("links", &self.links.len())
            .field("job_sessions", &self.job_sessions.len())
            .field("workflow_sessions", &self.workflow_sessions.len())
            .field("job_updates_enabled", &self.job_updates_enabled)
            .finish()
    }
}

impl JobNotifier {
    /// Create a new notifier bound to `sessions`.
    pub fn new(sessions: SessionManager, job_updates_enabled: bool) -> Self {
        Self {
            sessions,
            links: Arc::new(DashMap::new()),
            job_sessions: Arc::new(DashMap::new()),
            workflow_sessions: Arc::new(DashMap::new()),
            job_updates_enabled,
        }
    }

    /// Whether `$/dcc.jobUpdated` emission is globally enabled.
    pub fn job_updates_enabled(&self) -> bool {
        self.job_updates_enabled
    }

    /// Register a session to receive `$/dcc.jobUpdated` and
    /// `$/dcc.workflowUpdated` notifications.
    ///
    /// Called from the session lifecycle (typically on `initialize`).
    /// No-op when `enable_job_notifications` is `false`.
    pub fn subscribe_session(&self, session_id: &str) {
        if !self.job_updates_enabled {
            return;
        }
        self.job_sessions.insert(session_id.to_string(), ());
        self.workflow_sessions.insert(session_id.to_string(), ());
    }

    /// Forget a session — called when the session is evicted / deleted.
    pub fn unsubscribe_session(&self, session_id: &str) {
        self.job_sessions.remove(session_id);
        self.workflow_sessions.remove(session_id);
        self.links.retain(|_, v| v.session_id != session_id);
    }

    /// Record that `job_id` belongs to `session_id` and (optionally) carries
    /// `progress_token` from the caller's `_meta.progressToken`.
    pub fn register_job(&self, job_id: &str, session_id: &str, progress_token: Option<Value>) {
        self.links.insert(
            job_id.to_string(),
            SessionLink {
                session_id: session_id.to_string(),
                progress_token,
            },
        );
    }

    /// Forget a job mapping. Terminal-state emission still goes out before
    /// the caller forgets the mapping, so this is safe to call at any time.
    pub fn forget_job(&self, job_id: &str) {
        self.links.remove(job_id);
    }

    /// Handle a [`JobEvent`] emitted by [`JobManager`](crate::job::JobManager).
    ///
    /// Fires channel A (if a `progressToken` is known) and channel B
    /// (if `enable_job_notifications` is on and the owning session is
    /// subscribed).
    pub fn on_job_event(&self, event: JobEvent) {
        let Some(link) = self.links.get(&event.id).map(|e| e.value().clone()) else {
            // No session registered — nothing to push.
            return;
        };

        // ── Channel A: notifications/progress ────────────────────────────
        if let Some(token) = link.progress_token.as_ref() {
            let (progress, total, message) = progress_mapping(&event);
            let notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": token,
                    "progress": progress,
                    "total": total,
                    "message": message,
                },
            });
            self.sessions
                .push_event(&link.session_id, format_sse_event(&notification, None));
        }

        // ── Channel B: notifications/$/dcc.jobUpdated ────────────────────
        if self.job_updates_enabled && self.job_sessions.contains_key(&link.session_id) {
            let notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/$/dcc.jobUpdated",
                "params": {
                    "job_id": event.id,
                    "parent_job_id": Value::Null,
                    "tool": event.tool_name,
                    "status": status_str(event.status),
                    "started_at": started_at(&event),
                    "completed_at": completed_at(&event),
                    "error": event.error,
                },
            });
            self.sessions
                .push_event(&link.session_id, format_sse_event(&notification, None));
        }

        if event.status.is_terminal() {
            self.forget_job(&event.id);
        }
    }

    /// Emit a workflow-level transition (channel C).
    ///
    /// Called by the workflow executor (landing in #348). The current PR
    /// ships the emit API and routing; the executor will invoke this once
    /// step execution is implemented.
    pub fn emit_workflow_update(&self, upd: WorkflowUpdate) {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/$/dcc.workflowUpdated",
            "params": {
                "workflow_id": upd.workflow_id.to_string(),
                "job_id": upd.job_id.to_string(),
                "status": upd.status.as_str(),
                "current_step_id": upd.current_step_id,
                "progress": {
                    "completed_steps": upd.progress.completed_steps,
                    "total_steps": upd.progress.total_steps,
                },
            },
        });
        if !self.job_updates_enabled {
            return;
        }
        let event = format_sse_event(&notification, None);
        for kv in self.workflow_sessions.iter() {
            self.sessions.push_event(kv.key(), event.clone());
        }
    }

    /// Synthesise a `$/dcc.workflowUpdated` SSE frame without pushing it.
    ///
    /// Useful for tests that want to assert the payload shape without
    /// standing up a full session.
    pub fn workflow_update_payload(upd: &WorkflowUpdate) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/$/dcc.workflowUpdated",
            "params": {
                "workflow_id": upd.workflow_id.to_string(),
                "job_id": upd.job_id.to_string(),
                "status": upd.status.as_str(),
                "current_step_id": upd.current_step_id,
                "progress": {
                    "completed_steps": upd.progress.completed_steps,
                    "total_steps": upd.progress.total_steps,
                },
            },
        })
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn status_str(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Pending => "pending",
        JobStatus::Running => "running",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
        JobStatus::Interrupted => "interrupted",
    }
}

fn progress_mapping(event: &JobEvent) -> (u64, u64, String) {
    // If the tool supplied its own fine-grained progress, surface that.
    if let Some(p) = &event.progress {
        let msg = p
            .message
            .clone()
            .unwrap_or_else(|| status_str(event.status).to_string());
        return (p.current, p.total.max(1), msg);
    }
    match event.status {
        JobStatus::Pending => (0, 100, "pending".to_string()),
        JobStatus::Running => (10, 100, "running".to_string()),
        JobStatus::Completed => (100, 100, "completed".to_string()),
        JobStatus::Failed => (100, 100, "failed".to_string()),
        JobStatus::Cancelled => (100, 100, "cancelled".to_string()),
        JobStatus::Interrupted => (100, 100, "interrupted".to_string()),
    }
}

fn started_at(event: &JobEvent) -> Option<String> {
    match event.status {
        JobStatus::Pending => None,
        _ => Some(event.updated_at.to_rfc3339()),
    }
}

fn completed_at(event: &JobEvent) -> Option<String> {
    if event.status.is_terminal() {
        Some(event.updated_at.to_rfc3339())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobManager;
    use serde_json::Value;

    fn poll_event(rx: &mut tokio::sync::broadcast::Receiver<String>) -> Option<Value> {
        match rx.try_recv() {
            Ok(s) => {
                let body = s.strip_prefix("data: ")?.trim();
                serde_json::from_str(body).ok()
            }
            Err(_) => None,
        }
    }

    fn drain_events(rx: &mut tokio::sync::broadcast::Receiver<String>) -> Vec<Value> {
        let mut out = Vec::new();
        while let Some(v) = poll_event(rx) {
            out.push(v);
        }
        out
    }

    #[test]
    fn progress_channel_echoes_token_and_maps_status() {
        let sessions = SessionManager::new();
        let sid = sessions.create();
        let mut rx = sessions.subscribe(&sid).expect("subscriber");

        let notifier = JobNotifier::new(sessions.clone(), true);
        notifier.subscribe_session(&sid);

        let jm = JobManager::new();
        let notifier_cb = notifier.clone();
        jm.subscribe(move |ev| notifier_cb.on_job_event(ev));

        let job = jm.create("scene.get_info");
        let id = job.read().id.clone();
        notifier.register_job(&id, &sid, Some(Value::String("tok-1".into())));

        jm.start(&id).unwrap();
        jm.complete(&id, serde_json::json!({"ok": true})).unwrap();

        let events = drain_events(&mut rx);
        // Expect at least: running + completed on BOTH channels.
        let progress: Vec<_> = events
            .iter()
            .filter(|e| e["method"] == "notifications/progress")
            .collect();
        assert!(
            progress.len() >= 2,
            "progress events missing: got {events:?}"
        );
        for ev in &progress {
            assert_eq!(ev["params"]["progressToken"], Value::String("tok-1".into()));
        }
        let completed = progress
            .iter()
            .find(|e| e["params"]["message"] == "completed")
            .expect("completed progress");
        assert_eq!(completed["params"]["progress"], 100);
        assert_eq!(completed["params"]["total"], 100);
    }

    #[test]
    fn job_updated_channel_gated_by_flag_and_session_subscription() {
        // Disabled globally → no $/dcc.jobUpdated.
        let sessions = SessionManager::new();
        let sid = sessions.create();
        let mut rx = sessions.subscribe(&sid).expect("sub");
        let notifier = JobNotifier::new(sessions.clone(), false);
        notifier.subscribe_session(&sid); // no-op when disabled

        let jm = JobManager::new();
        let cb = notifier.clone();
        jm.subscribe(move |e| cb.on_job_event(e));

        let job = jm.create("t");
        let id = job.read().id.clone();
        notifier.register_job(&id, &sid, None);
        jm.start(&id).unwrap();
        jm.complete(&id, serde_json::Value::Null).unwrap();

        let evts = drain_events(&mut rx);
        assert!(
            evts.iter()
                .all(|e| e["method"] != "notifications/$/dcc.jobUpdated"),
            "disabled flag leaked events: {evts:?}"
        );
    }

    #[test]
    fn job_updated_channel_fires_on_every_transition() {
        let sessions = SessionManager::new();
        let sid = sessions.create();
        let mut rx = sessions.subscribe(&sid).expect("sub");
        let notifier = JobNotifier::new(sessions.clone(), true);
        notifier.subscribe_session(&sid);

        let jm = JobManager::new();
        let cb = notifier.clone();
        jm.subscribe(move |e| cb.on_job_event(e));

        let job = jm.create("t");
        let id = job.read().id.clone();
        notifier.register_job(&id, &sid, None);
        jm.start(&id).unwrap();
        jm.fail(&id, "boom").unwrap();

        let evts = drain_events(&mut rx);
        let updates: Vec<_> = evts
            .iter()
            .filter(|e| e["method"] == "notifications/$/dcc.jobUpdated")
            .collect();
        // pending (on register happens before create emits... create emits
        // pending before register_job runs, so pending is skipped by the
        // router — that's the documented behaviour). Expect running +
        // failed at minimum.
        assert!(updates.len() >= 2, "got: {updates:?}");
        assert_eq!(
            updates.last().unwrap()["params"]["status"],
            Value::String("failed".into()),
        );
        assert_eq!(
            updates.last().unwrap()["params"]["error"],
            Value::String("boom".into()),
        );
    }

    #[test]
    fn emit_workflow_update_synthesises_expected_payload() {
        use dcc_mcp_workflow::WorkflowStatus;
        let upd = WorkflowUpdate {
            workflow_id: uuid::Uuid::nil(),
            job_id: uuid::Uuid::nil(),
            status: WorkflowStatus::Running,
            current_step_id: Some("step-1".into()),
            progress: WorkflowProgress {
                completed_steps: 1,
                total_steps: 3,
            },
        };
        let payload = JobNotifier::workflow_update_payload(&upd);
        assert_eq!(payload["method"], "notifications/$/dcc.workflowUpdated");
        assert_eq!(payload["params"]["status"], "running");
        assert_eq!(payload["params"]["current_step_id"], "step-1");
        assert_eq!(payload["params"]["progress"]["completed_steps"], 1);
        assert_eq!(payload["params"]["progress"]["total_steps"], 3);
    }

    #[test]
    fn workflow_update_goes_to_all_subscribed_sessions() {
        use dcc_mcp_workflow::WorkflowStatus;
        let sessions = SessionManager::new();
        let s1 = sessions.create();
        let s2 = sessions.create();
        let mut r1 = sessions.subscribe(&s1).unwrap();
        let mut r2 = sessions.subscribe(&s2).unwrap();

        let n = JobNotifier::new(sessions.clone(), true);
        n.subscribe_session(&s1);
        n.subscribe_session(&s2);

        n.emit_workflow_update(WorkflowUpdate {
            workflow_id: uuid::Uuid::nil(),
            job_id: uuid::Uuid::nil(),
            status: WorkflowStatus::Completed,
            current_step_id: None,
            progress: WorkflowProgress::default(),
        });

        let e1 = drain_events(&mut r1);
        let e2 = drain_events(&mut r2);
        assert!(
            e1.iter()
                .any(|e| e["method"] == "notifications/$/dcc.workflowUpdated")
        );
        assert!(
            e2.iter()
                .any(|e| e["method"] == "notifications/$/dcc.workflowUpdated")
        );
    }
}
