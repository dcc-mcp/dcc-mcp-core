//! Session model — first-class entity for session lifecycle tracking (PIP-2751).
//!
//! Provides a formal Session struct with start/end time, status, parent-child
//! relationship, tool-call and error counts, and disconnect/crash reason.

use serde::{Deserialize, Serialize};

/// Status of an MCP session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Session is currently active.
    Active,
    /// Session ended normally.
    Ended,
    /// Session ended due to a disconnect.
    Disconnected,
    /// Session ended due to a host (DCC) crash.
    Crashed,
    /// Session ended due to a GPU crash.
    GpuCrashed,
    /// Session timed out.
    TimedOut,
    /// Session was cancelled.
    Cancelled,
    /// Session had a thread-affinity failure.
    ThreadAffinityFailure,
}

/// End reason detail when a session terminates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReason {
    /// Normal shutdown.
    Normal,
    /// Network/transport disconnect.
    Disconnect { detail: String },
    /// Host DCC process crash.
    HostCrash { detail: String },
    /// GPU driver or device crash.
    GpuCrash { detail: String },
    /// Client-initiated timeout.
    Timeout { detail: String },
    /// Explicit cancellation.
    Cancelled { detail: String },
    /// Thread affinity failure (DCC main-thread contract violation).
    ThreadAffinityFailure { detail: String },
}

/// A first-class session entity for the observability system (PIP-2751).
///
/// Tracks the full lifecycle of an MCP session: creation, activity, termination,
/// parent-child relationships, and aggregated statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier (matches `mcp.session_id` span key).
    pub session_id: String,

    /// Parent session identifier for nested/child sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,

    /// DCC application type (e.g. `"maya"`, `"blender"`).
    pub dcc_type: String,

    /// DCC instance identifier (stable per running DCC process).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,

    /// Current session status.
    pub status: SessionStatus,

    /// Millisecond timestamp when the session was created.
    pub started_at_ms: i64,

    /// Millisecond timestamp of last observed activity.
    pub last_activity_at_ms: i64,

    /// Millisecond timestamp when the session ended (None if still active).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<i64>,

    /// Structured end reason (None if still active).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<SessionEndReason>,

    /// Total number of tool calls made in this session.
    pub tool_call_count: u64,

    /// Number of failed tool calls in this session.
    pub error_count: u64,

    /// DCC Core version string at session start.
    pub core_version: String,

    /// DCC adapter version string (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_version: Option<String>,

    /// Build SHA of the running server binary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_sha: Option<String>,
}

impl Session {
    /// Create a new active session.
    #[must_use]
    pub fn new(
        session_id: String,
        dcc_type: String,
        started_at_ms: i64,
        core_version: String,
    ) -> Self {
        Self {
            session_id,
            parent_session_id: None,
            dcc_type,
            instance_id: None,
            status: SessionStatus::Active,
            started_at_ms,
            last_activity_at_ms: started_at_ms,
            ended_at_ms: None,
            end_reason: None,
            tool_call_count: 0,
            error_count: 0,
            core_version,
            adapter_version: None,
            build_sha: None,
        }
    }

    /// Mark the session as ended with the given reason.
    pub fn end(&mut self, end_reason: SessionEndReason, ended_at_ms: i64) {
        self.status = match &end_reason {
            SessionEndReason::Normal => SessionStatus::Ended,
            SessionEndReason::Disconnect { .. } => SessionStatus::Disconnected,
            SessionEndReason::HostCrash { .. } => SessionStatus::Crashed,
            SessionEndReason::GpuCrash { .. } => SessionStatus::GpuCrashed,
            SessionEndReason::Timeout { .. } => SessionStatus::TimedOut,
            SessionEndReason::Cancelled { .. } => SessionStatus::Cancelled,
            SessionEndReason::ThreadAffinityFailure { .. } => SessionStatus::ThreadAffinityFailure,
        };
        self.end_reason = Some(end_reason);
        self.ended_at_ms = Some(ended_at_ms);
    }

    /// Update last activity timestamp.
    pub fn touch(&mut self, at_ms: i64) {
        self.last_activity_at_ms = at_ms;
    }

    /// Record a tool call result.
    pub fn record_tool_call(&mut self, success: bool) {
        self.tool_call_count += 1;
        if !success {
            self.error_count += 1;
        }
    }

    /// Set parent session for nested/child sessions.
    pub fn with_parent(mut self, parent_session_id: String) -> Self {
        self.parent_session_id = Some(parent_session_id);
        self
    }

    /// Set instance ID.
    pub fn with_instance(mut self, instance_id: String) -> Self {
        self.instance_id = Some(instance_id);
        self
    }

    /// Set adapter version.
    pub fn with_adapter_version(mut self, version: String) -> Self {
        self.adapter_version = Some(version);
        self
    }

    /// Set build SHA.
    pub fn with_build_sha(mut self, sha: String) -> Self {
        self.build_sha = Some(sha);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_is_active() {
        let s = Session::new("s1".into(), "maya".into(), 1000, "0.19.59".into());
        assert_eq!(s.status, SessionStatus::Active);
        assert!(s.ended_at_ms.is_none());
        assert!(s.end_reason.is_none());
        assert_eq!(s.tool_call_count, 0);
        assert_eq!(s.error_count, 0);
    }

    #[test]
    fn end_session_transitions_status() {
        let mut s = Session::new("s1".into(), "maya".into(), 1000, "0.19.59".into());
        s.end(
            SessionEndReason::HostCrash {
                detail: "segfault".into(),
            },
            2000,
        );
        assert_eq!(s.status, SessionStatus::Crashed);
        assert_eq!(s.ended_at_ms, Some(2000));
        assert!(s.end_reason.is_some());
    }

    #[test]
    fn record_tool_calls_tracks_counts() {
        let mut s = Session::new("s1".into(), "maya".into(), 1000, "0.19.59".into());
        s.record_tool_call(true);
        s.record_tool_call(true);
        s.record_tool_call(false);
        assert_eq!(s.tool_call_count, 3);
        assert_eq!(s.error_count, 1);
    }

    #[test]
    fn parent_child_relationship() {
        let s = Session::new("child".into(), "maya".into(), 1000, "0.19.59".into())
            .with_parent("parent".into());
        assert_eq!(s.parent_session_id.as_deref(), Some("parent"));
    }

    #[test]
    fn serialization_round_trip() {
        let mut s = Session::new(
            "s1".into(),
            "blender".into(),
            1_700_000_000_000,
            "0.19.59".into(),
        );
        s.end(SessionEndReason::Normal, 1_700_000_001_000);
        s.record_tool_call(true);
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.session_id, "s1");
        assert_eq!(back.status, SessionStatus::Ended);
        assert_eq!(back.tool_call_count, 1);
    }

    #[test]
    fn end_reason_variants() {
        let s = Session::new("s1".into(), "maya".into(), 1000, "0.19.59".into());
        let cases = vec![
            (SessionEndReason::Normal, SessionStatus::Ended),
            (
                SessionEndReason::Disconnect {
                    detail: "tcp reset".into(),
                },
                SessionStatus::Disconnected,
            ),
            (
                SessionEndReason::HostCrash {
                    detail: "oom".into(),
                },
                SessionStatus::Crashed,
            ),
            (
                SessionEndReason::GpuCrash {
                    detail: "driver timeout".into(),
                },
                SessionStatus::GpuCrashed,
            ),
            (
                SessionEndReason::Timeout {
                    detail: "idle 300s".into(),
                },
                SessionStatus::TimedOut,
            ),
            (
                SessionEndReason::Cancelled {
                    detail: "user abort".into(),
                },
                SessionStatus::Cancelled,
            ),
            (
                SessionEndReason::ThreadAffinityFailure {
                    detail: "main thread required".into(),
                },
                SessionStatus::ThreadAffinityFailure,
            ),
        ];
        for (reason, expected_status) in cases {
            let mut s2 = s.clone();
            s2.end(reason, 2000);
            assert_eq!(s2.status, expected_status);
            assert_eq!(s2.ended_at_ms, Some(2000));
        }
    }
}
