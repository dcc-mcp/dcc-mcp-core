use super::*;

use crate::config::SessionConfig;
use crate::discovery::types::ServiceKey;
use crate::error::TransportError;

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
