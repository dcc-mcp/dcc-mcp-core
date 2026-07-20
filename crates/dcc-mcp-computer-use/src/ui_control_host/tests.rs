use super::system_operations::{
    parse_system_grants, valid_windows_absolute_path, validate_system_operation,
};
use super::*;
use dcc_mcp_ui_control::host_protocol::{
    UiControlInputKind, UiControlIntent, UiControlPoint, UiControlSystemGrantOperation,
};
use std::collections::VecDeque;
use std::sync::Mutex;

struct FakeRuntime {
    snapshot: RuntimeAccessibilityState,
    live_states: Mutex<VecDeque<RuntimeAccessibilityState>>,
}

struct FakeSession {
    target: UiControlTarget,
    snapshot: RuntimeAccessibilityState,
    live_states: VecDeque<RuntimeAccessibilityState>,
}

impl Default for FakeRuntime {
    fn default() -> Self {
        Self {
            snapshot: fake_accessibility_state(),
            live_states: Mutex::new(VecDeque::new()),
        }
    }
}

impl HostRuntime for FakeRuntime {
    fn open(&self, grant: &UiControlTaskGrant) -> Result<Box<dyn HostRuntimeSession>, HostFailure> {
        Ok(Box::new(FakeSession {
            target: UiControlTarget {
                process_id: grant.process_id.unwrap_or(42),
                window_handle: grant.window_handle.unwrap_or(0x1234),
                window_title: "Test DCC".to_owned(),
            },
            snapshot: self.snapshot.clone(),
            live_states: std::mem::take(
                &mut *self
                    .live_states
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ),
        }))
    }
}

impl HostRuntimeSession for FakeSession {
    fn target(&self) -> &UiControlTarget {
        &self.target
    }

    fn start_visible_notice(&mut self) -> Result<(), HostFailure> {
        Ok(())
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
            root: self.snapshot.root.clone(),
            focus_runtime_id: self.snapshot.focus_runtime_id.clone(),
            node_count: self.snapshot.node_count,
            image: UiControlSharedImage {
                name: "test".to_owned(),
                id: "test".to_owned(),
                length: 3,
                mime_type: "image/png".to_owned(),
            },
        })
    }

    fn accessibility_state(
        &mut self,
        _max_depth: u32,
        _max_nodes: u32,
    ) -> Result<RuntimeAccessibilityState, HostFailure> {
        Ok(self
            .live_states
            .pop_front()
            .unwrap_or_else(|| self.snapshot.clone()))
    }

    fn execute(
        &mut self,
        _observation_id: &str,
        _action: &UiControlAction,
        _fence: &ActionFenceExpectation,
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
        _kind: ConfirmationKind<'_>,
        _target: Option<&UiControlTarget>,
        _action: Option<&UiControlAction>,
    ) -> Result<bool, HostFailure> {
        Ok(true)
    }
}

struct DenyConfirmation;

impl ConfirmationSurface for DenyConfirmation {
    fn confirm(
        &self,
        _kind: ConfirmationKind<'_>,
        _target: Option<&UiControlTarget>,
        _action: Option<&UiControlAction>,
    ) -> Result<bool, HostFailure> {
        Ok(false)
    }
}

fn host() -> UiControlHost {
    UiControlHost {
        sessions: HashMap::new(),
        system_sessions: HashMap::new(),
        system_grants: HashMap::new(),
        runtime: Box::new(FakeRuntime::default()),
        confirmation: Box::new(AllowConfirmation),
    }
}

fn fake_accessibility_state() -> RuntimeAccessibilityState {
    RuntimeAccessibilityState {
        root: json!({
            "runtime_id": "42.1",
            "name": "Delete",
            "is_password": false,
            "children": [],
        }),
        focus_runtime_id: None,
        node_count: 1,
    }
}

fn host_with_accessibility_states(
    snapshot: RuntimeAccessibilityState,
    live_states: Vec<RuntimeAccessibilityState>,
) -> UiControlHost {
    UiControlHost {
        sessions: HashMap::new(),
        system_sessions: HashMap::new(),
        system_grants: HashMap::new(),
        runtime: Box::new(FakeRuntime {
            snapshot,
            live_states: Mutex::new(live_states.into()),
        }),
        confirmation: Box::new(AllowConfirmation),
    }
}

fn keyboard_accessibility_state(focus_runtime_id: &str) -> RuntimeAccessibilityState {
    RuntimeAccessibilityState {
        root: json!({
            "runtime_id": "42.root",
            "name": "Maya",
            "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
            "children": [
                {
                    "runtime_id": "42.ordinary",
                    "name": "Viewport",
                    "is_password": false,
                    "focused": focus_runtime_id == "42.ordinary",
                    "bounds": {"x": 0.0, "y": 0.0, "width": 50.0, "height": 100.0},
                    "children": []
                },
                {
                    "runtime_id": "42.password",
                    "name": "Password",
                    "is_password": true,
                    "focused": focus_runtime_id == "42.password",
                    "bounds": {"x": 50.0, "y": 0.0, "width": 50.0, "height": 100.0},
                    "children": []
                }
            ]
        }),
        focus_runtime_id: Some(focus_runtime_id.to_owned()),
        node_count: 3,
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

fn registry_operation(value: u32) -> UiControlSystemOperation {
    UiControlSystemOperation::EnsureRegistryDword {
        key: "Software\\Vendor\\Plugin".to_owned(),
        value_name: "RemoteEnabled".to_owned(),
        value,
    }
}

fn named_registry_operation(operation_id: &str, value: u32) -> UiControlSystemGrantOperation {
    UiControlSystemGrantOperation {
        operation_id: operation_id.to_owned(),
        operation: registry_operation(value),
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
fn routine_session_is_notice_only() {
    let (mut host, mut connection) = negotiated();
    host.confirmation = Box::new(DenyConfirmation);
    assert!(matches!(
        connection.handle(
            &mut host,
            UiControlHostRequest::OpenSession {
                session_id: "notice-only".to_owned(),
                grant: grant(false),
            },
        ),
        UiControlHostResponse::SessionOpened { .. }
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
    for chord in ["WIN+R", "CONTROL+SHIFT+ESCAPE", "ALT+TAB", "ALT+SPACE"] {
        assert_eq!(
            classify_action(
                &UiControlAction {
                    keys: vec![chord.to_owned()],
                    input_kind: UiControlInputKind::RawInput,
                    ..action(None, UiControlInputKind::RawInput)
                },
                None,
                None,
            ),
            UiControlPolicyTier::HardDeny,
            "{chord}"
        );
    }
    for chord in [
        "ALT+F4",
        "CTRL+W",
        "CONTROL+F04",
        "CTRL+Q",
        "DELETE",
        "DEL",
        "SHIFT+DELETE",
        "CTRL+S",
        "CONTROL+SHIFT+S",
    ] {
        assert_eq!(
            classify_action(
                &UiControlAction {
                    keys: vec![chord.to_owned()],
                    input_kind: UiControlInputKind::RawInput,
                    ..action(None, UiControlInputKind::RawInput)
                },
                None,
                None,
            ),
            UiControlPolicyTier::ActionConfirmation,
            "{chord}"
        );
    }
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
    assert_eq!(
        classify_action(
            &UiControlAction {
                action: "type".to_owned(),
                text: Some("unsafe raw text".to_owned()),
                input_kind: UiControlInputKind::RawInput,
                ..action(None, UiControlInputKind::RawInput)
            },
            None,
            None,
        ),
        UiControlPolicyTier::HardDeny
    );
    for chord in ["A", "1", "SHIFT+A", "SHIFT+SEMICOLON", "SPACE"] {
        assert_eq!(
            classify_action(
                &UiControlAction {
                    action: "keypress".to_owned(),
                    keys: vec![chord.to_owned()],
                    input_kind: UiControlInputKind::RawInput,
                    ..action(None, UiControlInputKind::RawInput)
                },
                None,
                None,
            ),
            UiControlPolicyTier::HardDeny,
            "{chord}"
        );
    }
    let password_state = keyboard_accessibility_state("42.password");
    assert_eq!(
        classify_action(
            &UiControlAction {
                action: "keypress".to_owned(),
                keys: vec!["A".to_owned()],
                input_kind: UiControlInputKind::RawInput,
                ..action(None, UiControlInputKind::RawInput)
            },
            Some(&password_state.root),
            None,
        ),
        UiControlPolicyTier::HardDeny
    );
}

#[test]
fn raw_drag_classifies_interpolated_points_between_ordinary_endpoints() {
    let root = json!({
        "runtime_id": "root",
        "name": "Face editor",
        "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
        "children": [
            {
                "runtime_id": "ordinary-left",
                "name": "Left face control",
                "bounds": {"x": 0.0, "y": 0.0, "width": 40.0, "height": 100.0},
                "children": []
            },
            {
                "runtime_id": "password",
                "name": "Password",
                "is_password": true,
                "bounds": {"x": 40.0, "y": 0.0, "width": 20.0, "height": 100.0},
                "children": []
            },
            {
                "runtime_id": "ordinary-right",
                "name": "Right face control",
                "bounds": {"x": 60.0, "y": 0.0, "width": 40.0, "height": 100.0},
                "children": []
            }
        ]
    });
    let observation = json!({
        "source_rect": [0, 0, 100, 100],
        "width": 100,
        "height": 100
    });
    let drag = UiControlAction {
        action: "drag".to_owned(),
        path: vec![
            UiControlPoint { x: 10.0, y: 50.0 },
            UiControlPoint { x: 90.0, y: 50.0 },
        ],
        duration_ms: Some(64),
        ..action(None, UiControlInputKind::RawInput)
    };

    assert_eq!(
        classify_action(&drag, Some(&root), Some(&observation)),
        UiControlPolicyTier::HardDeny
    );
}

#[test]
fn focused_password_cannot_be_hidden_behind_innocuous_keyboard_coordinates() {
    let root = json!({
        "runtime_id": "root",
        "name": "Login panel",
        "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
        "children": [
            {
                "runtime_id": "ordinary",
                "name": "Ordinary control",
                "bounds": {"x": 0.0, "y": 0.0, "width": 50.0, "height": 100.0},
                "children": []
            },
            {
                "runtime_id": "password",
                "name": "Password",
                "is_password": true,
                "focused": true,
                "bounds": {"x": 50.0, "y": 0.0, "width": 50.0, "height": 100.0},
                "children": []
            }
        ]
    });
    let observation = json!({
        "source_rect": [0, 0, 100, 100],
        "width": 100,
        "height": 100
    });
    let typed = UiControlAction {
        action: "type".to_owned(),
        x: Some(25.0),
        y: Some(50.0),
        text: Some("redacted".to_owned()),
        ..action(None, UiControlInputKind::RawInput)
    };

    assert!(validate_action_descriptor(&typed).is_err());
    assert_eq!(
        classify_action(&typed, Some(&root), Some(&observation)),
        UiControlPolicyTier::HardDeny
    );
}

#[test]
fn action_descriptors_cannot_hide_the_executor_target_or_required_value() {
    let scroll_without_target = UiControlAction {
        action: "scroll".to_owned(),
        scroll_y: Some(120),
        ..action(None, UiControlInputKind::RawInput)
    };
    assert!(validate_action_descriptor(&scroll_without_target).is_err());

    let keypress_with_pointer = UiControlAction {
        action: "keypress".to_owned(),
        keys: vec!["A".to_owned()],
        x: Some(10.0),
        y: Some(10.0),
        ..action(None, UiControlInputKind::RawInput)
    };
    assert!(validate_action_descriptor(&keypress_with_pointer).is_err());

    for keys in [["A"], ["SHIFT+A"], ["CTRL+V"], ["WIN"]] {
        let pointer_with_non_modifier = UiControlAction {
            action: "click".to_owned(),
            x: Some(10.0),
            y: Some(10.0),
            keys: keys.into_iter().map(str::to_owned).collect(),
            ..action(None, UiControlInputKind::RawInput)
        };
        assert!(validate_action_descriptor(&pointer_with_non_modifier).is_err());
    }

    let pointer_with_modifiers = UiControlAction {
        action: "click".to_owned(),
        x: Some(10.0),
        y: Some(10.0),
        keys: vec!["CTRL+SHIFT+ALT".to_owned()],
        ..action(None, UiControlInputKind::RawInput)
    };
    assert!(validate_action_descriptor(&pointer_with_modifiers).is_ok());

    let set_text_without_text = UiControlAction {
        action: "set_text".to_owned(),
        ..action(Some("uia:42.1"), UiControlInputKind::Semantic)
    };
    assert!(validate_action_descriptor(&set_text_without_text).is_err());
}

#[test]
fn action_time_fence_detects_semantic_and_raw_target_changes() {
    let cached = json!({
        "runtime_id": "42.root",
        "name": "Maya",
        "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
        "children": [{
            "runtime_id": "42.target",
            "name": "Viewport",
            "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
            "children": []
        }]
    });
    let changed = json!({
        "runtime_id": "42.root",
        "name": "Maya",
        "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
        "children": [{
            "runtime_id": "42.replacement",
            "name": "Delete",
            "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
            "children": []
        }]
    });
    let live = RuntimeAccessibilityState {
        root: changed,
        focus_runtime_id: None,
        node_count: 2,
    };
    let semantic = action(Some("uia:42.target"), UiControlInputKind::Semantic);
    let semantic_error = verify_action_fence(&semantic, &cached, None, None, &live).unwrap_err();
    assert_eq!(
        semantic_error.code,
        UiControlHostErrorCode::StaleObservation
    );

    let raw = UiControlAction {
        action: "click".to_owned(),
        x: Some(50.0),
        y: Some(50.0),
        input_kind: UiControlInputKind::RawInput,
        ..action(None, UiControlInputKind::RawInput)
    };
    let observation = json!({
        "source_rect": [0, 0, 100, 100],
        "width": 100,
        "height": 100
    });
    let raw_error =
        verify_action_fence(&raw, &cached, None, Some(&observation), &live).unwrap_err();
    assert_eq!(raw_error.code, UiControlHostErrorCode::StaleObservation);
}

#[test]
fn action_time_fence_supports_semantic_fallback_path_ids() {
    let root = json!({
        "runtime_id": "42.root",
        "name": "Maya",
        "children": [{
            "runtime_id": "",
            "fallback_path": "0.0",
            "name": "Viewport",
            "children": []
        }]
    });
    let live = RuntimeAccessibilityState {
        root: root.clone(),
        focus_runtime_id: None,
        node_count: 2,
    };
    let semantic = action(Some("uia:path:0.0"), UiControlInputKind::Semantic);

    assert_eq!(
        verify_action_fence(&semantic, &root, None, None, &live)
            .unwrap()
            .0,
        UiControlPolicyTier::TaskGrant
    );
}

#[test]
fn execution_fence_rejects_same_identity_with_a_changed_security_signature() {
    let cached = json!({
        "runtime_id": "42.root",
        "name": "Maya",
        "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
        "children": [{
            "runtime_id": "42.target",
            "name": "Viewport",
            "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
            "children": []
        }]
    });
    let initial = RuntimeAccessibilityState {
        root: cached.clone(),
        focus_runtime_id: None,
        node_count: 2,
    };
    let action = UiControlAction {
        action: "click".to_owned(),
        x: Some(50.0),
        y: Some(50.0),
        input_kind: UiControlInputKind::RawInput,
        ..action(None, UiControlInputKind::RawInput)
    };
    let observation = json!({
        "source_rect": [0, 0, 100, 100],
        "width": 100,
        "height": 100
    });
    let (_, controls) =
        verify_action_fence(&action, &cached, None, Some(&observation), &initial).unwrap();
    let expected = ActionFenceExpectation {
        controls,
        observation: Some(observation),
        max_depth: 12,
        max_nodes: 2_000,
        policy_tier: UiControlPolicyTier::TaskGrant,
    };
    let changed = RuntimeAccessibilityState {
        root: json!({
            "runtime_id": "42.root",
            "name": "Maya",
            "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
            "children": [{
                "runtime_id": "42.target",
                "name": "Delete",
                "bounds": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0},
                "children": []
            }]
        }),
        focus_runtime_id: None,
        node_count: 2,
    };

    let error = verify_expected_action_fence(&action, &expected, &changed).unwrap_err();
    assert_eq!(error.code, UiControlHostErrorCode::StaleObservation);
}

#[test]
fn confirmation_round_trip_rechecks_keyboard_focus_before_input() {
    let snapshot = keyboard_accessibility_state("42.ordinary");
    let before_confirmation = keyboard_accessibility_state("42.ordinary");
    let after_confirmation = keyboard_accessibility_state("42.password");
    let mut host =
        host_with_accessibility_states(snapshot, vec![before_confirmation, after_confirmation]);
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
    let opened = connection.handle(
        &mut host,
        UiControlHostRequest::OpenSession {
            session_id: "keyboard-focus".to_owned(),
            grant: grant(true),
        },
    );
    let UiControlHostResponse::SessionOpened {
        window_capability, ..
    } = opened
    else {
        panic!("session not opened: {opened:?}");
    };
    let snapshot = connection.handle(
        &mut host,
        UiControlHostRequest::Snapshot {
            session_id: "keyboard-focus".to_owned(),
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
    let response = connection.handle(
        &mut host,
        UiControlHostRequest::ExecuteAction {
            session_id: "keyboard-focus".to_owned(),
            task_grant_id: "grant-1".to_owned(),
            window_capability,
            observation_id,
            accessibility_state_id,
            action: Box::new(UiControlAction {
                action: "keypress".to_owned(),
                input_kind: UiControlInputKind::RawInput,
                keys: vec!["CTRL+W".to_owned()],
                ..action(None, UiControlInputKind::RawInput)
            }),
        },
    );

    assert!(matches!(
        response,
        UiControlHostResponse::ActionCompleted {
            success: false,
            error: Some(UiControlHostErrorCode::StaleObservation),
            ..
        }
    ));
    assert!(
        host.sessions
            .get("keyboard-focus")
            .is_some_and(|session| session.observation_id.is_none())
    );
}

#[test]
fn system_session_is_consumed_after_every_execute_attempt() {
    let (mut host, mut connection) = negotiated();
    host.confirmation = Box::new(DenyConfirmation);
    host.system_grants.insert(
        "configured".to_owned(),
        UiControlSystemGrant {
            system_grant_id: "configured".to_owned(),
            dcc_type: "photoshop".to_owned(),
            operations: vec![named_registry_operation("enable-remote", 1)],
        },
    );
    let opened = connection.handle(
        &mut host,
        UiControlHostRequest::OpenSystemSession {
            session_id: "pre-launch-setup".to_owned(),
            system_grant_id: "configured".to_owned(),
        },
    );
    let UiControlHostResponse::SystemSessionOpened {
        system_capability, ..
    } = opened
    else {
        panic!("system session not opened: {opened:?}");
    };
    let widened = connection.handle(
        &mut host,
        UiControlHostRequest::ExecuteSystemOperation {
            session_id: "pre-launch-setup".to_owned(),
            system_grant_id: "configured".to_owned(),
            system_capability: system_capability.clone(),
            operation_id: "not-granted".to_owned(),
        },
    );
    assert!(matches!(
        widened,
        UiControlHostResponse::Error {
            code: UiControlHostErrorCode::SystemOperationNotGranted,
            ..
        }
    ));
    assert!(host.system_sessions.is_empty());
    assert!(connection.owned_system_sessions.is_empty());
    let reused = connection.handle(
        &mut host,
        UiControlHostRequest::ExecuteSystemOperation {
            session_id: "pre-launch-setup".to_owned(),
            system_grant_id: "configured".to_owned(),
            system_capability,
            operation_id: "enable-remote".to_owned(),
        },
    );
    assert!(matches!(
        reused,
        UiControlHostResponse::Error {
            code: UiControlHostErrorCode::SessionNotFound,
            ..
        }
    ));

    let reopened = connection.handle(
        &mut host,
        UiControlHostRequest::OpenSystemSession {
            session_id: "pre-launch-setup-2".to_owned(),
            system_grant_id: "configured".to_owned(),
        },
    );
    let UiControlHostResponse::SystemSessionOpened {
        system_capability, ..
    } = reopened
    else {
        panic!("system session not reopened: {reopened:?}");
    };
    let denied = connection.handle(
        &mut host,
        UiControlHostRequest::ExecuteSystemOperation {
            session_id: "pre-launch-setup-2".to_owned(),
            system_grant_id: "configured".to_owned(),
            system_capability,
            operation_id: "enable-remote".to_owned(),
        },
    );
    assert!(matches!(
        denied,
        UiControlHostResponse::Error {
            code: UiControlHostErrorCode::ApprovalRequired,
            ..
        }
    ));
    assert!(host.system_sessions.is_empty());
    assert!(connection.owned_system_sessions.is_empty());
}

#[test]
fn system_operations_reject_alternate_hives_and_non_absolute_links() {
    let hklm = UiControlSystemOperation::EnsureRegistryString {
        key: "HKLM\\Software\\Vendor".to_owned(),
        value_name: "Enabled".to_owned(),
        value: "yes".to_owned(),
    };
    assert!(matches!(
        validate_system_operation(&hklm),
        Err(HostFailure {
            code: UiControlHostErrorCode::Unsupported,
            ..
        })
    ));
    let startup = UiControlSystemOperation::EnsureRegistryString {
        key: "Software\\Microsoft\\Windows\\CurrentVersion\\Run".to_owned(),
        value_name: "Plugin".to_owned(),
        value: "plugin.exe".to_owned(),
    };
    assert!(matches!(
        validate_system_operation(&startup),
        Err(HostFailure {
            code: UiControlHostErrorCode::HardDenied,
            ..
        })
    ));
    let relative = UiControlSystemOperation::EnsureDirectorySymlink {
        link: "plugins\\vendor".to_owned(),
        target: "D:\\packages\\vendor".to_owned(),
    };
    assert!(validate_system_operation(&relative).is_err());
    assert!(!valid_windows_absolute_path(
        r"C:\plugins\vendor.dll:alternate"
    ));
}

#[test]
fn remote_control_labels_always_require_action_confirmation() {
    for label in [
        "Enable Remote Control",
        "Remote Connections",
        "Allow Remote Clients",
    ] {
        assert_eq!(
            classify_control_text(&label.to_ascii_lowercase()),
            UiControlPolicyTier::ActionConfirmation
        );
    }
}

#[test]
fn save_labels_require_confirmation_without_matching_autosave_settings() {
    for label in ["Save", "Save As", "Save Button", "Save As Menu Item"] {
        assert_eq!(
            classify_control(&json!({"name": label})),
            UiControlPolicyTier::ActionConfirmation,
            "{label}"
        );
    }
    assert_eq!(
        classify_control(&json!({"name": "Autosave Settings"})),
        UiControlPolicyTier::TaskGrant
    );
    assert_eq!(
        classify_control(&json!({"name": "Overwrite existing scene"})),
        UiControlPolicyTier::ActionConfirmation
    );
}

#[test]
fn authentication_secret_markers_are_hard_denied_without_broad_acronym_matches() {
    for label in [
        "Password",
        "Credential",
        "Authentication Code",
        "Verification Code",
        "One-Time Code",
        "OTP",
        "MFA Code",
        "2FA",
        "Passcode",
    ] {
        assert_eq!(
            classify_control_text(&label.to_ascii_lowercase()),
            UiControlPolicyTier::HardDeny,
            "{label}"
        );
    }
    for label in ["Prototype Settings", "Adoption Tool"] {
        assert_ne!(
            classify_control_text(&label.to_ascii_lowercase()),
            UiControlPolicyTier::HardDeny,
            "{label}"
        );
    }
    assert_eq!(
        classify_control_text("login button"),
        UiControlPolicyTier::PreApproval
    );
}

#[test]
fn semantic_set_text_rejects_authentication_markers_on_ancestors() {
    let root = json!({
        "runtime_id": "42.root",
        "name": "One-Time Code",
        "children": [{
            "runtime_id": "42.input",
            "name": "Code",
            "focused": true,
            "children": []
        }]
    });
    let set_text = UiControlAction {
        action: "set_text".to_owned(),
        text: Some("123456".to_owned()),
        ..action(Some("uia:42.input"), UiControlInputKind::Semantic)
    };

    assert_eq!(
        classify_action(&set_text, Some(&root), None),
        UiControlPolicyTier::HardDeny
    );
    let keypress = UiControlAction {
        action: "keypress".to_owned(),
        keys: vec!["ENTER".to_owned()],
        ..action(None, UiControlInputKind::RawInput)
    };
    assert_eq!(
        classify_action(&keypress, Some(&root), None),
        UiControlPolicyTier::HardDeny
    );
}

#[test]
fn execution_fence_rejects_keyboard_authentication_tier_escalation() {
    let ordinary = RuntimeAccessibilityState {
        root: json!({
            "runtime_id": "42.root",
            "name": "Tools",
            "children": [{
                "runtime_id": "42.input",
                "name": "Code",
                "focused": true,
                "children": []
            }]
        }),
        focus_runtime_id: Some("42.input".to_owned()),
        node_count: 2,
    };
    let keypress = UiControlAction {
        action: "keypress".to_owned(),
        keys: vec!["ENTER".to_owned()],
        ..action(None, UiControlInputKind::RawInput)
    };
    let (policy_tier, controls) = verify_action_fence(
        &keypress,
        &ordinary.root,
        ordinary.focus_runtime_id.as_deref(),
        None,
        &ordinary,
    )
    .unwrap();
    let expected = ActionFenceExpectation {
        controls,
        observation: None,
        max_depth: 12,
        max_nodes: 2_000,
        policy_tier,
    };
    let authentication = RuntimeAccessibilityState {
        root: json!({
            "runtime_id": "42.root",
            "name": "One-Time Code",
            "children": [{
                "runtime_id": "42.input",
                "name": "Code",
                "focused": true,
                "children": []
            }]
        }),
        focus_runtime_id: Some("42.input".to_owned()),
        node_count: 2,
    };

    let error = verify_expected_action_fence(&keypress, &expected, &authentication).unwrap_err();
    assert_eq!(error.code, UiControlHostErrorCode::StaleObservation);
}

#[test]
fn operator_catalog_parser_accepts_valid_and_rejects_duplicate_or_forbidden_grants() {
    let grant = UiControlSystemGrant {
        system_grant_id: "configured".to_owned(),
        dcc_type: "photoshop".to_owned(),
        operations: vec![named_registry_operation("enable-remote", 1)],
    };
    let valid = serde_json::to_vec(std::slice::from_ref(&grant)).unwrap();
    assert_eq!(
        parse_system_grants(&valid)
            .unwrap()
            .get("configured")
            .unwrap(),
        &grant
    );

    let duplicate = serde_json::to_vec(&[grant.clone(), grant.clone()]).unwrap();
    assert!(parse_system_grants(&duplicate).is_err());

    let forbidden = UiControlSystemGrant {
        operations: vec![UiControlSystemGrantOperation {
            operation_id: "persist-plugin".to_owned(),
            operation: UiControlSystemOperation::EnsureRegistryString {
                key: "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce".to_owned(),
                value_name: "Plugin".to_owned(),
                value: "plugin.exe".to_owned(),
            },
        }],
        ..grant
    };
    assert!(parse_system_grants(&serde_json::to_vec(&[forbidden]).unwrap()).is_err());

    let duplicate_operation_ids = UiControlSystemGrant {
        operations: vec![
            named_registry_operation("same-id", 1),
            named_registry_operation("same-id", 2),
        ],
        ..UiControlSystemGrant {
            system_grant_id: "other".to_owned(),
            dcc_type: "maya".to_owned(),
            operations: Vec::new(),
        }
    };
    assert!(parse_system_grants(&serde_json::to_vec(&[duplicate_operation_ids]).unwrap()).is_err());
}
