//! Isolated UI Control host state machine and executable entry point.

use std::collections::HashMap;

use dcc_mcp_ui_control::host_protocol::{
    UI_CONTROL_HOST_PROTOCOL_VERSION, UiControlAction, UiControlClipArtifact, UiControlClipFormat,
    UiControlHostErrorCode, UiControlHostHello, UiControlHostRequest, UiControlHostResponse,
    UiControlInputKind, UiControlPolicyTier, UiControlSharedImage, UiControlSystemGrant,
    UiControlSystemOperation, UiControlTarget, UiControlTaskGrant, UiControlWindowOperation,
    UiControlWindowState,
};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use uuid::Uuid;

mod audit;
mod connection;
mod policy;
#[cfg(any(windows, test))]
mod recording_artifact;
#[cfg(test)]
mod recording_artifact_tests;
#[cfg(windows)]
mod runtime_windows;
mod system_operations;
#[cfg(windows)]
mod windows;

use audit::{audit_event, audit_system_event};
pub use connection::UiControlHostConnection;
#[cfg(any(windows, test))]
use policy::{ActionControlFence, verify_expected_action_fence};
use policy::{
    action_fields_are_valid, allows_owned_standard_menu_popup, cached_action_fence,
    classify_action, stale_accessibility_state, verify_action_fence,
};
#[cfg(test)]
use policy::{classify_control, classify_control_text};
#[cfg(windows)]
use system_operations::load_system_grants;
use system_operations::{
    invalid_system_operation, run_system_operation, validate_system_operation,
};

const MIN_CLIP_DURATION_MS: u64 = 1_000;
const MAX_CLIP_DURATION_MS: u64 = 180_000;
const MIN_CLIP_FRAMES_PER_SECOND: u32 = 1;
const MAX_CLIP_FRAMES_PER_SECOND: u32 = 60;
const MIN_CLIP_JPEG_QUALITY: u8 = 70;
const MAX_CLIP_JPEG_QUALITY: u8 = 100;
const MAX_HOST_SESSION_ID_BYTES: usize = 192;

#[derive(Debug)]
struct HostFailure {
    code: UiControlHostErrorCode,
    message: String,
}

impl HostFailure {
    fn new(code: UiControlHostErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

struct RuntimeSnapshot {
    observation_id: String,
    target: UiControlTarget,
    observation: Value,
    root: Value,
    focus_runtime_id: Option<String>,
    node_count: u32,
    image: UiControlSharedImage,
}

#[derive(Clone)]
struct RuntimeAccessibilityState {
    root: Value,
    focus_runtime_id: Option<String>,
    node_count: u32,
}

struct RuntimeActionResult {
    message: String,
    target_closed: bool,
    before_focus_runtime_id: Option<String>,
    after_focus_runtime_id: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeClipRequest {
    duration_ms: u64,
    frames_per_second: u32,
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    format: UiControlClipFormat,
    jpeg_quality: u8,
}

trait HostRuntimeSession: Send {
    fn target(&self) -> &UiControlTarget;
    fn user_interrupted(&self) -> bool;
    fn start_visible_notice(&mut self) -> Result<(), HostFailure>;
    fn window_state(&mut self) -> Result<UiControlWindowState, HostFailure>;
    fn change_window_state(
        &mut self,
        operation: UiControlWindowOperation,
    ) -> Result<UiControlWindowState, HostFailure>;
    fn snapshot(&mut self, max_depth: u32, max_nodes: u32) -> Result<RuntimeSnapshot, HostFailure>;
    fn record_clip(
        &mut self,
        request: RuntimeClipRequest,
    ) -> Result<UiControlClipArtifact, HostFailure>;
    fn accessibility_state(
        &mut self,
        max_depth: u32,
        max_nodes: u32,
        allow_owned_standard_menu_popup: bool,
    ) -> Result<RuntimeAccessibilityState, HostFailure>;
    fn execute(
        &mut self,
        observation_id: &str,
        action: &UiControlAction,
        fence: &ActionFenceExpectation,
    ) -> Result<RuntimeActionResult, HostFailure>;
    fn resume_after_approval(&mut self) -> Result<(), HostFailure>;
    fn stop(&mut self) -> bool;
}

trait HostRuntime: Send + Sync {
    fn open(
        &self,
        grant: &UiControlTaskGrant,
        session_id: &str,
    ) -> Result<Box<dyn HostRuntimeSession>, HostFailure>;
}

#[derive(Debug, Clone, Copy)]
enum ConfirmationKind<'a> {
    ConsequentialAction(UiControlPolicyTier),
    SystemOperation(&'a UiControlSystemOperation),
    ResumeAfterStop,
}

trait ConfirmationSurface: Send + Sync {
    fn confirm(
        &self,
        kind: ConfirmationKind<'_>,
        target: Option<&UiControlTarget>,
        action: Option<&UiControlAction>,
    ) -> Result<bool, HostFailure>;
}

struct HostSession {
    grant: UiControlTaskGrant,
    window_capability: String,
    observation_id: Option<String>,
    observation: Option<Value>,
    accessibility_state_id: Option<String>,
    accessibility_root: Option<Value>,
    focus_runtime_id: Option<String>,
    accessibility_max_depth: Option<u32>,
    accessibility_max_nodes: Option<u32>,
    runtime: Box<dyn HostRuntimeSession>,
}

struct ActionFenceExpectation {
    #[cfg(any(windows, test))]
    controls: Vec<ActionControlFence>,
    #[cfg(any(windows, test))]
    observation: Option<Value>,
    #[cfg(windows)]
    max_depth: u32,
    #[cfg(windows)]
    max_nodes: u32,
    #[cfg(any(windows, test))]
    policy_tier: UiControlPolicyTier,
}

struct SystemHostSession {
    grant: UiControlSystemGrant,
    system_capability: String,
}

/// Process-owned capability, policy, confirmation, and native execution authority.
pub struct UiControlHost {
    sessions: HashMap<String, HostSession>,
    system_sessions: HashMap<String, SystemHostSession>,
    system_grants: HashMap<String, UiControlSystemGrant>,
    runtime: Box<dyn HostRuntime>,
    confirmation: Box<dyn ConfirmationSurface>,
}

impl Default for UiControlHost {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            system_sessions: HashMap::new(),
            system_grants: HashMap::new(),
            runtime: default_runtime(),
            confirmation: default_confirmation_surface(),
        }
    }
}

impl UiControlHost {
    #[cfg(windows)]
    fn from_operator_config() -> Result<Self, String> {
        Ok(Self {
            system_grants: load_system_grants()?,
            ..Self::default()
        })
    }
}

impl UiControlHost {
    fn handle(&mut self, request: UiControlHostRequest) -> UiControlHostResponse {
        match request {
            UiControlHostRequest::Hello(_) => unreachable!("hello is handled by the connection"),
            UiControlHostRequest::OpenSession { session_id, grant } => {
                self.open_session(session_id, grant)
            }
            UiControlHostRequest::OpenSystemSession {
                session_id,
                system_grant_id,
            } => self.open_system_session(session_id, &system_grant_id),
            UiControlHostRequest::GetWindowState {
                session_id,
                task_grant_id,
                window_capability,
            } => self.get_window_state(&session_id, &task_grant_id, &window_capability),
            UiControlHostRequest::ChangeWindowState {
                session_id,
                task_grant_id,
                window_capability,
                operation,
            } => {
                self.change_window_state(&session_id, &task_grant_id, &window_capability, operation)
            }
            UiControlHostRequest::Snapshot {
                session_id,
                task_grant_id,
                window_capability,
                max_depth,
                max_nodes,
            } => self.snapshot(
                &session_id,
                &task_grant_id,
                &window_capability,
                max_depth,
                max_nodes,
            ),
            UiControlHostRequest::AccessibilitySnapshot {
                session_id,
                task_grant_id,
                window_capability,
                max_depth,
                max_nodes,
            } => self.accessibility_snapshot(
                &session_id,
                &task_grant_id,
                &window_capability,
                max_depth,
                max_nodes,
            ),
            UiControlHostRequest::RecordClip {
                session_id,
                task_grant_id,
                window_capability,
                duration_ms,
                frames_per_second,
                format,
                jpeg_quality,
            } => self.record_clip(
                &session_id,
                &task_grant_id,
                &window_capability,
                RuntimeClipRequest {
                    duration_ms,
                    frames_per_second,
                    format,
                    jpeg_quality,
                },
            ),
            UiControlHostRequest::ExecuteAction {
                session_id,
                task_grant_id,
                window_capability,
                observation_id,
                accessibility_state_id,
                action,
            } => self.execute_action(
                &session_id,
                &task_grant_id,
                &window_capability,
                &observation_id,
                &accessibility_state_id,
                *action,
            ),
            UiControlHostRequest::ExecuteSystemOperation {
                session_id,
                system_grant_id,
                system_capability,
                operation_id,
            } => self.execute_system_operation(
                &session_id,
                &system_grant_id,
                &system_capability,
                &operation_id,
            ),
            UiControlHostRequest::ResumeSession {
                session_id,
                task_grant_id,
                window_capability,
            } => self.resume_session(&session_id, &task_grant_id, &window_capability),
            UiControlHostRequest::StopSession { session_id } => self.stop_session(session_id),
            UiControlHostRequest::StopSystemSession { session_id } => {
                self.stop_system_session(session_id)
            }
        }
    }

    fn open_session(
        &mut self,
        session_id: String,
        grant: UiControlTaskGrant,
    ) -> UiControlHostResponse {
        if !valid_wire_label(&session_id, MAX_HOST_SESSION_ID_BYTES)
            || !valid_wire_label(&grant.task_grant_id, 256)
            || !valid_wire_label(&grant.dcc_type, 64)
            || (grant.process_id.is_none() && grant.window_handle.is_none())
            || grant.process_id == Some(0)
            || grant.window_handle == Some(0)
        {
            return error(
                UiControlHostErrorCode::InvalidRequest,
                "session, grant, DCC, and exact PID or HWND scope must be explicit",
            );
        }
        if self.sessions.contains_key(&session_id) || self.system_sessions.contains_key(&session_id)
        {
            return error(
                UiControlHostErrorCode::SessionAlreadyExists,
                "stop the existing session before replacing its task grant",
            );
        }

        let mut runtime = match self.runtime.open(&grant, &session_id) {
            Ok(runtime) => runtime,
            Err(failure) => return failure.into_response(),
        };
        if let Err(failure) = runtime.start_visible_notice() {
            runtime.stop();
            return failure.into_response();
        }
        let target = runtime.target().clone();

        let window_capability = new_capability("window");
        self.sessions.insert(
            session_id.clone(),
            HostSession {
                grant: grant.clone(),
                window_capability: window_capability.clone(),
                observation_id: None,
                observation: None,
                accessibility_state_id: None,
                accessibility_root: None,
                focus_runtime_id: None,
                accessibility_max_depth: None,
                accessibility_max_nodes: None,
                runtime,
            },
        );
        audit_event(
            &grant,
            "open_session",
            true,
            UiControlPolicyTier::TaskGrant,
            None,
        );
        UiControlHostResponse::SessionOpened {
            session_id,
            window_capability,
            target,
        }
    }

    fn open_system_session(
        &mut self,
        session_id: String,
        system_grant_id: &str,
    ) -> UiControlHostResponse {
        if !valid_wire_label(&session_id, MAX_HOST_SESSION_ID_BYTES)
            || !valid_wire_label(system_grant_id, 256)
        {
            return error(
                UiControlHostErrorCode::InvalidRequest,
                "system session and operator grant ids must be explicit",
            );
        }
        if self.sessions.contains_key(&session_id) || self.system_sessions.contains_key(&session_id)
        {
            return error(
                UiControlHostErrorCode::SessionAlreadyExists,
                "stop the existing session before opening a system grant",
            );
        }
        let Some(grant) = self.system_grants.get(system_grant_id).cloned() else {
            return error(
                UiControlHostErrorCode::SystemOperationNotGranted,
                "the operator-owned system grant is unavailable",
            );
        };
        let system_capability = new_capability("system");
        self.system_sessions.insert(
            session_id.clone(),
            SystemHostSession {
                grant: grant.clone(),
                system_capability: system_capability.clone(),
            },
        );
        audit_system_event(
            &grant,
            "open_system_session",
            true,
            UiControlPolicyTier::TaskGrant,
            None,
        );
        UiControlHostResponse::SystemSessionOpened {
            session_id,
            system_capability,
            dcc_type: grant.dcc_type,
        }
    }

    fn get_window_state(
        &mut self,
        session_id: &str,
        task_grant_id: &str,
        window_capability: &str,
    ) -> UiControlHostResponse {
        let session = match Self::authorized_session_mut(
            &mut self.sessions,
            session_id,
            task_grant_id,
            window_capability,
        ) {
            Ok(session) => session,
            Err(failure) => return failure.into_response(),
        };
        match session.runtime.window_state() {
            Ok(state) => {
                audit_event(
                    &session.grant,
                    "get_window_state",
                    true,
                    UiControlPolicyTier::TaskGrant,
                    None,
                );
                UiControlHostResponse::WindowState {
                    session_id: session_id.to_owned(),
                    state,
                }
            }
            Err(failure) => {
                audit_event(
                    &session.grant,
                    "get_window_state",
                    false,
                    UiControlPolicyTier::TaskGrant,
                    Some(failure.code),
                );
                failure.into_response()
            }
        }
    }

    fn change_window_state(
        &mut self,
        session_id: &str,
        task_grant_id: &str,
        window_capability: &str,
        operation: UiControlWindowOperation,
    ) -> UiControlHostResponse {
        let session = match Self::authorized_session_mut(
            &mut self.sessions,
            session_id,
            task_grant_id,
            window_capability,
        ) {
            Ok(session) => session,
            Err(failure) => return failure.into_response(),
        };
        session.observation_id = None;
        session.observation = None;
        session.accessibility_state_id = None;
        session.accessibility_root = None;
        session.focus_runtime_id = None;
        session.accessibility_max_depth = None;
        session.accessibility_max_nodes = None;
        let action = match operation {
            UiControlWindowOperation::Restore => "restore_window",
            UiControlWindowOperation::Show => "show_window",
            UiControlWindowOperation::Activate => "activate_window",
        };
        match session.runtime.change_window_state(operation) {
            Ok(state) => {
                audit_event(
                    &session.grant,
                    action,
                    true,
                    UiControlPolicyTier::TaskGrant,
                    None,
                );
                UiControlHostResponse::WindowStateChanged {
                    session_id: session_id.to_owned(),
                    operation,
                    state,
                }
            }
            Err(failure) => {
                audit_event(
                    &session.grant,
                    action,
                    false,
                    UiControlPolicyTier::TaskGrant,
                    Some(failure.code),
                );
                failure.into_response()
            }
        }
    }

    fn snapshot(
        &mut self,
        session_id: &str,
        task_grant_id: &str,
        window_capability: &str,
        max_depth: u32,
        max_nodes: u32,
    ) -> UiControlHostResponse {
        if !(1..=12).contains(&max_depth) || !(1..=2_000).contains(&max_nodes) {
            return error(
                UiControlHostErrorCode::InvalidRequest,
                "max_depth must be 1..=12 and max_nodes must be 1..=2000",
            );
        }
        let session = match Self::authorized_session_mut(
            &mut self.sessions,
            session_id,
            task_grant_id,
            window_capability,
        ) {
            Ok(session) => session,
            Err(failure) => return failure.into_response(),
        };
        let snapshot = match session.runtime.snapshot(max_depth, max_nodes) {
            Ok(snapshot) => snapshot,
            Err(failure) => return failure.into_response(),
        };
        let accessibility_state_id = new_capability("accessibility");
        session.observation_id = Some(snapshot.observation_id.clone());
        session.observation = Some(snapshot.observation.clone());
        session.accessibility_state_id = Some(accessibility_state_id.clone());
        session.accessibility_root = Some(snapshot.root.clone());
        session.focus_runtime_id = snapshot.focus_runtime_id.clone();
        session.accessibility_max_depth = Some(max_depth);
        session.accessibility_max_nodes = Some(max_nodes);
        UiControlHostResponse::Snapshot {
            observation_id: snapshot.observation_id,
            accessibility_state_id,
            target: snapshot.target,
            observation: snapshot.observation,
            root: snapshot.root,
            focus_runtime_id: snapshot.focus_runtime_id,
            node_count: snapshot.node_count,
            image: Box::new(snapshot.image),
        }
    }

    fn accessibility_snapshot(
        &mut self,
        session_id: &str,
        task_grant_id: &str,
        window_capability: &str,
        max_depth: u32,
        max_nodes: u32,
    ) -> UiControlHostResponse {
        if !(1..=12).contains(&max_depth) || !(1..=2_000).contains(&max_nodes) {
            return error(
                UiControlHostErrorCode::InvalidRequest,
                "max_depth must be 1..=12 and max_nodes must be 1..=2000",
            );
        }
        let session = match Self::authorized_session_mut(
            &mut self.sessions,
            session_id,
            task_grant_id,
            window_capability,
        ) {
            Ok(session) => session,
            Err(failure) => return failure.into_response(),
        };
        let state = match session
            .runtime
            .accessibility_state(max_depth, max_nodes, true)
        {
            Ok(state) => state,
            Err(failure) => return failure.into_response(),
        };
        let accessibility_state_id = new_capability("accessibility");
        session.accessibility_state_id = Some(accessibility_state_id.clone());
        session.accessibility_root = Some(state.root.clone());
        session.focus_runtime_id = state.focus_runtime_id.clone();
        session.accessibility_max_depth = Some(max_depth);
        session.accessibility_max_nodes = Some(max_nodes);
        UiControlHostResponse::AccessibilitySnapshot {
            accessibility_state_id,
            target: session.runtime.target().clone(),
            root: state.root,
            focus_runtime_id: state.focus_runtime_id,
            node_count: state.node_count,
        }
    }

    fn record_clip(
        &mut self,
        session_id: &str,
        task_grant_id: &str,
        window_capability: &str,
        request: RuntimeClipRequest,
    ) -> UiControlHostResponse {
        if !(MIN_CLIP_DURATION_MS..=MAX_CLIP_DURATION_MS).contains(&request.duration_ms)
            || !(MIN_CLIP_FRAMES_PER_SECOND..=MAX_CLIP_FRAMES_PER_SECOND)
                .contains(&request.frames_per_second)
            || !(MIN_CLIP_JPEG_QUALITY..=MAX_CLIP_JPEG_QUALITY).contains(&request.jpeg_quality)
        {
            return error(
                UiControlHostErrorCode::InvalidRequest,
                "duration_ms must be 1000..=180000, frames_per_second must be 1..=60, and jpeg_quality must be 70..=100",
            );
        }
        let session = match Self::authorized_session_mut(
            &mut self.sessions,
            session_id,
            task_grant_id,
            window_capability,
        ) {
            Ok(session) => session,
            Err(failure) => return failure.into_response(),
        };
        consume_observation(session);
        match session.runtime.record_clip(request) {
            Ok(artifact) => {
                audit_event(
                    &session.grant,
                    "record_clip",
                    true,
                    UiControlPolicyTier::TaskGrant,
                    None,
                );
                UiControlHostResponse::ClipRecorded {
                    target: session.runtime.target().clone(),
                    artifact,
                }
            }
            Err(failure) => {
                audit_event(
                    &session.grant,
                    "record_clip",
                    false,
                    UiControlPolicyTier::TaskGrant,
                    Some(failure.code),
                );
                failure.into_response()
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_action(
        &mut self,
        session_id: &str,
        task_grant_id: &str,
        window_capability: &str,
        observation_id: &str,
        accessibility_state_id: &str,
        action: UiControlAction,
    ) -> UiControlHostResponse {
        let confirmation = &self.confirmation;
        let session = match Self::authorized_session_mut(
            &mut self.sessions,
            session_id,
            task_grant_id,
            window_capability,
        ) {
            Ok(session) => session,
            Err(failure) => return failure.into_response(),
        };
        if session.observation_id.as_deref() != Some(observation_id)
            || session.accessibility_state_id.as_deref() != Some(accessibility_state_id)
        {
            return action_response(
                false,
                UiControlPolicyTier::TaskGrant,
                "the action does not reference the latest host observation and accessibility state",
                Some(UiControlHostErrorCode::StaleObservation),
                None,
                None,
            );
        }
        if let Err(failure) = validate_action_descriptor(&action) {
            audit_event(
                &session.grant,
                "invalid_action",
                false,
                UiControlPolicyTier::TaskGrant,
                Some(failure.code),
            );
            return failure.into_response();
        }
        if action.input_kind == UiControlInputKind::RawInput && !session.grant.allow_raw_input {
            return action_response(
                false,
                UiControlPolicyTier::TaskGrant,
                "raw pointer and keyboard input is outside this task grant",
                Some(UiControlHostErrorCode::RawInputNotGranted),
                None,
                None,
            );
        }

        let mut policy_tier = classify_action(
            &action,
            session.accessibility_root.as_ref(),
            session.observation.as_ref(),
        );
        if policy_tier == UiControlPolicyTier::HardDeny {
            audit_event(
                &session.grant,
                &action.action,
                false,
                policy_tier,
                Some(UiControlHostErrorCode::HardDenied),
            );
            return action_response(
                false,
                policy_tier,
                "this target or action is outside the non-bypassable UI Control boundary",
                Some(UiControlHostErrorCode::HardDenied),
                None,
                None,
            );
        }
        let (refreshed_tier, mut action_fence) = match refresh_action_policy(session, &action) {
            Ok(refreshed) => refreshed,
            Err(failure) => {
                return action_fence_failure(session, &action, policy_tier, failure);
            }
        };
        policy_tier = refreshed_tier;
        if policy_tier == UiControlPolicyTier::HardDeny {
            return hard_deny_action(session, &action);
        }
        if policy_tier >= UiControlPolicyTier::PreApproval {
            let confirmed_tier = policy_tier;
            match confirmation.confirm(
                ConfirmationKind::ConsequentialAction(policy_tier),
                Some(session.runtime.target()),
                Some(&action),
            ) {
                Ok(true) => {}
                Ok(false) => {
                    audit_event(
                        &session.grant,
                        &action.action,
                        false,
                        policy_tier,
                        Some(UiControlHostErrorCode::ApprovalRequired),
                    );
                    return action_response(
                        false,
                        policy_tier,
                        "the user did not approve this consequential UI Control action",
                        Some(UiControlHostErrorCode::ApprovalRequired),
                        None,
                        None,
                    );
                }
                Err(failure) => return failure.into_response(),
            }
            let (refreshed_tier, refreshed_fence) = match refresh_action_policy(session, &action) {
                Ok(refreshed) => refreshed,
                Err(failure) => {
                    return action_fence_failure(session, &action, policy_tier, failure);
                }
            };
            if refreshed_tier == UiControlPolicyTier::HardDeny {
                return hard_deny_action(session, &action);
            }
            if refreshed_tier != confirmed_tier {
                return action_fence_failure(
                    session,
                    &action,
                    confirmed_tier,
                    stale_accessibility_state(),
                );
            }
            policy_tier = refreshed_tier;
            action_fence = refreshed_fence;
        }

        // Every attempted mutation consumes both fences, including backend failures.
        consume_observation(session);
        match session
            .runtime
            .execute(observation_id, &action, &action_fence)
        {
            Ok(result) => {
                audit_event(&session.grant, &action.action, true, policy_tier, None);
                let target_closed = result.target_closed;
                if target_closed {
                    let _ = session.runtime.stop();
                }
                let response = completed_action_response(
                    true,
                    target_closed,
                    policy_tier,
                    result.message,
                    None,
                    result.before_focus_runtime_id,
                    result.after_focus_runtime_id,
                );
                if target_closed {
                    self.sessions.remove(session_id);
                }
                response
            }
            Err(failure) => {
                audit_event(
                    &session.grant,
                    &action.action,
                    false,
                    policy_tier,
                    Some(failure.code),
                );
                action_response(
                    false,
                    policy_tier,
                    failure.message,
                    Some(failure.code),
                    None,
                    None,
                )
            }
        }
    }

    fn resume_session(
        &mut self,
        session_id: &str,
        task_grant_id: &str,
        window_capability: &str,
    ) -> UiControlHostResponse {
        let confirmation = &self.confirmation;
        let session = match Self::authorized_session_mut(
            &mut self.sessions,
            session_id,
            task_grant_id,
            window_capability,
        ) {
            Ok(session) => session,
            Err(failure) => return failure.into_response(),
        };
        if session.runtime.user_interrupted() {
            match confirmation.confirm(
                ConfirmationKind::ResumeAfterStop,
                Some(session.runtime.target()),
                None,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    return error(
                        UiControlHostErrorCode::ApprovalRequired,
                        "the user did not approve resuming UI Control",
                    );
                }
                Err(failure) => return failure.into_response(),
            }
            if let Err(failure) = session.runtime.resume_after_approval() {
                return failure.into_response();
            }
        }
        session.observation_id = None;
        session.observation = None;
        session.accessibility_state_id = None;
        session.accessibility_root = None;
        session.focus_runtime_id = None;
        session.accessibility_max_depth = None;
        session.accessibility_max_nodes = None;
        UiControlHostResponse::SessionResumed {
            session_id: session_id.to_owned(),
        }
    }

    fn execute_system_operation(
        &mut self,
        session_id: &str,
        system_grant_id: &str,
        system_capability: &str,
        operation_id: &str,
    ) -> UiControlHostResponse {
        let confirmation = &self.confirmation;
        let Some(session) = self.system_sessions.remove(session_id) else {
            return error(
                UiControlHostErrorCode::SessionNotFound,
                "UI Control system session does not exist",
            );
        };
        if session.grant.system_grant_id != system_grant_id {
            return error(
                UiControlHostErrorCode::GrantMismatch,
                "operator system grant does not own this session",
            );
        }
        if session.system_capability != system_capability {
            return error(
                UiControlHostErrorCode::CapabilityMismatch,
                "system capability is invalid or stale",
            );
        }
        if !valid_wire_label(operation_id, 256) {
            audit_system_event(
                &session.grant,
                "invalid_system_operation_id",
                false,
                UiControlPolicyTier::ActionConfirmation,
                Some(UiControlHostErrorCode::InvalidRequest),
            );
            return invalid_system_operation().into_response();
        }
        let Some(operation) = session
            .grant
            .operations
            .iter()
            .find(|entry| entry.operation_id == operation_id)
            .map(|entry| entry.operation.clone())
        else {
            audit_system_event(
                &session.grant,
                "ungranted_system_operation",
                false,
                UiControlPolicyTier::ActionConfirmation,
                Some(UiControlHostErrorCode::SystemOperationNotGranted),
            );
            return error(
                UiControlHostErrorCode::SystemOperationNotGranted,
                "the named operation is outside the operator-owned system grant",
            );
        };
        if let Err(failure) = validate_system_operation(&operation) {
            audit_system_event(
                &session.grant,
                operation.audit_name(),
                false,
                UiControlPolicyTier::ActionConfirmation,
                Some(failure.code),
            );
            return failure.into_response();
        }
        match confirmation.confirm(ConfirmationKind::SystemOperation(&operation), None, None) {
            Ok(true) => {}
            Ok(false) => {
                audit_system_event(
                    &session.grant,
                    operation.audit_name(),
                    false,
                    UiControlPolicyTier::ActionConfirmation,
                    Some(UiControlHostErrorCode::ApprovalRequired),
                );
                return error(
                    UiControlHostErrorCode::ApprovalRequired,
                    "the user did not approve this system configuration operation",
                );
            }
            Err(failure) => return failure.into_response(),
        }
        match run_system_operation(&operation) {
            Ok(outcome) => {
                audit_system_event(
                    &session.grant,
                    operation.audit_name(),
                    true,
                    UiControlPolicyTier::ActionConfirmation,
                    None,
                );
                UiControlHostResponse::SystemOperationCompleted {
                    operation_type: operation.audit_name().to_owned(),
                    outcome,
                    policy_tier: UiControlPolicyTier::ActionConfirmation,
                    message: "the approved system configuration state is ensured".to_owned(),
                }
            }
            Err(failure) => {
                audit_system_event(
                    &session.grant,
                    operation.audit_name(),
                    false,
                    UiControlPolicyTier::ActionConfirmation,
                    Some(failure.code),
                );
                failure.into_response()
            }
        }
    }

    fn stop_session(&mut self, session_id: String) -> UiControlHostResponse {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return error(
                UiControlHostErrorCode::SessionNotFound,
                "UI Control session does not exist",
            );
        };
        let cleanup_pending = session.runtime.stop();
        audit_event(
            &session.grant,
            "stop_session",
            true,
            UiControlPolicyTier::TaskGrant,
            None,
        );
        if !cleanup_pending {
            self.sessions.remove(&session_id);
        }
        UiControlHostResponse::SessionStopped {
            session_id,
            cleanup_pending,
        }
    }

    fn stop_system_session(&mut self, session_id: String) -> UiControlHostResponse {
        let Some(session) = self.system_sessions.remove(&session_id) else {
            return error(
                UiControlHostErrorCode::SessionNotFound,
                "UI Control system session does not exist",
            );
        };
        audit_system_event(
            &session.grant,
            "stop_system_session",
            true,
            UiControlPolicyTier::TaskGrant,
            None,
        );
        UiControlHostResponse::SystemSessionStopped { session_id }
    }

    fn authorized_session_mut<'a>(
        sessions: &'a mut HashMap<String, HostSession>,
        session_id: &str,
        task_grant_id: &str,
        window_capability: &str,
    ) -> Result<&'a mut HostSession, HostFailure> {
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            HostFailure::new(
                UiControlHostErrorCode::SessionNotFound,
                "UI Control session does not exist",
            )
        })?;
        if session.grant.task_grant_id != task_grant_id {
            return Err(HostFailure::new(
                UiControlHostErrorCode::GrantMismatch,
                "task grant does not own this UI Control session",
            ));
        }
        if session.window_capability != window_capability {
            return Err(HostFailure::new(
                UiControlHostErrorCode::CapabilityMismatch,
                "window capability is invalid or stale",
            ));
        }
        Ok(session)
    }
}

fn refresh_action_policy(
    session: &mut HostSession,
    action: &UiControlAction,
) -> Result<(UiControlPolicyTier, ActionFenceExpectation), HostFailure> {
    let max_depth = session
        .accessibility_max_depth
        .ok_or_else(stale_accessibility_state)?;
    let max_nodes = session
        .accessibility_max_nodes
        .ok_or_else(stale_accessibility_state)?;
    let cached_root = session
        .accessibility_root
        .as_ref()
        .ok_or_else(stale_accessibility_state)?;
    let (cached_tier, _cached_controls) = cached_action_fence(
        action,
        cached_root,
        session.focus_runtime_id.as_deref(),
        session.observation.as_ref(),
    )?;
    if cached_tier == UiControlPolicyTier::TaskGrant {
        return Ok((
            cached_tier,
            ActionFenceExpectation {
                #[cfg(any(windows, test))]
                controls: _cached_controls,
                #[cfg(any(windows, test))]
                observation: session.observation.clone(),
                #[cfg(windows)]
                max_depth,
                #[cfg(windows)]
                max_nodes,
                #[cfg(any(windows, test))]
                policy_tier: cached_tier,
            },
        ));
    }
    let live = session.runtime.accessibility_state(
        max_depth,
        max_nodes,
        allows_owned_standard_menu_popup(action),
    )?;
    let (policy_tier, _action_controls) = verify_action_fence(
        action,
        cached_root,
        session.focus_runtime_id.as_deref(),
        session.observation.as_ref(),
        &live,
    )?;
    Ok((
        policy_tier,
        ActionFenceExpectation {
            #[cfg(any(windows, test))]
            controls: _action_controls,
            #[cfg(any(windows, test))]
            observation: session.observation.clone(),
            #[cfg(windows)]
            max_depth,
            #[cfg(windows)]
            max_nodes,
            #[cfg(any(windows, test))]
            policy_tier,
        },
    ))
}

fn consume_observation(session: &mut HostSession) {
    session.observation_id = None;
    session.observation = None;
    session.accessibility_state_id = None;
    session.accessibility_root = None;
    session.focus_runtime_id = None;
    session.accessibility_max_depth = None;
    session.accessibility_max_nodes = None;
}

fn action_fence_failure(
    session: &mut HostSession,
    action: &UiControlAction,
    policy_tier: UiControlPolicyTier,
    failure: HostFailure,
) -> UiControlHostResponse {
    consume_observation(session);
    audit_event(
        &session.grant,
        &action.action,
        false,
        policy_tier,
        Some(failure.code),
    );
    action_response(
        false,
        policy_tier,
        failure.message,
        Some(failure.code),
        None,
        None,
    )
}

fn hard_deny_action(session: &HostSession, action: &UiControlAction) -> UiControlHostResponse {
    audit_event(
        &session.grant,
        &action.action,
        false,
        UiControlPolicyTier::HardDeny,
        Some(UiControlHostErrorCode::HardDenied),
    );
    action_response(
        false,
        UiControlPolicyTier::HardDeny,
        "this target or action is outside the non-bypassable UI Control boundary",
        Some(UiControlHostErrorCode::HardDenied),
        None,
        None,
    )
}

fn valid_wire_label(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

fn validate_action_descriptor(action: &UiControlAction) -> Result<(), HostFailure> {
    const SEMANTIC_ACTIONS: &[&str] = &[
        "click",
        "set_text",
        "toggle",
        "set_checked",
        "select_option",
        "focus",
    ];
    const RAW_ACTIONS: &[&str] = &[
        "move",
        "click",
        "double_click",
        "scroll",
        "drag",
        "raw_coordinate_click",
        "type",
        "keypress",
        "game_navigation",
        "keyboard_shortcut",
    ];
    let allowed = match action.input_kind {
        UiControlInputKind::Semantic => {
            action
                .control_id
                .as_deref()
                .is_some_and(|id| !id.is_empty() && id.len() <= 512)
                && SEMANTIC_ACTIONS.contains(&action.action.as_str())
        }
        UiControlInputKind::RawInput => RAW_ACTIONS.contains(&action.action.as_str()),
    };
    let points_are_finite = action
        .x
        .into_iter()
        .chain(action.y)
        .chain(action.path.iter().flat_map(|point| [point.x, point.y]))
        .all(f64::is_finite);
    let key_tokens = action
        .keys
        .iter()
        .flat_map(|item| item.split('+'))
        .filter(|item| !item.trim().is_empty())
        .count();
    if !allowed
        || !action_fields_are_valid(action)
        || !points_are_finite
        || action.path.len() > 256
        || key_tokens > 16
        || action
            .text
            .as_deref()
            .is_some_and(|text| text.encode_utf16().count() > 4096)
        || action.duration_ms.is_some_and(|duration| duration > 60_000)
    {
        return Err(HostFailure::new(
            UiControlHostErrorCode::InvalidRequest,
            "the UI Control action descriptor is unsupported or outside safety limits",
        ));
    }
    Ok(())
}

impl HostFailure {
    fn into_response(self) -> UiControlHostResponse {
        error(self.code, self.message)
    }
}

fn new_capability(kind: &str) -> String {
    format!("{kind}:{}", Uuid::new_v4().simple())
}

fn error(code: UiControlHostErrorCode, message: impl Into<String>) -> UiControlHostResponse {
    UiControlHostResponse::Error {
        code,
        message: message.into(),
    }
}

fn action_response(
    success: bool,
    policy_tier: UiControlPolicyTier,
    message: impl Into<String>,
    error: Option<UiControlHostErrorCode>,
    before_focus_runtime_id: Option<String>,
    after_focus_runtime_id: Option<String>,
) -> UiControlHostResponse {
    completed_action_response(
        success,
        false,
        policy_tier,
        message,
        error,
        before_focus_runtime_id,
        after_focus_runtime_id,
    )
}

#[allow(clippy::too_many_arguments)]
fn completed_action_response(
    success: bool,
    target_closed: bool,
    policy_tier: UiControlPolicyTier,
    message: impl Into<String>,
    error: Option<UiControlHostErrorCode>,
    before_focus_runtime_id: Option<String>,
    after_focus_runtime_id: Option<String>,
) -> UiControlHostResponse {
    UiControlHostResponse::ActionCompleted {
        success,
        target_closed,
        policy_tier,
        message: message.into(),
        error,
        before_focus_runtime_id,
        after_focus_runtime_id,
    }
}

#[cfg(windows)]
fn default_runtime() -> Box<dyn HostRuntime> {
    Box::new(runtime_windows::WindowsHostRuntime)
}

#[cfg(not(windows))]
fn default_runtime() -> Box<dyn HostRuntime> {
    Box::new(UnsupportedRuntime)
}

#[cfg(windows)]
fn default_confirmation_surface() -> Box<dyn ConfirmationSurface> {
    Box::new(runtime_windows::WindowsConfirmationSurface)
}

#[cfg(not(windows))]
fn default_confirmation_surface() -> Box<dyn ConfirmationSurface> {
    Box::new(RejectingConfirmationSurface)
}

#[cfg(not(windows))]
struct UnsupportedRuntime;

#[cfg(not(windows))]
impl HostRuntime for UnsupportedRuntime {
    fn open(
        &self,
        _grant: &UiControlTaskGrant,
        _session_id: &str,
    ) -> Result<Box<dyn HostRuntimeSession>, HostFailure> {
        Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "dcc-mcp-ui-control-host is only supported on Windows",
        ))
    }
}

#[cfg(not(windows))]
struct RejectingConfirmationSurface;

#[cfg(not(windows))]
impl ConfirmationSurface for RejectingConfirmationSurface {
    fn confirm(
        &self,
        kind: ConfirmationKind<'_>,
        _target: Option<&UiControlTarget>,
        _action: Option<&UiControlAction>,
    ) -> Result<bool, HostFailure> {
        match kind {
            ConfirmationKind::ConsequentialAction(tier) => {
                let _ = tier;
            }
            ConfirmationKind::SystemOperation(operation) => {
                let _ = operation;
            }
            ConfirmationKind::ResumeAfterStop => {}
        }
        Ok(false)
    }
}

/// Run the dedicated executable using the current process arguments.
#[doc(hidden)]
#[must_use]
pub fn run_from_env() -> i32 {
    if std::env::args_os().skip(1).any(|arg| arg == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if std::env::args_os().skip(1).any(|arg| arg == "--self-check") {
        return self_check();
    }
    #[cfg(windows)]
    {
        match windows::run() {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("UI Control host failed: {message}");
                70
            }
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("dcc-mcp-ui-control-host is only supported on Windows");
        78
    }
}

fn self_check() -> i32 {
    let mut host = UiControlHost::default();
    let mut connection = UiControlHostConnection::default();
    let hello = connection.handle(
        &mut host,
        UiControlHostRequest::Hello(UiControlHostHello {
            protocol_version: UI_CONTROL_HOST_PROTOCOL_VERSION,
            client_name: "self-check".to_owned(),
        }),
    );
    i32::from(!matches!(hello, UiControlHostResponse::Hello { .. }))
}

#[cfg(test)]
mod tests;
