use super::*;

#[test]
fn owned_standard_menu_popup_is_limited_to_one_navigation_keypress() {
    for keys in ["DOWN", "ARROW_LEFT", "ENTER", "ESCAPE", "TAB", "PGDN"] {
        let navigation = UiControlAction {
            action: "keypress".to_owned(),
            keys: vec![keys.to_owned()],
            input_kind: UiControlInputKind::RawInput,
            ..action(None, UiControlInputKind::RawInput)
        };
        assert!(allows_owned_standard_menu_popup(&navigation), "{keys}");
    }

    for keys in [
        "F5",
        "CTRL+P",
        "CTRL+DOWN",
        "ALT+DOWN",
        "SHIFT+TAB",
        "SHIFT+SHIFT+ENTER",
        "WIN+DOWN",
        "META+DOWN",
        "DOWN+ENTER",
        "A",
    ] {
        let not_navigation = UiControlAction {
            action: "keypress".to_owned(),
            keys: vec![keys.to_owned()],
            input_kind: UiControlInputKind::RawInput,
            ..action(None, UiControlInputKind::RawInput)
        };
        assert!(!allows_owned_standard_menu_popup(&not_navigation), "{keys}");
    }

    let multiple_navigation_keys = UiControlAction {
        action: "keypress".to_owned(),
        keys: vec!["DOWN".to_owned(), "ENTER".to_owned()],
        input_kind: UiControlInputKind::RawInput,
        ..action(None, UiControlInputKind::RawInput)
    };
    assert!(!allows_owned_standard_menu_popup(&multiple_navigation_keys));

    let shortcut = UiControlAction {
        action: "keyboard_shortcut".to_owned(),
        keys: vec!["ALT+W".to_owned()],
        input_kind: UiControlInputKind::RawInput,
        ..action(None, UiControlInputKind::RawInput)
    };
    assert!(!allows_owned_standard_menu_popup(&shortcut));
}

#[test]
fn non_activating_navigation_fences_the_exact_root_across_dynamic_focus_children() {
    let snapshot =
        unity_game_view_accessibility_state("42.game-view.snapshot", "42.game-view.snapshot");
    let live = unity_game_view_accessibility_state("42.game-view.live", "42.play");
    let pre_input = unity_game_view_accessibility_state("42.game-view.live", "42.status");

    for key in ["RIGHT", "ESCAPE"] {
        let navigation = UiControlAction {
            action: "keypress".to_owned(),
            keys: vec![key.to_owned()],
            input_kind: UiControlInputKind::RawInput,
            ..action(None, UiControlInputKind::RawInput)
        };
        let (policy_tier, controls) = verify_action_fence(
            &navigation,
            &snapshot.root,
            snapshot.focus_runtime_id.as_deref(),
            None,
            &live,
        )
        .unwrap();
        assert_eq!(controls.len(), 1, "{key}");
        assert_eq!(controls[0].identity, "42.root", "{key}");
        let expected = ActionFenceExpectation {
            controls,
            observation: None,
            #[cfg(windows)]
            max_depth: 12,
            #[cfg(windows)]
            max_nodes: 2_000,
            policy_tier,
        };
        assert_eq!(
            verify_expected_action_fence(&navigation, &expected, &pre_input).unwrap(),
            UiControlPolicyTier::TaskGrant,
            "{key}"
        );

        let mut replacement = pre_input.clone();
        replacement.root["runtime_id"] = json!("42.other-top-level");
        let failure = verify_expected_action_fence(&navigation, &expected, &replacement)
            .expect_err("navigation must not cross the capability-bound root");
        assert_eq!(failure.code, UiControlHostErrorCode::StaleObservation);
    }
}

fn game_navigation_accessibility_state(
    root_runtime_id: &str,
    focus_runtime_id: &str,
    focus_control_type: &str,
    focus_process_id: u32,
    value: Option<&str>,
) -> RuntimeAccessibilityState {
    RuntimeAccessibilityState {
        root: json!({
            "runtime_id": root_runtime_id,
            "name": "Eclipse Swarm",
            "class_name": "UnityWndClass",
            "control_type": "ControlType.Window",
            "process_id": 42,
            "is_password": false,
            "focused": focus_runtime_id == root_runtime_id,
            "value": null,
            "value_pattern_available": false,
            "text_pattern_available": false,
            "children": [{
                "runtime_id": focus_runtime_id,
                "name": "Game Canvas",
                "class_name": "UnityGUIView",
                "control_type": focus_control_type,
                "process_id": focus_process_id,
                "is_password": false,
                "focused": true,
                "value": value,
                "value_pattern_available": false,
                "text_pattern_available": false,
                "children": []
            }]
        }),
        focus_runtime_id: Some(focus_runtime_id.to_owned()),
        node_count: 2,
    }
}

fn nested_game_navigation_accessibility_state(
    parent_runtime_id: &str,
    parent_control_type: &str,
    focus_runtime_id: &str,
) -> RuntimeAccessibilityState {
    RuntimeAccessibilityState {
        root: json!({
            "runtime_id": "42.game-root",
            "name": "Eclipse Swarm",
            "class_name": "UnityWndClass",
            "control_type": "ControlType.Window",
            "process_id": 42,
            "is_password": false,
            "focused": false,
            "value": null,
            "value_pattern_available": false,
            "text_pattern_available": false,
            "children": [{
                "runtime_id": parent_runtime_id,
                "name": "Runtime Canvas",
                "class_name": "UnityGUIView",
                "control_type": parent_control_type,
                "process_id": 42,
                "is_password": false,
                "focused": false,
                "value": null,
                "value_pattern_available": false,
                "text_pattern_available": false,
                "children": [{
                    "runtime_id": focus_runtime_id,
                    "name": "Game Canvas",
                    "class_name": "UnityGUIView",
                    "control_type": "ControlType.Custom",
                    "process_id": 42,
                    "is_password": false,
                    "focused": true,
                    "value": null,
                    "value_pattern_available": false,
                    "text_pattern_available": false,
                    "children": []
                }]
            }]
        }),
        focus_runtime_id: Some(focus_runtime_id.to_owned()),
        node_count: 3,
    }
}

#[test]
fn game_navigation_has_a_separate_bounded_descriptor() {
    for key in ["W", "a", "S", "d"] {
        let game_navigation = UiControlAction {
            action: "game_navigation".to_owned(),
            input_kind: UiControlInputKind::RawInput,
            keys: vec![key.to_owned()],
            duration_ms: Some(500),
            ..action(None, UiControlInputKind::RawInput)
        };
        validate_action_descriptor(&game_navigation).unwrap();
    }

    for keys in [
        vec![],
        vec!["W", "D"],
        vec!["SHIFT+W"],
        vec!["LEFT"],
        vec![" W"],
    ] {
        let invalid = UiControlAction {
            action: "game_navigation".to_owned(),
            input_kind: UiControlInputKind::RawInput,
            keys: keys.into_iter().map(str::to_owned).collect(),
            ..action(None, UiControlInputKind::RawInput)
        };
        assert!(validate_action_descriptor(&invalid).is_err());
    }

    let overlong = UiControlAction {
        action: "game_navigation".to_owned(),
        input_kind: UiControlInputKind::RawInput,
        keys: vec!["W".to_owned()],
        duration_ms: Some(501),
        ..action(None, UiControlInputKind::RawInput)
    };
    assert!(validate_action_descriptor(&overlong).is_err());

    let printable_keypress = UiControlAction {
        action: "keypress".to_owned(),
        input_kind: UiControlInputKind::RawInput,
        keys: vec!["W".to_owned()],
        ..action(None, UiControlInputKind::RawInput)
    };
    let safe_game = game_navigation_accessibility_state(
        "42.game-root",
        "42.canvas",
        "ControlType.Custom",
        42,
        None,
    );
    assert_eq!(
        classify_action(&printable_keypress, Some(&safe_game.root), None),
        UiControlPolicyTier::HardDeny
    );
}

#[test]
fn game_navigation_rejects_editable_or_unknown_intermediate_ancestors() {
    let game_navigation = UiControlAction {
        action: "game_navigation".to_owned(),
        input_kind: UiControlInputKind::RawInput,
        intent: UiControlIntent::Navigate,
        keys: vec!["W".to_owned()],
        duration_ms: Some(120),
        ..action(None, UiControlInputKind::RawInput)
    };
    let snapshot = nested_game_navigation_accessibility_state(
        "42.runtime.snapshot",
        "ControlType.Pane",
        "42.canvas.snapshot",
    );
    let live = nested_game_navigation_accessibility_state(
        "42.runtime.live",
        "ControlType.Pane",
        "42.canvas.live",
    );
    let (policy_tier, controls) = verify_action_fence(
        &game_navigation,
        &snapshot.root,
        snapshot.focus_runtime_id.as_deref(),
        None,
        &live,
    )
    .expect("a non-editable Pane to Custom runtime canvas chain must remain valid");
    assert_eq!(policy_tier, UiControlPolicyTier::TaskGrant);
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].identity, "42.game-root");

    for parent_control_type in ["ControlType.Edit", "ControlType.Document"] {
        let editable = nested_game_navigation_accessibility_state(
            "42.runtime.editable",
            parent_control_type,
            "42.canvas.editable-parent",
        );
        assert_eq!(
            classify_action(&game_navigation, Some(&editable.root), None),
            UiControlPolicyTier::HardDeny,
            "{parent_control_type} must hard-deny game navigation"
        );
    }

    for field in ["value_pattern_available", "text_pattern_available"] {
        for malformed in [
            None,
            Some(Value::Null),
            Some(json!(true)),
            Some(json!("false")),
        ] {
            let mut unknown = nested_game_navigation_accessibility_state(
                "42.runtime.unknown-pattern",
                "ControlType.Pane",
                "42.canvas.unknown-pattern-parent",
            );
            let parent = unknown.root["children"][0].as_object_mut().unwrap();
            if let Some(value) = malformed {
                parent.insert(field.to_owned(), value);
            } else {
                parent.remove(field);
            }
            assert_eq!(
                classify_action(&game_navigation, Some(&unknown.root), None),
                UiControlPolicyTier::HardDeny,
                "intermediate {field} metadata must be explicit boolean false"
            );
        }
    }
}

#[test]
fn game_navigation_reclassifies_non_editable_focus_and_fences_the_exact_root() {
    let snapshot = game_navigation_accessibility_state(
        "42.game-root",
        "42.canvas.snapshot",
        "ControlType.Custom",
        42,
        None,
    );
    let live = game_navigation_accessibility_state(
        "42.game-root",
        "42.canvas.live",
        "ControlType.Pane",
        42,
        None,
    );
    let pre_input = game_navigation_accessibility_state(
        "42.game-root",
        "42.canvas.pre-input",
        "ControlType.Custom",
        42,
        None,
    );
    let game_navigation = UiControlAction {
        action: "game_navigation".to_owned(),
        input_kind: UiControlInputKind::RawInput,
        intent: UiControlIntent::Navigate,
        keys: vec!["W".to_owned()],
        duration_ms: Some(120),
        ..action(None, UiControlInputKind::RawInput)
    };

    let (policy_tier, controls) = verify_action_fence(
        &game_navigation,
        &snapshot.root,
        snapshot.focus_runtime_id.as_deref(),
        None,
        &live,
    )
    .unwrap();
    assert_eq!(policy_tier, UiControlPolicyTier::TaskGrant);
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].identity, "42.game-root");
    let expected = ActionFenceExpectation {
        controls,
        observation: None,
        #[cfg(windows)]
        max_depth: 12,
        #[cfg(windows)]
        max_nodes: 2_000,
        policy_tier,
    };
    assert_eq!(
        verify_expected_action_fence(&game_navigation, &expected, &pre_input).unwrap(),
        UiControlPolicyTier::TaskGrant
    );

    let mut replacement = pre_input.clone();
    replacement.root["runtime_id"] = json!("42.replacement-root");
    assert_eq!(
        verify_expected_action_fence(&game_navigation, &expected, &replacement)
            .unwrap_err()
            .code,
        UiControlHostErrorCode::StaleObservation
    );

    for unsafe_focus in [
        game_navigation_accessibility_state(
            "42.game-root",
            "42.edit",
            "ControlType.Edit",
            42,
            Some(""),
        ),
        game_navigation_accessibility_state(
            "42.game-root",
            "42.foreign",
            "ControlType.Custom",
            43,
            None,
        ),
    ] {
        assert_eq!(
            classify_action(&game_navigation, Some(&unsafe_focus.root), None),
            UiControlPolicyTier::HardDeny
        );
        assert_eq!(
            verify_expected_action_fence(&game_navigation, &expected, &unsafe_focus)
                .unwrap_err()
                .code,
            UiControlHostErrorCode::HardDenied
        );
    }

    for node in ["root", "focused"] {
        for malformed in [None, Some(json!("false"))] {
            let mut password_state = game_navigation_accessibility_state(
                "42.game-root",
                "42.canvas.password-metadata",
                "ControlType.Custom",
                42,
                None,
            );
            let control = if node == "root" {
                password_state.root.as_object_mut().unwrap()
            } else {
                password_state.root["children"][0].as_object_mut().unwrap()
            };
            if let Some(value) = malformed {
                control.insert("is_password".to_owned(), value);
            } else {
                control.remove("is_password");
            }
            assert_eq!(
                classify_action(&game_navigation, Some(&password_state.root), None),
                UiControlPolicyTier::HardDeny,
                "{node} is_password metadata must be an explicit boolean false"
            );
            assert_eq!(
                verify_expected_action_fence(&game_navigation, &expected, &password_state)
                    .unwrap_err()
                    .code,
                UiControlHostErrorCode::HardDenied,
                "{node} is_password metadata must fail closed"
            );
        }
    }

    let mut missing_value = game_navigation_accessibility_state(
        "42.game-root",
        "42.canvas.missing-value",
        "ControlType.Custom",
        42,
        None,
    );
    missing_value.root["children"][0]
        .as_object_mut()
        .unwrap()
        .remove("value");
    assert_eq!(
        classify_action(&game_navigation, Some(&missing_value.root), None),
        UiControlPolicyTier::HardDeny
    );

    for (field, value) in [
        ("value_pattern_available", json!(true)),
        ("text_pattern_available", json!(true)),
        ("value_pattern_available", Value::Null),
        ("text_pattern_available", Value::Null),
    ] {
        let mut pattern_target = game_navigation_accessibility_state(
            "42.game-root",
            "42.canvas.pattern",
            "ControlType.Custom",
            42,
            None,
        );
        pattern_target.root["children"][0][field] = value;
        assert_eq!(
            classify_action(&game_navigation, Some(&pattern_target.root), None),
            UiControlPolicyTier::HardDeny,
            "{field} must be explicitly false"
        );
    }
}

#[test]
fn game_navigation_runs_through_the_host_with_a_root_focused_game_window() {
    fn root_focused_state(root_runtime_id: &str) -> RuntimeAccessibilityState {
        RuntimeAccessibilityState {
            root: json!({
                "runtime_id": root_runtime_id,
                "name": "Eclipse Swarm",
                "class_name": "UnityWndClass",
                "control_type": "ControlType.Window",
                "process_id": 42,
                "is_password": false,
                "focused": true,
                "value": null,
                "value_pattern_available": false,
                "text_pattern_available": false,
                "children": []
            }),
            focus_runtime_id: Some(root_runtime_id.to_owned()),
            node_count: 1,
        }
    }

    let snapshot = root_focused_state("42.game-root");
    let host_refresh = root_focused_state("42.game-root");
    let pre_input = root_focused_state("42.game-root");
    let mut host = host_with_accessibility_states(snapshot, vec![host_refresh, pre_input]);
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
            session_id: "packaged-game".to_owned(),
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
            session_id: "packaged-game".to_owned(),
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
            session_id: "packaged-game".to_owned(),
            task_grant_id: "grant-1".to_owned(),
            window_capability,
            observation_id,
            accessibility_state_id,
            action: Box::new(UiControlAction {
                action: "game_navigation".to_owned(),
                input_kind: UiControlInputKind::RawInput,
                intent: UiControlIntent::Navigate,
                keys: vec!["W".to_owned()],
                duration_ms: Some(120),
                ..action(None, UiControlInputKind::RawInput)
            }),
        },
    );

    assert!(matches!(
        response,
        UiControlHostResponse::ActionCompleted {
            success: true,
            policy_tier: UiControlPolicyTier::TaskGrant,
            ..
        }
    ));
}
