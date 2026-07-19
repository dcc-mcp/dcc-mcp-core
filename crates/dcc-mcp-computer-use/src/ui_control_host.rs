//! Isolated UI Control host state machine and executable entry point.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use dcc_mcp_app_ui::host_protocol::{
    UI_CONTROL_HOST_CAPABILITIES, UI_CONTROL_HOST_PROTOCOL_VERSION, UiControlAction,
    UiControlHostErrorCode, UiControlHostHello, UiControlHostRequest, UiControlHostResponse,
    UiControlInputKind, UiControlPolicyTier, UiControlSharedImage, UiControlTarget,
    UiControlTaskGrant, UiControlWindowOperation, UiControlWindowState,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

#[cfg(windows)]
mod runtime_windows;
#[cfg(windows)]
mod windows;

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

struct RuntimeActionResult {
    message: String,
    before_focus_runtime_id: Option<String>,
    after_focus_runtime_id: Option<String>,
}

trait HostRuntimeSession: Send {
    fn target(&self) -> &UiControlTarget;
    fn window_state(&mut self) -> Result<UiControlWindowState, HostFailure>;
    fn change_window_state(
        &mut self,
        operation: UiControlWindowOperation,
    ) -> Result<UiControlWindowState, HostFailure>;
    fn snapshot(&mut self, max_depth: u32, max_nodes: u32) -> Result<RuntimeSnapshot, HostFailure>;
    fn execute(
        &mut self,
        observation_id: &str,
        action: &UiControlAction,
    ) -> Result<RuntimeActionResult, HostFailure>;
    fn resume_after_approval(&mut self) -> Result<(), HostFailure>;
    fn stop(&mut self) -> bool;
}

trait HostRuntime: Send + Sync {
    fn open(&self, grant: &UiControlTaskGrant) -> Result<Box<dyn HostRuntimeSession>, HostFailure>;
}

#[derive(Debug, Clone, Copy)]
enum ConfirmationKind {
    TaskGrant,
    ConsequentialAction(UiControlPolicyTier),
    ResumeAfterStop,
}

trait ConfirmationSurface: Send + Sync {
    fn confirm(
        &self,
        kind: ConfirmationKind,
        target: &UiControlTarget,
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
    runtime: Box<dyn HostRuntimeSession>,
}

/// Process-owned capability, policy, confirmation, and native execution authority.
pub struct UiControlHost {
    sessions: HashMap<String, HostSession>,
    runtime: Box<dyn HostRuntime>,
    confirmation: Box<dyn ConfirmationSurface>,
}

impl Default for UiControlHost {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            runtime: default_runtime(),
            confirmation: default_confirmation_surface(),
        }
    }
}

/// Per-connection handshake state.
#[derive(Debug, Default)]
pub struct UiControlHostConnection {
    negotiated: bool,
    owned_sessions: HashSet<String>,
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
        let session_id = request_session_id(&request).map(str::to_owned);
        if let Some(session_id) = session_id.as_deref()
            && !matches!(request, UiControlHostRequest::OpenSession { .. })
            && !self.owned_sessions.contains(session_id)
        {
            return error(
                UiControlHostErrorCode::SessionNotFound,
                "UI Control session is not owned by this named-pipe connection",
            );
        }
        let response = host.handle(request);
        match &response {
            UiControlHostResponse::SessionOpened { session_id, .. } => {
                self.owned_sessions.insert(session_id.clone());
            }
            UiControlHostResponse::SessionStopped { session_id, .. } => {
                self.owned_sessions.remove(session_id);
            }
            _ => {}
        }
        response
    }

    /// Stop every session minted for this pipe when the client disconnects.
    #[cfg(any(windows, test))]
    fn disconnect(&mut self, host: &mut UiControlHost) {
        for session_id in self.owned_sessions.drain() {
            let _ = host.stop_session(session_id);
        }
        self.negotiated = false;
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

fn request_session_id(request: &UiControlHostRequest) -> Option<&str> {
    match request {
        UiControlHostRequest::Hello(_) => None,
        UiControlHostRequest::OpenSession { session_id, .. }
        | UiControlHostRequest::GetWindowState { session_id, .. }
        | UiControlHostRequest::ChangeWindowState { session_id, .. }
        | UiControlHostRequest::Snapshot { session_id, .. }
        | UiControlHostRequest::ExecuteAction { session_id, .. }
        | UiControlHostRequest::ResumeSession { session_id, .. }
        | UiControlHostRequest::StopSession { session_id } => Some(session_id),
    }
}

impl UiControlHost {
    fn handle(&mut self, request: UiControlHostRequest) -> UiControlHostResponse {
        match request {
            UiControlHostRequest::Hello(_) => unreachable!("hello is handled by the connection"),
            UiControlHostRequest::OpenSession { session_id, grant } => {
                self.open_session(session_id, grant)
            }
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
            UiControlHostRequest::ResumeSession {
                session_id,
                task_grant_id,
                window_capability,
            } => self.resume_session(&session_id, &task_grant_id, &window_capability),
            UiControlHostRequest::StopSession { session_id } => self.stop_session(session_id),
        }
    }

    fn open_session(
        &mut self,
        session_id: String,
        grant: UiControlTaskGrant,
    ) -> UiControlHostResponse {
        if !valid_wire_label(&session_id, 128)
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
        if self.sessions.contains_key(&session_id) {
            return error(
                UiControlHostErrorCode::SessionAlreadyExists,
                "stop the existing session before replacing its task grant",
            );
        }

        let mut runtime = match self.runtime.open(&grant) {
            Ok(runtime) => runtime,
            Err(failure) => return failure.into_response(),
        };
        let target = runtime.target().clone();
        match self
            .confirmation
            .confirm(ConfirmationKind::TaskGrant, &target, None)
        {
            Ok(true) => {}
            Ok(false) => {
                runtime.stop();
                audit_event(
                    &grant,
                    "open_session",
                    false,
                    UiControlPolicyTier::PreApproval,
                    Some(UiControlHostErrorCode::ApprovalRequired),
                );
                return error(
                    UiControlHostErrorCode::ApprovalRequired,
                    "the user did not approve UI Control for the selected window",
                );
            }
            Err(failure) => {
                runtime.stop();
                return failure.into_response();
            }
        }

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
                runtime,
            },
        );
        audit_event(
            &grant,
            "open_session",
            true,
            UiControlPolicyTier::PreApproval,
            None,
        );
        UiControlHostResponse::SessionOpened {
            session_id,
            window_capability,
            target,
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

        let policy_tier = classify_action(
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
        if policy_tier >= UiControlPolicyTier::PreApproval {
            match confirmation.confirm(
                ConfirmationKind::ConsequentialAction(policy_tier),
                session.runtime.target(),
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
        }

        // Every attempted mutation consumes both fences, including backend failures.
        session.observation_id = None;
        session.observation = None;
        session.accessibility_state_id = None;
        session.accessibility_root = None;
        match session.runtime.execute(observation_id, &action) {
            Ok(result) => {
                audit_event(&session.grant, &action.action, true, policy_tier, None);
                action_response(
                    true,
                    policy_tier,
                    result.message,
                    None,
                    result.before_focus_runtime_id,
                    result.after_focus_runtime_id,
                )
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
        match confirmation.confirm(
            ConfirmationKind::ResumeAfterStop,
            session.runtime.target(),
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
        session.observation_id = None;
        session.observation = None;
        session.accessibility_state_id = None;
        session.accessibility_root = None;
        UiControlHostResponse::SessionResumed {
            session_id: session_id.to_owned(),
        }
    }

    fn stop_session(&mut self, session_id: String) -> UiControlHostResponse {
        let Some(mut session) = self.sessions.remove(&session_id) else {
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
        UiControlHostResponse::SessionStopped {
            session_id,
            cleanup_pending,
        }
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
    UiControlHostResponse::ActionCompleted {
        success,
        policy_tier,
        message: message.into(),
        error,
        before_focus_runtime_id,
        after_focus_runtime_id,
    }
}

fn classify_action(
    action: &UiControlAction,
    root: Option<&Value>,
    observation: Option<&Value>,
) -> UiControlPolicyTier {
    let mut tier = action.intent.policy_tier();
    let normalized_keys = action
        .keys
        .iter()
        .map(|key| key.to_ascii_lowercase().replace(' ', ""))
        .collect::<Vec<_>>()
        .join("+");
    if normalized_keys.contains("win+r")
        || normalized_keys.contains("meta+r")
        || normalized_keys.contains("ctrl+shift+esc")
    {
        return UiControlPolicyTier::HardDeny;
    }

    if let Some(control_id) = action.control_id.as_deref()
        && let Some(control) = root.and_then(|root| find_control(root, control_id))
    {
        tier = tier.max(classify_control(control));
    } else if action.input_kind == UiControlInputKind::RawInput
        && let Some(root) = root
    {
        let visual_target = action
            .x
            .zip(action.y)
            .and_then(|point| screenshot_to_desktop(point, observation))
            .and_then(|(x, y)| find_control_at_point(root, x, y))
            .or_else(|| find_focused_control(root));
        if let Some(control) = visual_target {
            tier = tier.max(classify_control(control));
        }
    }
    tier
}

fn screenshot_to_desktop(point: (f64, f64), observation: Option<&Value>) -> Option<(f64, f64)> {
    let observation = observation?;
    let rect = observation.get("source_rect")?.as_array()?;
    if rect.len() != 4 {
        return None;
    }
    let source_x = rect[0].as_i64()? as f64;
    let source_y = rect[1].as_i64()? as f64;
    let source_width = rect[2].as_i64()? as f64;
    let source_height = rect[3].as_i64()? as f64;
    let image_width = observation.get("width")?.as_u64()? as f64;
    let image_height = observation.get("height")?.as_u64()? as f64;
    if source_width <= 0.0 || source_height <= 0.0 || image_width <= 0.0 || image_height <= 0.0 {
        return None;
    }
    Some((
        source_x + point.0 * source_width / image_width,
        source_y + point.1 * source_height / image_height,
    ))
}

fn classify_control(control: &Value) -> UiControlPolicyTier {
    if control
        .get("is_password")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return UiControlPolicyTier::HardDeny;
    }
    let classification_text = ["name", "automation_id", "class_name"]
        .iter()
        .filter_map(|key| control.get(key).and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    classify_control_text(&classification_text)
}

fn classify_control_text(text: &str) -> UiControlPolicyTier {
    const HARD_DENY: &[&str] = &[
        "password",
        "credential",
        "authentication code",
        "security settings",
        "privacy settings",
    ];
    const ALWAYS_CONFIRM: &[&str] = &[
        "delete",
        "remove permanently",
        "overwrite",
        "install",
        "purchase",
        "buy now",
        "pay",
        "send",
        "publish",
        "submit",
        "share",
        "grant access",
        "revoke access",
    ];
    const PRE_APPROVE: &[&str] = &[
        "sign in",
        "log in",
        "login",
        "permission",
        "upload",
        "move",
        "rename",
        "connect account",
    ];
    if HARD_DENY.iter().any(|needle| text.contains(needle)) {
        UiControlPolicyTier::HardDeny
    } else if ALWAYS_CONFIRM.iter().any(|needle| text.contains(needle)) {
        UiControlPolicyTier::ActionConfirmation
    } else if PRE_APPROVE.iter().any(|needle| text.contains(needle)) {
        UiControlPolicyTier::PreApproval
    } else {
        UiControlPolicyTier::TaskGrant
    }
}

fn find_control<'a>(node: &'a Value, control_id: &str) -> Option<&'a Value> {
    let runtime_id = control_id.strip_prefix("uia:").unwrap_or(control_id);
    if node.get("runtime_id").and_then(Value::as_str) == Some(runtime_id) {
        return Some(node);
    }
    node.get("children")
        .and_then(Value::as_array)
        .and_then(|children| {
            children
                .iter()
                .find_map(|child| find_control(child, control_id))
        })
}

fn find_control_at_point(node: &Value, x: f64, y: f64) -> Option<&Value> {
    let bounds = node.get("bounds")?;
    let left = bounds.get("x")?.as_f64()?;
    let top = bounds.get("y")?.as_f64()?;
    let width = bounds.get("width")?.as_f64()?;
    let height = bounds.get("height")?.as_f64()?;
    if x < left || y < top || x >= left + width || y >= top + height {
        return None;
    }
    node.get("children")
        .and_then(Value::as_array)
        .and_then(|children| {
            children
                .iter()
                .rev()
                .find_map(|child| find_control_at_point(child, x, y))
        })
        .or(Some(node))
}

fn find_focused_control(node: &Value) -> Option<&Value> {
    if node.get("focused").and_then(Value::as_bool) == Some(true) {
        return Some(node);
    }
    node.get("children")
        .and_then(Value::as_array)
        .and_then(|children| children.iter().find_map(find_focused_control))
}

fn audit_event(
    grant: &UiControlTaskGrant,
    action: &str,
    success: bool,
    policy_tier: UiControlPolicyTier,
    error_code: Option<UiControlHostErrorCode>,
) {
    let payload = json!({
        "event": "ui_control_operation",
        "tool": "dcc-mcp-ui-control-host",
        "dcc_type": grant.dcc_type,
        "action": action,
        "success": success,
        "error": error_code,
        "message": if success { "DCC UI Control host operation succeeded" } else { "DCC UI Control host operation rejected" },
        "detail": format!("action={action} tier={policy_tier:?}"),
    });
    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned());
    let level = if success { "INFO" } else { "WARN" };
    let line = format!(
        "{timestamp} {level} dcc_mcp_ui_control_host.audit: {}\n",
        payload
    );
    eprint!("{line}");
    let path = audit_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn audit_log_path() -> PathBuf {
    let directory = std::env::var_os("DCC_MCP_LOG_DIR")
        .map(PathBuf::from)
        .or_else(|| dcc_mcp_paths::get_log_dir().ok().map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    directory.join(format!(
        "dcc-mcp-ui-control-host.{}.log",
        std::process::id()
    ))
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
        kind: ConfirmationKind,
        _target: &UiControlTarget,
        _action: Option<&UiControlAction>,
    ) -> Result<bool, HostFailure> {
        if let ConfirmationKind::ConsequentialAction(tier) = kind {
            let _ = tier;
        }
        Ok(false)
    }
}

/// Run the dedicated executable using the current process arguments.
#[doc(hidden)]
#[must_use]
pub fn run_from_env() -> i32 {
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
mod tests {
    use super::*;
    use dcc_mcp_app_ui::host_protocol::{UiControlInputKind, UiControlIntent};

    struct FakeRuntime;

    struct FakeSession {
        target: UiControlTarget,
    }

    impl HostRuntime for FakeRuntime {
        fn open(
            &self,
            grant: &UiControlTaskGrant,
        ) -> Result<Box<dyn HostRuntimeSession>, HostFailure> {
            Ok(Box::new(FakeSession {
                target: UiControlTarget {
                    process_id: grant.process_id.unwrap_or(42),
                    window_handle: grant.window_handle.unwrap_or(0x1234),
                    window_title: "Test DCC".to_owned(),
                },
            }))
        }
    }

    impl HostRuntimeSession for FakeSession {
        fn target(&self) -> &UiControlTarget {
            &self.target
        }

        fn window_state(&mut self) -> Result<UiControlWindowState, HostFailure> {
            Ok(UiControlWindowState {
                process_id: self.target.process_id,
                window_handle: self.target.window_handle,
                exists: true,
                visible: true,
                minimized: false,
                foreground: true,
            })
        }

        fn change_window_state(
            &mut self,
            _operation: UiControlWindowOperation,
        ) -> Result<UiControlWindowState, HostFailure> {
            self.window_state()
        }

        fn snapshot(
            &mut self,
            _max_depth: u32,
            _max_nodes: u32,
        ) -> Result<RuntimeSnapshot, HostFailure> {
            Ok(RuntimeSnapshot {
                observation_id: "obs-1".to_owned(),
                target: self.target.clone(),
                observation: json!({"observation_id": "obs-1"}),
                root: json!({
                    "runtime_id": "42.1",
                    "name": "Delete",
                    "is_password": false,
                    "children": [],
                }),
                focus_runtime_id: None,
                node_count: 1,
                image: UiControlSharedImage {
                    name: "test".to_owned(),
                    id: "test".to_owned(),
                    length: 3,
                    mime_type: "image/png".to_owned(),
                },
            })
        }

        fn execute(
            &mut self,
            _observation_id: &str,
            _action: &UiControlAction,
        ) -> Result<RuntimeActionResult, HostFailure> {
            Ok(RuntimeActionResult {
                message: "completed".to_owned(),
                before_focus_runtime_id: None,
                after_focus_runtime_id: None,
            })
        }

        fn resume_after_approval(&mut self) -> Result<(), HostFailure> {
            Ok(())
        }

        fn stop(&mut self) -> bool {
            false
        }
    }

    struct AllowConfirmation;

    impl ConfirmationSurface for AllowConfirmation {
        fn confirm(
            &self,
            _kind: ConfirmationKind,
            _target: &UiControlTarget,
            _action: Option<&UiControlAction>,
        ) -> Result<bool, HostFailure> {
            Ok(true)
        }
    }

    fn host() -> UiControlHost {
        UiControlHost {
            sessions: HashMap::new(),
            runtime: Box::new(FakeRuntime),
            confirmation: Box::new(AllowConfirmation),
        }
    }

    fn grant(raw: bool) -> UiControlTaskGrant {
        UiControlTaskGrant {
            task_grant_id: "grant-1".to_owned(),
            dcc_type: "unreal".to_owned(),
            process_id: Some(42),
            window_handle: Some(0x1234),
            allow_raw_input: raw,
        }
    }

    fn action(control_id: Option<&str>, input_kind: UiControlInputKind) -> UiControlAction {
        UiControlAction {
            action: "click".to_owned(),
            control_id: control_id.map(str::to_owned),
            input_kind,
            intent: UiControlIntent::OrdinaryEdit,
            x: None,
            y: None,
            button: None,
            scroll_x: None,
            scroll_y: None,
            path: Vec::new(),
            text: None,
            keys: Vec::new(),
            checked: None,
            duration_ms: None,
        }
    }

    fn negotiated() -> (UiControlHost, UiControlHostConnection) {
        let mut host = host();
        let mut connection = UiControlHostConnection::default();
        assert!(matches!(
            connection.handle(
                &mut host,
                UiControlHostRequest::Hello(UiControlHostHello {
                    protocol_version: UI_CONTROL_HOST_PROTOCOL_VERSION,
                    client_name: "test".to_owned(),
                })
            ),
            UiControlHostResponse::Hello { .. }
        ));
        (host, connection)
    }

    #[test]
    fn handshake_is_required_and_exact() {
        let mut host = host();
        let mut connection = UiControlHostConnection::default();
        assert!(matches!(
            connection.handle(
                &mut host,
                UiControlHostRequest::StopSession {
                    session_id: "missing".to_owned(),
                }
            ),
            UiControlHostResponse::Error {
                code: UiControlHostErrorCode::HandshakeRequired,
                ..
            }
        ));
    }

    #[test]
    fn exact_target_capability_and_observation_are_required() {
        let (mut host, mut connection) = negotiated();
        let opened = connection.handle(
            &mut host,
            UiControlHostRequest::OpenSession {
                session_id: "session-1".to_owned(),
                grant: grant(false),
            },
        );
        let UiControlHostResponse::SessionOpened {
            window_capability,
            target,
            ..
        } = opened
        else {
            panic!("session not opened: {opened:?}");
        };
        assert_eq!(target.process_id, 42);
        let state = connection.handle(
            &mut host,
            UiControlHostRequest::GetWindowState {
                session_id: "session-1".to_owned(),
                task_grant_id: "grant-1".to_owned(),
                window_capability: window_capability.clone(),
            },
        );
        assert!(matches!(
            state,
            UiControlHostResponse::WindowState {
                state: UiControlWindowState {
                    exists: true,
                    minimized: false,
                    ..
                },
                ..
            }
        ));
        let snapshot = connection.handle(
            &mut host,
            UiControlHostRequest::Snapshot {
                session_id: "session-1".to_owned(),
                task_grant_id: "grant-1".to_owned(),
                window_capability: window_capability.clone(),
                max_depth: 5,
                max_nodes: 250,
            },
        );
        let UiControlHostResponse::Snapshot {
            observation_id,
            accessibility_state_id,
            ..
        } = snapshot
        else {
            panic!("snapshot failed: {snapshot:?}");
        };
        let completed = connection.handle(
            &mut host,
            UiControlHostRequest::ExecuteAction {
                session_id: "session-1".to_owned(),
                task_grant_id: "grant-1".to_owned(),
                window_capability,
                observation_id,
                accessibility_state_id,
                action: Box::new(action(Some("uia:42.1"), UiControlInputKind::Semantic)),
            },
        );
        assert!(matches!(
            completed,
            UiControlHostResponse::ActionCompleted {
                success: true,
                policy_tier: UiControlPolicyTier::ActionConfirmation,
                ..
            }
        ));
    }

    #[test]
    fn window_recovery_does_not_require_an_observation_and_disconnect_revokes_it() {
        let (mut host, mut connection) = negotiated();
        let opened = connection.handle(
            &mut host,
            UiControlHostRequest::OpenSession {
                session_id: "session-recovery".to_owned(),
                grant: grant(false),
            },
        );
        let UiControlHostResponse::SessionOpened {
            window_capability, ..
        } = opened
        else {
            panic!("session not opened: {opened:?}");
        };
        let changed = connection.handle(
            &mut host,
            UiControlHostRequest::ChangeWindowState {
                session_id: "session-recovery".to_owned(),
                task_grant_id: "grant-1".to_owned(),
                window_capability,
                operation: UiControlWindowOperation::Restore,
            },
        );
        assert!(matches!(
            changed,
            UiControlHostResponse::WindowStateChanged {
                operation: UiControlWindowOperation::Restore,
                ..
            }
        ));
        connection.disconnect(&mut host);
        assert!(host.sessions.is_empty());
    }

    #[test]
    fn one_pipe_cannot_address_another_pipes_session() {
        let (mut host, mut owner) = negotiated();
        let opened = owner.handle(
            &mut host,
            UiControlHostRequest::OpenSession {
                session_id: "owned".to_owned(),
                grant: grant(false),
            },
        );
        let UiControlHostResponse::SessionOpened {
            window_capability, ..
        } = opened
        else {
            panic!("session not opened: {opened:?}");
        };
        let (_, mut other) = negotiated();
        let response = other.handle(
            &mut host,
            UiControlHostRequest::GetWindowState {
                session_id: "owned".to_owned(),
                task_grant_id: "grant-1".to_owned(),
                window_capability,
            },
        );
        assert!(matches!(
            response,
            UiControlHostResponse::Error {
                code: UiControlHostErrorCode::SessionNotFound,
                ..
            }
        ));
    }

    #[test]
    fn raw_input_grant_and_hard_denies_cannot_be_bypassed() {
        assert_eq!(
            classify_action(
                &UiControlAction {
                    keys: vec!["WIN+R".to_owned()],
                    input_kind: UiControlInputKind::RawInput,
                    ..action(None, UiControlInputKind::RawInput)
                },
                None,
                None,
            ),
            UiControlPolicyTier::HardDeny
        );
        assert_eq!(
            classify_control_text("password field"),
            UiControlPolicyTier::HardDeny
        );
        assert!(
            validate_action_descriptor(&UiControlAction {
                action: "secret supplied as an action name".to_owned(),
                ..action(Some("uia:42.1"), UiControlInputKind::Semantic)
            })
            .is_err()
        );
    }
}
