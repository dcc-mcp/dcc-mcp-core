//! Connection-local protocol negotiation and logical session routing.

use std::collections::HashSet;

use dcc_mcp_ui_control::host_protocol::{
    UI_CONTROL_HOST_CAPABILITIES, UI_CONTROL_HOST_PROTOCOL_VERSION, UiControlHostErrorCode,
    UiControlHostHello, UiControlHostRequest, UiControlHostResponse,
};
use uuid::Uuid;

use super::{UiControlHost, error, valid_wire_label};

/// Per-connection handshake and logical-session ownership state.
#[derive(Debug)]
pub struct UiControlHostConnection {
    namespace: String,
    negotiated: bool,
    pub(super) owned_sessions: HashSet<String>,
    pub(super) owned_system_sessions: HashSet<String>,
}

impl Default for UiControlHostConnection {
    fn default() -> Self {
        Self {
            namespace: Uuid::new_v4().simple().to_string(),
            negotiated: false,
            owned_sessions: HashSet::new(),
            owned_system_sessions: HashSet::new(),
        }
    }
}

impl UiControlHostConnection {
    /// Handle one decoded request and apply it to the process-owned host state.
    #[must_use]
    pub fn handle(
        &mut self,
        host: &mut UiControlHost,
        request: UiControlHostRequest,
    ) -> UiControlHostResponse {
        if let UiControlHostRequest::Hello(hello) = request {
            return self.hello(hello);
        }
        if !self.negotiated {
            return error(
                UiControlHostErrorCode::HandshakeRequired,
                "hello must negotiate the exact protocol version first",
            );
        }
        let Some((logical_session_id, system, opening)) = request_session(&request)
            .map(|(session_id, system, opening)| (session_id.to_owned(), system, opening))
        else {
            return error(
                UiControlHostErrorCode::InvalidRequest,
                "UI Control request has no session identity",
            );
        };
        if !opening {
            let owned = if system {
                &self.owned_system_sessions
            } else {
                &self.owned_sessions
            };
            if !owned.contains(&logical_session_id) {
                return error(
                    UiControlHostErrorCode::SessionNotFound,
                    "UI Control session is not owned by this named-pipe connection",
                );
            }
        }
        let consumes_system_session =
            matches!(request, UiControlHostRequest::ExecuteSystemOperation { .. });
        let may_close_window_session =
            matches!(request, UiControlHostRequest::ExecuteAction { .. });
        let host_session_id = self.host_session_id(&logical_session_id);
        let response = host.handle(request_with_session_id(request, host_session_id));
        if consumes_system_session {
            self.owned_system_sessions.remove(&logical_session_id);
        }
        match &response {
            UiControlHostResponse::SessionOpened { .. } => {
                self.owned_sessions.insert(logical_session_id.clone());
            }
            UiControlHostResponse::SessionStopped {
                cleanup_pending: false,
                ..
            } => {
                self.owned_sessions.remove(&logical_session_id);
            }
            UiControlHostResponse::SystemSessionOpened { .. } => {
                self.owned_system_sessions
                    .insert(logical_session_id.clone());
            }
            UiControlHostResponse::SystemSessionStopped { .. } => {
                self.owned_system_sessions.remove(&logical_session_id);
            }
            _ => {}
        }
        if matches!(
            response,
            UiControlHostResponse::ActionCompleted {
                target_closed: true,
                ..
            }
        ) && may_close_window_session
        {
            self.owned_sessions.remove(&logical_session_id);
        }
        response_with_session_id(response, logical_session_id)
    }

    /// Stop every session minted for this pipe when the client disconnects.
    #[cfg(any(windows, test))]
    pub(super) fn disconnect(&mut self, host: &mut UiControlHost) {
        for session_id in self.owned_sessions.drain() {
            let _ = host.stop_session(format!("{}:{session_id}", self.namespace));
        }
        for session_id in self.owned_system_sessions.drain() {
            let _ = host.stop_system_session(format!("{}:{session_id}", self.namespace));
        }
        self.negotiated = false;
    }

    pub(super) fn host_session_id(&self, logical_session_id: &str) -> String {
        format!("{}:{logical_session_id}", self.namespace)
    }

    fn hello(&mut self, hello: UiControlHostHello) -> UiControlHostResponse {
        if hello.protocol_version != UI_CONTROL_HOST_PROTOCOL_VERSION {
            self.negotiated = false;
            return error(
                UiControlHostErrorCode::ProtocolMismatch,
                format!(
                    "UI Control host protocol mismatch: client={}, host={}",
                    hello.protocol_version, UI_CONTROL_HOST_PROTOCOL_VERSION
                ),
            );
        }
        if !valid_wire_label(&hello.client_name, 128) {
            self.negotiated = false;
            return error(
                UiControlHostErrorCode::InvalidRequest,
                "client_name must not be empty",
            );
        }
        self.negotiated = true;
        UiControlHostResponse::Hello {
            protocol_version: UI_CONTROL_HOST_PROTOCOL_VERSION,
            capabilities: UI_CONTROL_HOST_CAPABILITIES
                .iter()
                .map(|item| (*item).to_owned())
                .collect(),
        }
    }
}

fn request_with_session_id(
    request: UiControlHostRequest,
    namespaced_session_id: String,
) -> UiControlHostRequest {
    match request {
        UiControlHostRequest::Hello(_) => unreachable!("hello is handled before session routing"),
        UiControlHostRequest::OpenSession { grant, .. } => UiControlHostRequest::OpenSession {
            session_id: namespaced_session_id,
            grant,
        },
        UiControlHostRequest::OpenSystemSession {
            system_grant_id, ..
        } => UiControlHostRequest::OpenSystemSession {
            session_id: namespaced_session_id,
            system_grant_id,
        },
        UiControlHostRequest::GetWindowState {
            task_grant_id,
            window_capability,
            ..
        } => UiControlHostRequest::GetWindowState {
            session_id: namespaced_session_id,
            task_grant_id,
            window_capability,
        },
        UiControlHostRequest::ChangeWindowState {
            task_grant_id,
            window_capability,
            operation,
            ..
        } => UiControlHostRequest::ChangeWindowState {
            session_id: namespaced_session_id,
            task_grant_id,
            window_capability,
            operation,
        },
        UiControlHostRequest::Snapshot {
            task_grant_id,
            window_capability,
            max_depth,
            max_nodes,
            ..
        } => UiControlHostRequest::Snapshot {
            session_id: namespaced_session_id,
            task_grant_id,
            window_capability,
            max_depth,
            max_nodes,
        },
        UiControlHostRequest::AccessibilitySnapshot {
            task_grant_id,
            window_capability,
            max_depth,
            max_nodes,
            ..
        } => UiControlHostRequest::AccessibilitySnapshot {
            session_id: namespaced_session_id,
            task_grant_id,
            window_capability,
            max_depth,
            max_nodes,
        },
        UiControlHostRequest::RecordClip {
            task_grant_id,
            window_capability,
            duration_ms,
            frames_per_second,
            format,
            jpeg_quality,
            ..
        } => UiControlHostRequest::RecordClip {
            session_id: namespaced_session_id,
            task_grant_id,
            window_capability,
            duration_ms,
            frames_per_second,
            format,
            jpeg_quality,
        },
        UiControlHostRequest::ExecuteAction {
            task_grant_id,
            window_capability,
            observation_id,
            accessibility_state_id,
            action,
            ..
        } => UiControlHostRequest::ExecuteAction {
            session_id: namespaced_session_id,
            task_grant_id,
            window_capability,
            observation_id,
            accessibility_state_id,
            action,
        },
        UiControlHostRequest::ExecuteSystemOperation {
            system_grant_id,
            system_capability,
            operation_id,
            ..
        } => UiControlHostRequest::ExecuteSystemOperation {
            session_id: namespaced_session_id,
            system_grant_id,
            system_capability,
            operation_id,
        },
        UiControlHostRequest::ResumeSession {
            task_grant_id,
            window_capability,
            ..
        } => UiControlHostRequest::ResumeSession {
            session_id: namespaced_session_id,
            task_grant_id,
            window_capability,
        },
        UiControlHostRequest::StopSession { .. } => UiControlHostRequest::StopSession {
            session_id: namespaced_session_id,
        },
        UiControlHostRequest::StopSystemSession { .. } => UiControlHostRequest::StopSystemSession {
            session_id: namespaced_session_id,
        },
    }
}

fn response_with_session_id(
    response: UiControlHostResponse,
    logical_session_id: String,
) -> UiControlHostResponse {
    match response {
        UiControlHostResponse::SessionOpened {
            window_capability,
            target,
            ..
        } => UiControlHostResponse::SessionOpened {
            session_id: logical_session_id,
            window_capability,
            target,
        },
        UiControlHostResponse::SessionStopped {
            cleanup_pending, ..
        } => UiControlHostResponse::SessionStopped {
            session_id: logical_session_id,
            cleanup_pending,
        },
        UiControlHostResponse::SystemSessionOpened {
            system_capability,
            dcc_type,
            ..
        } => UiControlHostResponse::SystemSessionOpened {
            session_id: logical_session_id,
            system_capability,
            dcc_type,
        },
        UiControlHostResponse::WindowState { state, .. } => UiControlHostResponse::WindowState {
            session_id: logical_session_id,
            state,
        },
        UiControlHostResponse::WindowStateChanged {
            operation, state, ..
        } => UiControlHostResponse::WindowStateChanged {
            session_id: logical_session_id,
            operation,
            state,
        },
        UiControlHostResponse::SessionResumed { .. } => UiControlHostResponse::SessionResumed {
            session_id: logical_session_id,
        },
        UiControlHostResponse::SystemSessionStopped { .. } => {
            UiControlHostResponse::SystemSessionStopped {
                session_id: logical_session_id,
            }
        }
        other => other,
    }
}

fn request_session(request: &UiControlHostRequest) -> Option<(&str, bool, bool)> {
    match request {
        UiControlHostRequest::Hello(_) => None,
        UiControlHostRequest::OpenSession { session_id, .. } => Some((session_id, false, true)),
        UiControlHostRequest::OpenSystemSession { session_id, .. } => {
            Some((session_id, true, true))
        }
        UiControlHostRequest::GetWindowState { session_id, .. }
        | UiControlHostRequest::ChangeWindowState { session_id, .. }
        | UiControlHostRequest::Snapshot { session_id, .. }
        | UiControlHostRequest::AccessibilitySnapshot { session_id, .. }
        | UiControlHostRequest::RecordClip { session_id, .. }
        | UiControlHostRequest::ExecuteAction { session_id, .. }
        | UiControlHostRequest::ResumeSession { session_id, .. }
        | UiControlHostRequest::StopSession { session_id } => Some((session_id, false, false)),
        UiControlHostRequest::ExecuteSystemOperation { session_id, .. }
        | UiControlHostRequest::StopSystemSession { session_id } => Some((session_id, true, false)),
    }
}
