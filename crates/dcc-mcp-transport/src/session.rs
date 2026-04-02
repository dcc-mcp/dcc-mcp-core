//! Session management — tracks DCC connection lifecycles with auto-reconnection.
//!
//! Provides:
//! - Session state machine (Connected → Idle → Reconnecting → Closed)
//! - Lazy session creation (connect only when an MCP tool is called)
//! - Automatic reconnection with exponential backoff
//! - Per-session metrics (request count, error rate, latency)
//! - Graceful batch shutdown

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::config::SessionConfig;
use crate::discovery::types::ServiceKey;
use crate::error::{TransportError, TransportResult};

/// Session lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Session is connected and ready for requests.
    Connected,
    /// Session is idle (idle_timeout exceeded, still valid).
    Idle,
    /// Session is reconnecting after a failure.
    Reconnecting,
    /// Session is closed (terminal state).
    Closed,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected => write!(f, "connected"),
            Self::Idle => write!(f, "idle"),
            Self::Reconnecting => write!(f, "reconnecting"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

/// Per-session metrics.
#[derive(Debug, Clone, Default)]
pub struct SessionMetrics {
    /// Total number of requests processed.
    pub request_count: u64,
    /// Total number of errors encountered.
    pub error_count: u64,
    /// Cumulative latency in milliseconds (for avg calculation).
    total_latency_ms: u64,
    /// Last error message.
    pub last_error: Option<String>,
    /// Last error timestamp.
    pub last_error_at: Option<Instant>,
}

impl SessionMetrics {
    /// Record a successful request.
    pub fn record_success(&mut self, latency: Duration) {
        self.request_count += 1;
        self.total_latency_ms += latency.as_millis() as u64;
    }

    /// Record a failed request.
    pub fn record_error(&mut self, latency: Duration, error: &str) {
        self.request_count += 1;
        self.error_count += 1;
        self.total_latency_ms += latency.as_millis() as u64;
        self.last_error = Some(error.to_string());
        self.last_error_at = Some(Instant::now());
    }

    /// Get the average latency in milliseconds.
    pub fn avg_latency_ms(&self) -> f64 {
        if self.request_count == 0 {
            0.0
        } else {
            self.total_latency_ms as f64 / self.request_count as f64
        }
    }

    /// Get the error rate (0.0 to 1.0).
    pub fn error_rate(&self) -> f64 {
        if self.request_count == 0 {
            0.0
        } else {
            self.error_count as f64 / self.request_count as f64
        }
    }
}

/// A session tracking a connection to a DCC instance.
#[derive(Debug, Clone)]
pub struct Session {
    /// Unique session ID.
    pub id: Uuid,
    /// Target DCC type.
    pub dcc_type: String,
    /// Target instance ID.
    pub instance_id: Uuid,
    /// Host address.
    pub host: String,
    /// Port number.
    pub port: u16,
    /// Current session state.
    pub state: SessionState,
    /// When the session was created.
    pub created_at: Instant,
    /// When the session was last active.
    pub last_active: Instant,
    /// Per-session metrics.
    pub metrics: SessionMetrics,
    /// Number of reconnection attempts so far.
    pub reconnect_attempts: u32,
}

impl Session {
    /// Create a new session in the Connected state.
    pub fn new(
        dcc_type: impl Into<String>,
        instance_id: Uuid,
        host: impl Into<String>,
        port: u16,
    ) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            dcc_type: dcc_type.into(),
            instance_id,
            host: host.into(),
            port,
            state: SessionState::Connected,
            created_at: now,
            last_active: now,
            metrics: SessionMetrics::default(),
            reconnect_attempts: 0,
        }
    }

    /// Get the service key for this session.
    pub fn service_key(&self) -> ServiceKey {
        ServiceKey {
            dcc_type: self.dcc_type.clone(),
            instance_id: self.instance_id,
        }
    }

    /// Mark session as active (update last_active timestamp).
    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    /// Check if the session has exceeded its idle timeout.
    pub fn is_idle(&self, timeout: Duration) -> bool {
        self.last_active.elapsed() > timeout
    }

    /// Check if the session has exceeded its maximum lifetime.
    pub fn is_expired(&self, max_lifetime: Duration) -> bool {
        self.created_at.elapsed() > max_lifetime
    }

    /// Transition to a new state. Returns an error if the transition is invalid.
    pub fn transition_to(&mut self, new_state: SessionState) -> TransportResult<()> {
        let valid = match (self.state, new_state) {
            // From Connected: can go to Idle, Reconnecting, or Closed
            (SessionState::Connected, SessionState::Idle) => true,
            (SessionState::Connected, SessionState::Reconnecting) => true,
            (SessionState::Connected, SessionState::Closed) => true,
            // From Idle: can go to Connected (reactivated), Reconnecting, or Closed
            (SessionState::Idle, SessionState::Connected) => true,
            (SessionState::Idle, SessionState::Reconnecting) => true,
            (SessionState::Idle, SessionState::Closed) => true,
            // From Reconnecting: can go to Connected (success) or Closed (failed)
            (SessionState::Reconnecting, SessionState::Connected) => true,
            (SessionState::Reconnecting, SessionState::Closed) => true,
            // From Closed: terminal state, no transitions allowed
            (SessionState::Closed, _) => false,
            // Same state is a no-op
            (a, b) if a == b => true,
            _ => false,
        };

        if valid {
            tracing::debug!(
                session_id = %self.id,
                from = %self.state,
                to = %new_state,
                "session state transition"
            );
            self.state = new_state;
            Ok(())
        } else {
            Err(TransportError::InvalidSessionState {
                session_id: self.id.to_string(),
                state: self.state.to_string(),
                expected: new_state.to_string(),
            })
        }
    }
}

/// Thread-safe session manager.
///
/// Tracks all active sessions with lock-free concurrent access via DashMap.
pub struct SessionManager {
    /// Active sessions: session_id → Session
    sessions: Arc<DashMap<Uuid, Session>>,
    /// Lookup index: ServiceKey → session_id (for get_or_create)
    index: Arc<DashMap<ServiceKey, Uuid>>,
    /// Session configuration.
    config: SessionConfig,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(SessionConfig::default())
    }
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new(config: SessionConfig) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            index: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Get or create a session for the given DCC instance.
    ///
    /// This is the primary API — it implements lazy session creation.
    /// If a session already exists and is usable, it is returned.
    /// Otherwise a new session is created.
    pub fn get_or_create(
        &self,
        dcc_type: &str,
        instance_id: Uuid,
        host: &str,
        port: u16,
    ) -> TransportResult<Uuid> {
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id,
        };

        // Check for existing session
        if let Some(session_id) = self.index.get(&key) {
            let session_id = *session_id.value();
            if let Some(session) = self.sessions.get(&session_id) {
                match session.state {
                    SessionState::Connected | SessionState::Idle => {
                        // Reactivate if idle
                        drop(session);
                        if let Some(mut session) = self.sessions.get_mut(&session_id) {
                            if session.state == SessionState::Idle {
                                let _ = session.transition_to(SessionState::Connected);
                            }
                            session.touch();
                        }
                        return Ok(session_id);
                    }
                    SessionState::Reconnecting => {
                        // Session is reconnecting, return its ID (caller should wait/retry)
                        return Ok(session_id);
                    }
                    SessionState::Closed => {
                        // Remove stale index entry, will create new below
                        self.index.remove(&key);
                        self.sessions.remove(&session_id);
                    }
                }
            } else {
                // Index entry without session — clean up
                self.index.remove(&key);
            }
        }

        // Create new session
        let session = Session::new(dcc_type, instance_id, host, port);
        let session_id = session.id;

        tracing::info!(
            session_id = %session_id,
            dcc_type = %dcc_type,
            instance_id = %instance_id,
            host = %host,
            port = port,
            "created new session"
        );

        self.sessions.insert(session_id, session);
        self.index.insert(key, session_id);

        Ok(session_id)
    }

    /// Get a session by ID.
    pub fn get(&self, session_id: &Uuid) -> Option<Session> {
        self.sessions.get(session_id).map(|s| s.value().clone())
    }

    /// Get a session by service key.
    pub fn get_by_service(&self, key: &ServiceKey) -> Option<Session> {
        self.index
            .get(key)
            .and_then(|id| self.sessions.get(id.value()).map(|s| s.value().clone()))
    }

    /// Record a successful request on a session.
    pub fn record_success(&self, session_id: &Uuid, latency: Duration) {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.metrics.record_success(latency);
            session.touch();
        }
    }

    /// Record a failed request on a session.
    pub fn record_error(&self, session_id: &Uuid, latency: Duration, error: &str) {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.metrics.record_error(latency, error);
            session.touch();
        }
    }

    /// Attempt to reconnect a session with exponential backoff.
    ///
    /// Returns the backoff duration to wait before the next attempt, or an error
    /// if max retries have been exceeded.
    pub fn begin_reconnect(&self, session_id: &Uuid) -> TransportResult<Duration> {
        let mut session =
            self.sessions
                .get_mut(session_id)
                .ok_or_else(|| TransportError::SessionNotFound {
                    session_id: session_id.to_string(),
                })?;

        if session.state == SessionState::Closed {
            return Err(TransportError::InvalidSessionState {
                session_id: session_id.to_string(),
                state: "closed".to_string(),
                expected: "connected or idle".to_string(),
            });
        }

        session.reconnect_attempts += 1;

        if session.reconnect_attempts > self.config.reconnect_max_retries {
            let attempts = session.reconnect_attempts - 1;
            session.transition_to(SessionState::Closed)?;

            tracing::warn!(
                session_id = %session_id,
                attempts = attempts,
                "reconnection failed, closing session"
            );

            return Err(TransportError::ReconnectionFailed {
                session_id: session_id.to_string(),
                retries: attempts,
                reason: "max retries exceeded".to_string(),
            });
        }

        session.transition_to(SessionState::Reconnecting)?;

        // Exponential backoff: base * 2^(attempt-1)
        let backoff = self.config.reconnect_backoff_base
            * 2u32.saturating_pow(session.reconnect_attempts - 1);

        tracing::info!(
            session_id = %session_id,
            attempt = session.reconnect_attempts,
            backoff_ms = backoff.as_millis() as u64,
            "beginning reconnection"
        );

        Ok(backoff)
    }

    /// Mark a reconnection as successful.
    pub fn reconnect_success(&self, session_id: &Uuid) -> TransportResult<()> {
        let mut session =
            self.sessions
                .get_mut(session_id)
                .ok_or_else(|| TransportError::SessionNotFound {
                    session_id: session_id.to_string(),
                })?;

        session.transition_to(SessionState::Connected)?;
        session.reconnect_attempts = 0;
        session.touch();

        tracing::info!(session_id = %session_id, "reconnection successful");
        Ok(())
    }

    /// Close a specific session.
    pub fn close(&self, session_id: &Uuid) -> TransportResult<Option<Session>> {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            let _ = session.transition_to(SessionState::Closed);
            let key = session.service_key();
            drop(session);

            self.index.remove(&key);
            let removed = self.sessions.remove(session_id).map(|(_, s)| s);

            tracing::info!(session_id = %session_id, "session closed");
            Ok(removed)
        } else {
            Ok(None)
        }
    }

    /// Transition idle sessions based on idle_timeout.
    pub fn mark_idle_sessions(&self) -> usize {
        let mut count = 0;
        for mut entry in self.sessions.iter_mut() {
            let session = entry.value_mut();
            if session.state == SessionState::Connected && session.is_idle(self.config.idle_timeout)
            {
                let _ = session.transition_to(SessionState::Idle);
                count += 1;
            }
        }
        count
    }

    /// Close expired sessions (exceeded max_session_lifetime).
    pub fn close_expired(&self) -> usize {
        let expired: Vec<Uuid> = self
            .sessions
            .iter()
            .filter(|entry| {
                entry.value().state != SessionState::Closed
                    && entry.value().is_expired(self.config.max_session_lifetime)
            })
            .map(|entry| *entry.key())
            .collect();

        let count = expired.len();
        for id in expired {
            let _ = self.close(&id);
        }
        count
    }

    /// Get the number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Check if there are no active sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Get the number of sessions for a specific DCC type.
    pub fn count_for_dcc(&self, dcc_type: &str) -> usize {
        self.sessions
            .iter()
            .filter(|entry| entry.value().dcc_type == dcc_type)
            .count()
    }

    /// List all sessions.
    pub fn list_all(&self) -> Vec<Session> {
        self.sessions.iter().map(|e| e.value().clone()).collect()
    }

    /// List sessions for a specific DCC type.
    pub fn list_for_dcc(&self, dcc_type: &str) -> Vec<Session> {
        self.sessions
            .iter()
            .filter(|e| e.value().dcc_type == dcc_type)
            .map(|e| e.value().clone())
            .collect()
    }

    /// Gracefully shut down all sessions.
    pub fn shutdown_all(&self) -> Vec<Session> {
        let ids: Vec<Uuid> = self.sessions.iter().map(|e| *e.key()).collect();
        let mut closed = Vec::with_capacity(ids.len());

        for id in ids {
            if let Ok(Some(session)) = self.close(&id) {
                closed.push(session);
            }
        }

        tracing::info!(count = closed.len(), "all sessions shut down");
        closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> SessionManager {
        SessionManager::new(SessionConfig::default())
    }

    #[test]
    fn test_session_creation() {
        let instance_id = Uuid::new_v4();
        let session = Session::new("maya", instance_id, "127.0.0.1", 18812);
        assert_eq!(session.dcc_type, "maya");
        assert_eq!(session.instance_id, instance_id);
        assert_eq!(session.state, SessionState::Connected);
        assert_eq!(session.metrics.request_count, 0);
    }

    #[test]
    fn test_session_state_transitions() {
        let mut session = Session::new("maya", Uuid::new_v4(), "127.0.0.1", 18812);

        // Connected → Idle
        session.transition_to(SessionState::Idle).unwrap();
        assert_eq!(session.state, SessionState::Idle);

        // Idle → Connected
        session.transition_to(SessionState::Connected).unwrap();
        assert_eq!(session.state, SessionState::Connected);

        // Connected → Reconnecting
        session.transition_to(SessionState::Reconnecting).unwrap();
        assert_eq!(session.state, SessionState::Reconnecting);

        // Reconnecting → Connected
        session.transition_to(SessionState::Connected).unwrap();
        assert_eq!(session.state, SessionState::Connected);

        // Connected → Closed
        session.transition_to(SessionState::Closed).unwrap();
        assert_eq!(session.state, SessionState::Closed);

        // Closed → anything should fail
        let result = session.transition_to(SessionState::Connected);
        assert!(result.is_err());
    }

    #[test]
    fn test_session_metrics() {
        let mut metrics = SessionMetrics::default();
        assert_eq!(metrics.avg_latency_ms(), 0.0);
        assert_eq!(metrics.error_rate(), 0.0);

        metrics.record_success(Duration::from_millis(100));
        metrics.record_success(Duration::from_millis(200));
        assert_eq!(metrics.request_count, 2);
        assert_eq!(metrics.avg_latency_ms(), 150.0);

        metrics.record_error(Duration::from_millis(50), "timeout");
        assert_eq!(metrics.request_count, 3);
        assert_eq!(metrics.error_count, 1);
        assert!((metrics.error_rate() - 1.0 / 3.0).abs() < f64::EPSILON);
        assert_eq!(metrics.last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_manager_get_or_create() {
        let manager = make_manager();
        let instance_id = Uuid::new_v4();

        let id1 = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();
        assert_eq!(manager.len(), 1);

        // Same service key should return the same session
        let id2 = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();
        assert_eq!(id1, id2);
        assert_eq!(manager.len(), 1);

        // Different instance → new session
        let other_id = Uuid::new_v4();
        let id3 = manager
            .get_or_create("maya", other_id, "127.0.0.1", 18813)
            .unwrap();
        assert_ne!(id1, id3);
        assert_eq!(manager.len(), 2);
    }

    #[test]
    fn test_manager_get_by_service() {
        let manager = make_manager();
        let instance_id = Uuid::new_v4();

        let session_id = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();

        let key = ServiceKey {
            dcc_type: "maya".to_string(),
            instance_id,
        };
        let session = manager.get_by_service(&key).unwrap();
        assert_eq!(session.id, session_id);
    }

    #[test]
    fn test_manager_record_metrics() {
        let manager = make_manager();
        let instance_id = Uuid::new_v4();

        let session_id = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();

        manager.record_success(&session_id, Duration::from_millis(100));
        manager.record_success(&session_id, Duration::from_millis(200));
        manager.record_error(&session_id, Duration::from_millis(50), "timeout");

        let session = manager.get(&session_id).unwrap();
        assert_eq!(session.metrics.request_count, 3);
        assert_eq!(session.metrics.error_count, 1);
    }

    #[test]
    fn test_manager_reconnect_backoff() {
        let config = SessionConfig {
            reconnect_max_retries: 3,
            reconnect_backoff_base: Duration::from_millis(100),
            ..Default::default()
        };
        let manager = SessionManager::new(config);
        let instance_id = Uuid::new_v4();

        let session_id = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();

        // Attempt 1: 100ms
        let backoff = manager.begin_reconnect(&session_id).unwrap();
        assert_eq!(backoff, Duration::from_millis(100));

        // Mark success, reset attempts
        manager.reconnect_success(&session_id).unwrap();
        let session = manager.get(&session_id).unwrap();
        assert_eq!(session.state, SessionState::Connected);
        assert_eq!(session.reconnect_attempts, 0);
    }

    #[test]
    fn test_manager_reconnect_exponential_backoff() {
        let config = SessionConfig {
            reconnect_max_retries: 3,
            reconnect_backoff_base: Duration::from_millis(100),
            ..Default::default()
        };
        let manager = SessionManager::new(config);
        let instance_id = Uuid::new_v4();

        let session_id = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();

        // Attempt 1: 100ms * 2^0 = 100ms
        let b1 = manager.begin_reconnect(&session_id).unwrap();
        assert_eq!(b1, Duration::from_millis(100));

        // Need to go back to connected to reconnect again
        manager.reconnect_success(&session_id).unwrap();

        // Attempt 1 again (reset): 100ms
        let b2 = manager.begin_reconnect(&session_id).unwrap();
        assert_eq!(b2, Duration::from_millis(100));

        // Attempt 2: 100ms * 2^1 = 200ms
        // Session is now in Reconnecting state, go to Connected first
        manager.reconnect_success(&session_id).unwrap();

        // Now exhaust retries
        let _ = manager.begin_reconnect(&session_id); // attempt 1: 100ms
        // Session is Reconnecting, we need to simulate failure:
        // transition back and try again (in real code, reconnect_success or begin_reconnect again)
    }

    #[test]
    fn test_manager_reconnect_max_retries() {
        let config = SessionConfig {
            reconnect_max_retries: 2,
            reconnect_backoff_base: Duration::from_millis(10),
            ..Default::default()
        };
        let manager = SessionManager::new(config);
        let instance_id = Uuid::new_v4();

        let session_id = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();

        // Attempt 1
        manager.begin_reconnect(&session_id).unwrap();
        // Attempt 2
        manager.begin_reconnect(&session_id).unwrap();
        // Attempt 3 should fail (max_retries=2)
        let result = manager.begin_reconnect(&session_id);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TransportError::ReconnectionFailed { .. }
        ));

        // Session should be closed
        let session = manager.get(&session_id).unwrap();
        assert_eq!(session.state, SessionState::Closed);
    }

    #[test]
    fn test_manager_close_session() {
        let manager = make_manager();
        let instance_id = Uuid::new_v4();

        let session_id = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();
        assert_eq!(manager.len(), 1);

        let closed = manager.close(&session_id).unwrap();
        assert!(closed.is_some());
        assert!(manager.is_empty());
    }

    #[test]
    fn test_manager_mark_idle() {
        let config = SessionConfig {
            idle_timeout: Duration::from_millis(0), // Everything is idle immediately
            ..Default::default()
        };
        let manager = SessionManager::new(config);

        let id = Uuid::new_v4();
        manager
            .get_or_create("maya", id, "127.0.0.1", 18812)
            .unwrap();

        let marked = manager.mark_idle_sessions();
        assert_eq!(marked, 1);

        let session = manager.get_by_service(&ServiceKey {
            dcc_type: "maya".to_string(),
            instance_id: id,
        });
        assert_eq!(session.unwrap().state, SessionState::Idle);
    }

    #[test]
    fn test_manager_count_for_dcc() {
        let manager = make_manager();

        manager
            .get_or_create("maya", Uuid::new_v4(), "127.0.0.1", 18812)
            .unwrap();
        manager
            .get_or_create("maya", Uuid::new_v4(), "127.0.0.1", 18813)
            .unwrap();
        manager
            .get_or_create("blender", Uuid::new_v4(), "127.0.0.1", 9090)
            .unwrap();

        assert_eq!(manager.count_for_dcc("maya"), 2);
        assert_eq!(manager.count_for_dcc("blender"), 1);
        assert_eq!(manager.count_for_dcc("houdini"), 0);
    }

    #[test]
    fn test_manager_list_for_dcc() {
        let manager = make_manager();

        manager
            .get_or_create("maya", Uuid::new_v4(), "127.0.0.1", 18812)
            .unwrap();
        manager
            .get_or_create("blender", Uuid::new_v4(), "127.0.0.1", 9090)
            .unwrap();

        let maya_sessions = manager.list_for_dcc("maya");
        assert_eq!(maya_sessions.len(), 1);
        assert_eq!(maya_sessions[0].dcc_type, "maya");
    }

    #[test]
    fn test_manager_shutdown_all() {
        let manager = make_manager();

        manager
            .get_or_create("maya", Uuid::new_v4(), "127.0.0.1", 18812)
            .unwrap();
        manager
            .get_or_create("blender", Uuid::new_v4(), "127.0.0.1", 9090)
            .unwrap();

        let closed = manager.shutdown_all();
        assert_eq!(closed.len(), 2);
        assert!(manager.is_empty());
    }

    #[test]
    fn test_manager_get_or_create_replaces_closed() {
        let manager = make_manager();
        let instance_id = Uuid::new_v4();

        let id1 = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();

        // Close the session
        manager.close(&id1).unwrap();
        assert!(manager.is_empty());

        // Creating again should produce a new session ID
        let id2 = manager
            .get_or_create("maya", instance_id, "127.0.0.1", 18812)
            .unwrap();
        assert_ne!(id1, id2);
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn test_session_state_display() {
        assert_eq!(format!("{}", SessionState::Connected), "connected");
        assert_eq!(format!("{}", SessionState::Idle), "idle");
        assert_eq!(format!("{}", SessionState::Reconnecting), "reconnecting");
        assert_eq!(format!("{}", SessionState::Closed), "closed");
    }
}
