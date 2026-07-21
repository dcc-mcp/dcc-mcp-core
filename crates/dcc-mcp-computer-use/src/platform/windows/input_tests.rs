use super::*;

#[test]
fn active_ui_control_reserves_ctrl_alt_escape_as_the_stop_key() {
    assert_eq!(STOP_HOTKEY_LABEL, "Esc");
    assert_eq!(
        STOP_HOTKEY_MODIFIERS.0,
        0
    );
}

#[test]
fn pre_input_fence_can_guard_preparatory_and_mutating_input() {
    let mut calls = 0;
    let error = {
        let mut callback = || {
            calls += 1;
            if calls == 1 {
                Ok(())
            } else {
                Err(ComputerUseError::new(
                    ComputerUseErrorCode::StaleObservation,
                    "changed target",
                ))
            }
        };
        let mut fence: Option<&mut PreInputFence<'_>> = Some(&mut callback);

        run_pre_input_fence(&mut fence).unwrap();
        run_pre_input_fence(&mut fence).unwrap_err()
    };

    assert_eq!(calls, 2);
    assert_eq!(error.code, ComputerUseErrorCode::StaleObservation);
}

#[test]
fn non_interactive_desktop_is_reported_explicitly() {
    let error = require_interactive_desktop(false).unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::DesktopUnavailable);
    assert!(error.message.contains("locked"));
}

#[test]
fn remote_and_lock_session_events_suspend_until_reconnect_or_unlock() {
    assert_eq!(session_event_blocked(WTS_REMOTE_DISCONNECT), Some(true));
    assert_eq!(session_event_blocked(WTS_CONSOLE_DISCONNECT), Some(true));
    assert_eq!(session_event_blocked(WTS_SESSION_LOCK), Some(true));
    assert_eq!(session_event_blocked(WTS_REMOTE_CONNECT), Some(false));
    assert_eq!(session_event_blocked(WTS_CONSOLE_CONNECT), Some(false));
    assert_eq!(session_event_blocked(WTS_SESSION_UNLOCK), Some(false));
    assert_eq!(session_event_blocked(0xffff), None);
}

#[test]
fn action_guard_rejects_mid_action_desktop_changes() {
    let stop = Arc::new(AtomicBool::new(false));
    let desktop_state = Arc::new(AtomicU64::new(crate::desktop_state_value(7, true)));
    let desktop_barrier = Arc::new(DesktopEventBarrier::default());
    let guard = ActionGuard::new(&stop, &desktop_state, &desktop_barrier, 7);

    assert!(guard.check().is_ok());
    record_desktop_environment_change(&desktop_state);
    assert_eq!(
        guard.check().unwrap_err().code,
        ComputerUseErrorCode::StaleObservation
    );

    let locked_guard = ActionGuard::new(&stop, &desktop_state, &desktop_barrier, 8);
    record_desktop_transition(&desktop_state, false);
    assert_eq!(
        locked_guard.check().unwrap_err().code,
        ComputerUseErrorCode::DesktopUnavailable
    );
}

#[test]
fn click_batch_contains_button_down_and_release_together() {
    let inputs = click_inputs("left").unwrap();

    assert_eq!(inputs.len(), 2);
    assert_eq!(
        unsafe { inputs[0].Anonymous.mi.dwFlags },
        MOUSEEVENTF_LEFTDOWN
    );
    assert_eq!(
        unsafe { inputs[1].Anonymous.mi.dwFlags },
        MOUSEEVENTF_LEFTUP
    );
}

#[test]
fn partial_click_injection_compensates_with_button_release() {
    let inputs = click_inputs("left").unwrap();
    let mut calls = 0;
    let mut cleanup_flags = MOUSE_EVENT_FLAGS(0);

    let error = send_inputs_with(
        &inputs,
        |batch| {
            calls += 1;
            if calls == 1 {
                1
            } else {
                cleanup_flags = unsafe { batch[0].Anonymous.mi.dwFlags };
                batch.len() as u32
            }
        },
        || true,
        |_| {},
    )
    .unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::InputFailed);
    assert_eq!(calls, 2);
    assert_eq!(cleanup_flags, MOUSEEVENTF_LEFTUP);
    assert!(error.message.contains("compensating release"));
}

#[test]
fn partial_keypress_cleanup_retries_only_remaining_releases_in_reverse_order() {
    let inputs = keypress_inputs(&["CTRL+A".to_string()]).unwrap();
    let mut calls = 0;
    let mut cleanup_keys = Vec::new();

    let error = send_inputs_with(
        &inputs,
        |batch| {
            calls += 1;
            match calls {
                1 => 2,
                2 => {
                    cleanup_keys.extend(batch.iter().map(|input| unsafe {
                        let keyboard = input.Anonymous.ki;
                        (keyboard.wVk.0, keyboard.dwFlags)
                    }));
                    1
                }
                _ => {
                    cleanup_keys.extend(batch.iter().map(|input| unsafe {
                        let keyboard = input.Anonymous.ki;
                        (keyboard.wVk.0, keyboard.dwFlags)
                    }));
                    batch.len() as u32
                }
            }
        },
        || true,
        |_| {},
    )
    .unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::InputFailed);
    assert_eq!(calls, 3);
    assert_eq!(cleanup_keys.len(), 3);
    assert_eq!(cleanup_keys[0].0, b'A' as u16);
    assert_eq!(cleanup_keys[1].0, 0x11);
    assert_eq!(cleanup_keys[2].0, 0x11);
    assert!(
        cleanup_keys
            .iter()
            .all(|(_, flags)| flags.contains(KEYEVENTF_KEYUP))
    );
}

#[test]
fn desktop_loss_after_partial_input_takes_error_priority() {
    let inputs = click_inputs("left").unwrap();
    let mut desktop_checks = 0;
    let mut injection_calls = 0;
    let mut deferred = Vec::new();

    let error = send_inputs_with(
        &inputs,
        |_batch| {
            injection_calls += 1;
            1
        },
        || {
            desktop_checks += 1;
            desktop_checks == 1
        },
        |releases| deferred.extend_from_slice(releases),
    )
    .unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::DesktopUnavailable);
    assert_eq!(injection_calls, 1);
    assert_eq!(deferred.len(), 1);
    assert_eq!(
        unsafe { deferred[0].Anonymous.mi.dwFlags },
        MOUSEEVENTF_LEFTUP
    );
}

#[test]
fn pending_release_fence_blocks_until_every_release_is_confirmed() {
    let inputs = keypress_inputs(&["CTRL+A".to_string()]).unwrap();
    let mut pending = compensating_releases(&inputs[..2]);
    let mut calls = 0;

    let error = flush_pending_input_releases_with(
        &mut pending,
        |_batch| {
            calls += 1;
            u32::from(calls == 1)
        },
        || true,
    )
    .unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::InputFailed);
    assert_eq!(pending.len(), 1);
    assert_eq!(unsafe { pending[0].Anonymous.ki.wVk.0 }, 0x11);

    flush_pending_input_releases_with(&mut pending, |batch| batch.len() as u32, || true).unwrap();
    assert!(pending.is_empty());
}

#[test]
fn keypress_batch_releases_every_pressed_key_in_reverse_order() {
    let inputs = keypress_inputs(&["CTRL+A".to_string()]).unwrap();

    assert_eq!(inputs.len(), 4);
    let key = |index: usize| unsafe { inputs[index].Anonymous.ki };
    assert_eq!(key(0).wVk, VIRTUAL_KEY(0x11));
    assert_eq!(key(1).wVk, VIRTUAL_KEY(b'A' as u16));
    assert_eq!(key(2).wVk, VIRTUAL_KEY(b'A' as u16));
    assert_eq!(key(3).wVk, VIRTUAL_KEY(0x11));
    assert_eq!(key(0).dwFlags, KEYBD_EVENT_FLAGS(0));
    assert_eq!(key(3).dwFlags, KEYEVENTF_KEYUP);
}

#[test]
fn pointer_modifiers_release_every_held_key_in_reverse_order() {
    let (presses, releases) = pointer_modifier_inputs(&["CTRL+SHIFT+ALT".to_string()]).unwrap();
    let key = |input: &INPUT| unsafe { input.Anonymous.ki };

    assert_eq!(
        presses
            .iter()
            .map(|input| key(input).wVk.0)
            .collect::<Vec<_>>(),
        [0x11, 0x10, 0x12]
    );
    assert_eq!(
        releases
            .iter()
            .map(|input| key(input).wVk.0)
            .collect::<Vec<_>>(),
        [0x12, 0x10, 0x11]
    );
    assert!(
        releases
            .iter()
            .all(|input| key(input).dwFlags.contains(KEYEVENTF_KEYUP))
    );
}

#[test]
fn keypress_rejects_canonical_and_aliased_scope_escape_chords() {
    for chord in [
        "CONTROL+SHIFT+ESCAPE",
        "ALT+TAB",
        "ALT+SPACE",
        "CONTROL+ESCAPE",
        "WIN+R",
        "PRINT_SCREEN",
    ] {
        let error = match keypress_inputs(&[chord.to_owned()]) {
            Ok(_) => panic!("{chord} must not be accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code, ComputerUseErrorCode::InvalidAction, "{chord}");
        assert!(error.message.contains("scope-escape"), "{chord}");
    }
}

#[test]
fn host_confirmed_close_shortcuts_remain_buildable() {
    for chord in ["CTRL+W", "CONTROL+F04", "CTRL+Q"] {
        assert!(keypress_inputs(&[chord.to_owned()]).is_ok(), "{chord}");
    }
}

#[test]
fn keypress_cannot_bypass_the_raw_text_entry_denial() {
    for chord in [
        "A",
        "1",
        "SHIFT+A",
        "SHIFT+SEMICOLON",
        "SPACE",
        "KP_1",
        "A+CTRL",
        "A+ALT",
        "ALTGR+Q",
        "RIGHTALT+Q",
    ] {
        let error = match keypress_inputs(&[chord.to_owned()]) {
            Ok(_) => panic!("{chord} must not be accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code, ComputerUseErrorCode::InvalidAction, "{chord}");
        assert!(error.message.contains("ordinary text"), "{chord}");
    }
    for chord in ["LEFT", "ENTER", "F5", "CTRL+A", "ALT+A"] {
        assert!(keypress_inputs(&[chord.to_owned()]).is_ok(), "{chord}");
    }
}

#[test]
fn pointer_keys_reject_non_modifiers_and_system_keys() {
    for key in ["A", "LEFT"] {
        let error = match pointer_modifier_inputs(&[key.to_string()]) {
            Ok(_) => panic!("{key} must not be accepted as a pointer modifier"),
            Err(error) => error,
        };
        assert_eq!(error.code, ComputerUseErrorCode::InvalidAction);
        assert!(error.message.contains("only allow Ctrl, Shift, Alt"));
    }

    let system_key = match pointer_modifier_inputs(&["WIN".to_string()]) {
        Ok(_) => panic!("WIN must remain unavailable to pointer actions"),
        Err(error) => error,
    };
    assert_eq!(system_key.code, ComputerUseErrorCode::InvalidAction);
    assert!(system_key.message.contains("system key"));
}

#[test]
fn navigation_and_right_modifiers_use_extended_key_events() {
    let inputs = keypress_inputs(&["RCTRL+LEFT".to_string()]).unwrap();
    let key = |index: usize| unsafe { inputs[index].Anonymous.ki };

    assert!(key(0).dwFlags.contains(KEYEVENTF_EXTENDEDKEY));
    assert!(key(1).dwFlags.contains(KEYEVENTF_EXTENDEDKEY));
    assert!(key(2).dwFlags.contains(KEYEVENTF_EXTENDEDKEY));
    assert!(key(2).dwFlags.contains(KEYEVENTF_KEYUP));
    assert!(key(3).dwFlags.contains(KEYEVENTF_EXTENDEDKEY));
    assert!(key(3).dwFlags.contains(KEYEVENTF_KEYUP));
}

#[test]
fn drag_duration_interpolates_visible_steps_along_the_path() {
    let path = [
        ComputerUsePoint { x: 0.0, y: 0.0 },
        ComputerUsePoint { x: 96.0, y: 48.0 },
    ];
    let steps = drag_step_count(path.len(), 48);
    let interpolated = interpolated_drag_path(&path, 48);

    assert_eq!(steps, 3);
    assert_eq!(interpolated[0], ComputerUsePoint { x: 32.0, y: 16.0 });
    assert_eq!(interpolated[1], ComputerUsePoint { x: 64.0, y: 32.0 });
    assert_eq!(interpolated[2], path[1]);

    let cornered = [
        ComputerUsePoint { x: 0.0, y: 0.0 },
        ComputerUsePoint { x: 10.0, y: 0.0 },
        ComputerUsePoint { x: 10.0, y: 10.0 },
    ];
    assert_eq!(drag_step_count(cornered.len(), 0), 2);
    assert_eq!(interpolated_drag_path(&cornered, 0), &cornered[1..]);
    let timed_corner = interpolated_drag_path(&cornered, 48);
    assert!(timed_corner.contains(&cornered[1]));
    assert_eq!(timed_corner.last(), Some(&cornered[2]));
}

#[test]
fn drag_preflight_rejects_a_late_out_of_bounds_point_before_input() {
    let observation = ComputerUseObservation {
        observation_id: "drag:1".to_string(),
        window_handle: 1,
        process_id: 2,
        window_title: "Maya".to_string(),
        width: 100,
        height: 100,
        source_rect: [0, 0, 100, 100],
        dpi_scale: 1.0,
        window_dpi: 96,
        capture_backend: "test".to_string(),
        timestamp_ms: 0,
        desktop_generation: 1,
        session_id: None,
    };
    let path = [
        ComputerUsePoint { x: 10.0, y: 50.0 },
        ComputerUsePoint { x: 100.0, y: 50.0 },
    ];

    let error =
        preflight_drag_path_for_desktop(&observation, &path, 64, [0, 0, 1920, 1080]).unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::InvalidAction);
    assert!(error.message.contains("outside the latest screenshot"));
}

#[test]
fn escape_remains_available_to_cancel_dcc_tools() {
    assert_eq!(virtual_key("escape").unwrap(), VK_ESCAPE);
}

#[test]
fn system_keys_are_rejected_before_input_is_built() {
    for key in ["Win", "Meta", "Windows", "Super", "Cmd", "Command"] {
        let error = match keypress_inputs(&[format!("{key}+LEFT")]) {
            Ok(_) => panic!("{key} must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code, ComputerUseErrorCode::InvalidAction);
        assert!(error.message.contains("not allowed"));
    }
}

#[test]
fn key_chords_accept_common_keysyms_and_function_keys() {
    assert_eq!(virtual_key("ctrl").unwrap(), VIRTUAL_KEY(0x11));
    assert_eq!(virtual_key("ctrl_l").unwrap(), VIRTUAL_KEY(0xA2));
    assert_eq!(virtual_key("right_alt").unwrap(), VIRTUAL_KEY(0xA5));
    assert_eq!(virtual_key("F12").unwrap(), VIRTUAL_KEY(0x7B));
    assert_eq!(virtual_key(".").unwrap(), VIRTUAL_KEY(0xBE));
    assert_eq!(virtual_key("semicolon").unwrap(), VIRTUAL_KEY(0xBA));
    assert_eq!(virtual_key("KP_0").unwrap(), VIRTUAL_KEY(0x60));
    assert_eq!(virtual_key("KP_9").unwrap(), VIRTUAL_KEY(0x69));
    assert_eq!(virtual_key("KP_Decimal").unwrap(), VIRTUAL_KEY(0x6E));
}

#[test]
fn focusing_must_not_change_the_observed_window_rect() {
    let observation = ComputerUseObservation {
        observation_id: "window:1".to_string(),
        window_handle: 1,
        process_id: 2,
        window_title: "Godot".to_string(),
        width: 800,
        height: 600,
        source_rect: [10, 20, 800, 600],
        dpi_scale: 1.0,
        window_dpi: 144,
        capture_backend: "test".to_string(),
        timestamp_ms: 0,
        desktop_generation: 1,
        session_id: None,
    };

    assert!(ensure_observation_rect(&observation, [10, 20, 800, 600]).is_ok());
    let error = ensure_observation_rect(&observation, [10, 20, 640, 480]).unwrap_err();
    assert_eq!(error.code, ComputerUseErrorCode::StaleObservation);
}

#[test]
fn focusing_must_not_change_the_observed_window_dpi() {
    let observation = ComputerUseObservation {
        observation_id: "window:1".to_string(),
        window_handle: 1,
        process_id: 2,
        window_title: "Godot".to_string(),
        width: 800,
        height: 600,
        source_rect: [10, 20, 800, 600],
        dpi_scale: 1.5,
        window_dpi: 144,
        capture_backend: "test".to_string(),
        timestamp_ms: 0,
        desktop_generation: 1,
        session_id: None,
    };

    assert!(ensure_observation_target_state(&observation, [10, 20, 800, 600], 144).is_ok());
    let error = ensure_observation_target_state(&observation, [10, 20, 800, 600], 192).unwrap_err();
    assert_eq!(error.code, ComputerUseErrorCode::StaleObservation);
    assert!(error.message.contains("DPI"));
}

#[test]
fn control_corners_mark_the_scoped_target_without_covering_its_edges() {
    let rect = windows::Win32::Foundation::RECT {
        left: -100,
        top: 20,
        right: 900,
        bottom: 620,
    };

    let geometries = corner_geometries(&rect, 96);
    assert_eq!(geometries.len(), 24);
    assert_eq!(
        &geometries[..8],
        &[
            ((-100, 20, 232, 42), 48, true),
            ((-100, 20, 42, 232), 48, true),
            ((668, 20, 232, 42), 48, true),
            ((858, 20, 42, 232), 48, true),
            ((-100, 578, 232, 42), 48, true),
            ((-100, 388, 42, 232), 48, true),
            ((668, 578, 232, 42), 48, true),
            ((858, 388, 42, 232), 48, true),
        ]
    );
    assert_eq!(
        &geometries[8..16],
        &[
            ((-100, 20, 208, 28), 92, true),
            ((-100, 20, 28, 208), 92, true),
            ((692, 20, 208, 28), 92, true),
            ((872, 20, 28, 208), 92, true),
            ((-100, 592, 208, 28), 92, true),
            ((-100, 412, 28, 208), 92, true),
            ((692, 592, 208, 28), 92, true),
            ((872, 412, 28, 208), 92, true),
        ]
    );
    assert_eq!(
        &geometries[16..],
        &[
            ((-100, 20, 180, 12), CONTROL_BORDER_ALPHA, false),
            ((-100, 20, 12, 180), CONTROL_BORDER_ALPHA, false),
            ((720, 20, 180, 12), CONTROL_BORDER_ALPHA, false),
            ((888, 20, 12, 180), CONTROL_BORDER_ALPHA, false),
            ((-100, 608, 180, 12), CONTROL_BORDER_ALPHA, false),
            ((-100, 440, 12, 180), CONTROL_BORDER_ALPHA, false),
            ((720, 608, 180, 12), CONTROL_BORDER_ALPHA, false),
            ((888, 440, 12, 180), CONTROL_BORDER_ALPHA, false),
        ]
    );
}

#[test]
fn overlay_pixels_scale_at_common_monitor_dpis() {
    assert_eq!(scaled_pixels(36, 96), 36);
    assert_eq!(scaled_pixels(36, 144), 54);
    assert_eq!(scaled_pixels(36, 192), 72);
    assert_eq!(scaled_pixels(CORNER_GLOW_THICKNESS, 96), 42);
    assert_eq!(scaled_pixels(CORNER_GLOW_THICKNESS, 144), 63);
    assert_eq!(scaled_pixels(CORNER_GLOW_THICKNESS, 192), 84);
    assert_eq!(scaled_pixels(CORNER_MID_THICKNESS, 96), 28);
    assert_eq!(scaled_pixels(CORNER_MID_THICKNESS, 144), 42);
    assert_eq!(scaled_pixels(CORNER_MID_THICKNESS, 192), 56);
    assert_eq!(scaled_pixels(POINTER_EFFECT_SIZE, 96), 72);
    assert_eq!(scaled_pixels(POINTER_EFFECT_SIZE, 144), 108);
    assert_eq!(scaled_pixels(POINTER_EFFECT_SIZE, 192), 144);
    assert_eq!(scaled_pixels(POINTER_RING_SIZE, 96), 52);
    assert_eq!(scaled_pixels(POINTER_RING_SIZE, 144), 78);
    assert_eq!(scaled_pixels(POINTER_RING_SIZE, 192), 104);
}

#[test]
fn control_overlay_breathes_smoothly_without_exceeding_base_alpha() {
    let minimum = breathing_alpha(CONTROL_BORDER_ALPHA, CONTROL_BORDER_PULSE_FLOOR_PERCENT, 0);
    let quarter = breathing_alpha(
        CONTROL_BORDER_ALPHA,
        CONTROL_BORDER_PULSE_FLOOR_PERCENT,
        CONTROL_PULSE_PERIOD_MS / 4,
    );
    let maximum = breathing_alpha(
        CONTROL_BORDER_ALPHA,
        CONTROL_BORDER_PULSE_FLOOR_PERCENT,
        CONTROL_PULSE_PERIOD_MS / 2,
    );

    assert_eq!(minimum, 204);
    assert!(minimum < quarter && quarter < maximum);
    assert_eq!(maximum, CONTROL_BORDER_ALPHA);
    assert_eq!(
        breathing_alpha(
            CONTROL_BORDER_ALPHA,
            CONTROL_BORDER_PULSE_FLOOR_PERCENT,
            CONTROL_PULSE_PERIOD_MS,
        ),
        minimum
    );
}

#[test]
fn control_overlay_visual_contract_is_prominent_blue() {
    let red = CONTROL_ACCENT_COLOR.0 & 0xff;
    let green = (CONTROL_ACCENT_COLOR.0 >> 8) & 0xff;
    let blue = (CONTROL_ACCENT_COLOR.0 >> 16) & 0xff;
    let glow_red = CONTROL_GLOW_COLOR.0 & 0xff;
    let glow_green = (CONTROL_GLOW_COLOR.0 >> 8) & 0xff;
    let glow_blue = (CONTROL_GLOW_COLOR.0 >> 16) & 0xff;
    let cursor_red = CONTROL_CURSOR_COLOR.0 & 0xff;
    let cursor_green = (CONTROL_CURSOR_COLOR.0 >> 8) & 0xff;
    let cursor_blue = (CONTROL_CURSOR_COLOR.0 >> 16) & 0xff;

    const {
        assert!(CORNER_ACCENT_THICKNESS >= 10);
        assert!(CORNER_ACCENT_LENGTH >= 180);
        assert!(POINTER_EFFECT_SIZE >= 64 && POINTER_EFFECT_SIZE <= 96);
        assert!(POINTER_RING_SIZE >= 40 && POINTER_RING_SIZE <= 64);
        assert!(CONTROL_CAPSULE_ALPHA > CONTROL_OVERLAY_ALPHA);
        assert!(CONTROL_BORDER_ALPHA >= CONTROL_CURSOR_ALPHA);
        assert!(CONTROL_BORDER_ALPHA >= 224);
        assert!(CONTROL_BORDER_PULSE_FLOOR_PERCENT >= 85);
        assert!(CONTROL_CAPSULE_PULSE_FLOOR_PERCENT >= 92);
        assert!(CONTROL_CAPSULE_FONT_SIZE >= 14 && CONTROL_CAPSULE_FONT_SIZE <= 18);
    }
    assert!((110..=220).contains(&CONTROL_OVERLAY_ALPHA));
    assert!(blue > green && green > red);
    assert!(glow_blue > glow_green && glow_green > glow_red);
    assert!(cursor_red > cursor_green && cursor_green > cursor_blue);
}

#[test]
fn capsule_stays_top_center_on_a_negative_origin_monitor() {
    let target = windows::Win32::Foundation::RECT {
        left: -1900,
        top: 20,
        right: -900,
        bottom: 620,
    };
    let display = windows::Win32::Foundation::RECT {
        left: -1920,
        top: 0,
        right: 0,
        bottom: 1040,
    };

    assert_eq!(
        capsule_geometry(&target, &display, 96),
        (-1640, 38, 480, 44)
    );
    assert_eq!(
        capsule_glow_geometries((-1640, 38, 480, 44)),
        [
            ((-1652, 26, 504, 68), 44),
            ((-1648, 30, 496, 60), 78),
            ((-1644, 34, 488, 52), 118),
        ]
    );
}

#[test]
fn capsule_clamps_partial_offscreen_targets_to_monitor_work_area() {
    let target = windows::Win32::Foundation::RECT {
        left: -2100,
        top: -100,
        right: -1700,
        bottom: 200,
    };
    let display = windows::Win32::Foundation::RECT {
        left: -1920,
        top: 0,
        right: 0,
        bottom: 1040,
    };

    assert_eq!(
        capsule_geometry(&target, &display, 144),
        (-1920, 0, 720, 66)
    );
}

#[test]
fn capsule_clamps_cross_gap_targets_to_the_selected_real_monitor() {
    let target = windows::Win32::Foundation::RECT {
        left: 1800,
        top: 150,
        right: 3000,
        bottom: 900,
    };
    let display = windows::Win32::Foundation::RECT {
        left: 2560,
        top: 200,
        right: 4480,
        bottom: 1280,
    };

    assert_eq!(
        capsule_geometry(&target, &display, 96),
        (2560, 200, 480, 44)
    );
}

#[test]
fn screenshot_coordinates_floor_and_clamp_inside_negative_desktop_rect() {
    let rect = [-1920, -100, 101, 51];

    assert_eq!(
        screenshot_point_to_screen(ComputerUsePoint { x: 0.0, y: 0.0 }, [100, 50], rect,),
        Some((-1920, -100))
    );
    assert_eq!(
        screenshot_point_to_screen(
            ComputerUsePoint {
                x: 99.999,
                y: 49.999,
            },
            [100, 50],
            rect,
        ),
        Some((-1820, -50))
    );
}

#[test]
fn screenshot_coordinates_reject_right_and_bottom_edges() {
    assert_eq!(
        screenshot_point_to_screen(
            ComputerUsePoint { x: 100.0, y: 0.0 },
            [100, 50],
            [10, 20, 100, 50],
        ),
        None
    );
    assert_eq!(
        screenshot_point_to_screen(
            ComputerUsePoint { x: 0.0, y: 50.0 },
            [100, 50],
            [10, 20, 100, 50],
        ),
        None
    );
}

#[test]
fn pointer_effect_dwell_defaults_and_clamps_overrides() {
    let action = |duration_ms| ComputerUseAction {
        action: "move".to_string(),
        observation_id: None,
        x: Some(0.0),
        y: Some(0.0),
        button: None,
        scroll_x: None,
        scroll_y: None,
        path: Vec::new(),
        text: None,
        keys: Vec::new(),
        duration_ms,
    };

    assert_eq!(
        pointer_effect_dwell(&action(None)),
        Duration::from_millis(350)
    );
    assert_eq!(
        pointer_effect_dwell(&action(Some(1))),
        Duration::from_millis(100)
    );
    assert_eq!(
        pointer_effect_dwell(&action(Some(5_000))),
        Duration::from_millis(2_000)
    );
}

#[test]
fn vertical_scroll_handles_minimum_i32_without_overflow() {
    assert_eq!(vertical_wheel_data(i32::MIN), i32::MAX as u32);
    assert_eq!(vertical_wheel_data(120), (-120_i32) as u32);
}

#[test]
fn pointer_effect_is_a_visible_click_through_overlay() {
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowDisplayAffinity, GetWindowLongPtrW, GetWindowTextW, IsWindowVisible,
        WDA_EXCLUDEFROMCAPTURE,
    };

    let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
    let effect = PointerEffect::new(200, 240, "●").unwrap();

    assert!(unsafe { IsWindowVisible(effect.hwnd) }.as_bool());

    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe { GetWindowRect(effect.hwnd, &mut rect) }.unwrap();
    let (_, _, expected_size, _) = pointer_mask_geometry(200, 240);
    assert_eq!(rect.right - rect.left, expected_size);
    assert_eq!(rect.bottom - rect.top, expected_size);

    let ex_style = unsafe { GetWindowLongPtrW(effect.hwnd, GWL_EXSTYLE) } as u32;
    for required in [WS_EX_NOACTIVATE.0, WS_EX_TRANSPARENT.0, WS_EX_TOPMOST.0] {
        assert_eq!(ex_style & required, required);
    }

    let mut affinity = 0;
    unsafe { GetWindowDisplayAffinity(effect.hwnd, &mut affinity) }.unwrap();
    assert_eq!(affinity, WDA_EXCLUDEFROMCAPTURE.0);

    let mut caption = [0_u16; 8];
    let length = unsafe { GetWindowTextW(effect.hwnd, &mut caption) } as usize;
    assert_eq!(String::from_utf16_lossy(&caption[..length]), "●");
}

#[test]
fn persistent_pointer_ring_keeps_the_system_cursor_visible() {
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, GetWindowRgn, PtInRegion};

    let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
    let geometry = pointer_ring_geometry(200, 240);
    let hwnd = create_cursor_ring_overlay(geometry, CONTROL_CURSOR_ALPHA, None).unwrap();
    let region = unsafe { CreateRectRgn(0, 0, geometry.2, geometry.3) };

    assert_ne!(unsafe { GetWindowRgn(hwnd, region) }, RGN_ERROR);
    let center = geometry.2 / 2;
    assert!(!unsafe { PtInRegion(region, center, center) }.as_bool());
    assert!(unsafe { PtInRegion(region, center, (geometry.2 / 24).max(1)) }.as_bool());

    let _ = unsafe { DeleteObject(HGDIOBJ(region.0)) };
    unsafe { DestroyWindow(hwnd) }.unwrap();
}

#[test]
fn persistent_overlay_layers_stay_hidden_until_their_geometry_is_ready() {
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
    let hwnd = create_color_overlay("", (80, 90, 160, 24), 42, false, OverlayTone::Glow, None).unwrap();
    assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());

    set_overlay_visible(hwnd, true).unwrap();
    assert!(unsafe { IsWindowVisible(hwnd) }.as_bool());
    unsafe { DestroyWindow(hwnd) }.unwrap();
}

#[test]
fn overlay_classes_are_revalidated_for_each_window_creation() {
    register_color_overlay_classes().unwrap();
    let instance = unsafe { GetModuleHandleW(None) }.unwrap();
    let mut class = WNDCLASSW::default();

    unsafe { GetClassInfoW(Some(instance.into()), CONTROL_OVERLAY_CLASS, &raw mut class) }.unwrap();

    let hwnd = create_color_overlay("", (80, 90, 160, 24), 42, false, OverlayTone::Accent, None).unwrap();
    assert!(unsafe { IsWindow(Some(hwnd)) }.as_bool());
    unsafe { DestroyWindow(hwnd) }.unwrap();
}

#[test]
fn visual_overlay_verification_fails_closed_for_a_destroyed_window() {
    let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
    let effect = PointerEffect::new(200, 240, "●").unwrap();
    unsafe { DestroyWindow(effect.hwnd) }.unwrap();

    let error = set_overlay_visible(effect.hwnd, true).unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::BackendUnavailable);
    assert!(error.message.contains("visual overlay"));
}

#[test]
fn large_negative_virtual_desktop_coordinates_are_valid_geometry() {
    let rect = windows::Win32::Foundation::RECT {
        left: -30_720,
        top: -2_160,
        right: -26_880,
        bottom: 0,
    };

    assert!(rect_has_positive_area(&rect));
}

#[test]
fn escape_latch_blocks_new_sessions_until_explicit_reset() {
    let _interrupt_guard = crate::user_interrupt_test_guard();
    clear_user_interrupt().unwrap();
    set_user_interrupt();

    let error = start_control_banner(
        0,
        0,
        "test".to_string(),
        ControlBannerSignals {
            stop: Arc::new(AtomicBool::new(false)),
            interrupted: Arc::new(AtomicBool::new(false)),
            visible: Arc::new(AtomicBool::new(false)),
            desktop_state: Arc::new(AtomicU64::new(crate::desktop_state_value(1, true))),
            desktop_barrier: Arc::new(DesktopEventBarrier::default()),
            target_available: Arc::new(AtomicBool::new(false)),
            cleanup_pending: Arc::new(AtomicBool::new(false)),
            session_id: None,
            last_action_point: Arc::new(std::sync::Mutex::new(None)),
        },
    )
    .unwrap_err();
    assert_eq!(error.error.code, ComputerUseErrorCode::UserInterrupted);

    clear_user_interrupt().unwrap();
    assert!(!user_interrupted());
}

#[test]
fn interrupt_reset_requires_the_input_owner_to_be_idle() {
    let _interrupt_guard = crate::user_interrupt_test_guard();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let owner_thread = std::thread::spawn(move || {
        let _owner = acquire_input_owner().unwrap();
        ready_tx.send(()).unwrap();
        let _ = release_rx.recv();
    });
    ready_rx.recv().unwrap();

    let error = clear_user_interrupt().unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);
    release_tx.send(()).unwrap();
    owner_thread.join().unwrap();
    clear_user_interrupt().unwrap();
}

#[test]
fn input_owner_is_held_until_local_input_drains() {
    let input_guard = INPUT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let name = format!(
        "Local\\DccMcpComputerUseInputDrain-test-{}",
        std::process::id()
    );
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_name = name.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
    let (finished_tx, finished_rx) = std::sync::mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        let owner = match try_acquire_named_mutex(&worker_name).unwrap() {
            NamedMutexAcquisition::Acquired(owner) => owner,
            _ => panic!("test input owner should be available"),
        };
        ready_tx.send(()).unwrap();
        drop(InputOwnerLease::new(owner, worker_stop));
        finished_tx.send(()).unwrap();
    });
    ready_rx.recv().unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while !stop.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(stop.load(Ordering::Acquire));
    assert!(matches!(
        try_acquire_named_mutex(&name).unwrap(),
        NamedMutexAcquisition::Busy
    ));
    assert!(matches!(
        finished_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));

    drop(input_guard);
    finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    worker.join().unwrap();
    assert!(matches!(
        try_acquire_named_mutex(&name).unwrap(),
        NamedMutexAcquisition::Acquired(_)
    ));
}

#[test]
fn named_input_owner_is_exclusive_across_processes() {
    let name = format!(
        "Local\\DccMcpComputerUseInputOwner-test-{}",
        std::process::id()
    );
    let _owner = match try_acquire_named_mutex(&name).unwrap() {
        NamedMutexAcquisition::Acquired(owner) => owner,
        _ => panic!("test input owner should be available"),
    };
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("named_input_owner_child_probe")
        .arg("--nocapture")
        .env("DCC_MCP_TEST_INPUT_OWNER_NAME", &name)
        .status()
        .unwrap();

    assert!(status.success());
}

#[test]
fn named_input_owner_child_probe() {
    let Ok(name) = std::env::var("DCC_MCP_TEST_INPUT_OWNER_NAME") else {
        return;
    };

    assert!(matches!(
        try_acquire_named_mutex(&name).unwrap(),
        NamedMutexAcquisition::Busy
    ));
}

#[test]
fn abandoned_input_owner_requires_explicit_user_approval() {
    let _interrupt_guard = crate::user_interrupt_test_guard();
    clear_user_interrupt().unwrap();
    let name = format!(
        "Local\\DccMcpComputerUseAbandonedOwner-test-{}",
        std::process::id()
    );
    let owner = match try_acquire_named_mutex(&name).unwrap() {
        NamedMutexAcquisition::Acquired(owner) => owner,
        _ => panic!("test input owner should be available"),
    };

    let error = match resolve_input_owner(NamedMutexAcquisition::Abandoned(owner), false) {
        Ok(_) => panic!("abandoned ownership must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.code, ComputerUseErrorCode::UserInterrupted);
    assert!(user_interrupted());
    clear_user_interrupt().unwrap();
    assert!(!user_interrupted());
}

#[test]
fn manual_reset_interrupt_event_is_visible_across_processes() {
    let name = format!(
        "Local\\DccMcpComputerUseInterrupt-test-{}",
        std::process::id()
    );
    let event = create_manual_reset_event(&name).unwrap();
    unsafe { SetEvent(event.get()) }.unwrap();
    assert!(run_interrupt_event_child(&name, true));

    unsafe { ResetEvent(event.get()) }.unwrap();
    assert!(run_interrupt_event_child(&name, false));
}

fn run_interrupt_event_child(name: &str, expected: bool) -> bool {
    std::process::Command::new(std::env::current_exe().unwrap())
        .arg("manual_reset_interrupt_event_child_probe")
        .arg("--nocapture")
        .env("DCC_MCP_TEST_INTERRUPT_EVENT_NAME", name)
        .env(
            "DCC_MCP_TEST_INTERRUPT_EVENT_EXPECTED",
            if expected { "1" } else { "0" },
        )
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn manual_reset_interrupt_event_child_probe() {
    let Ok(name) = std::env::var("DCC_MCP_TEST_INTERRUPT_EVENT_NAME") else {
        return;
    };
    let expected = std::env::var("DCC_MCP_TEST_INTERRUPT_EVENT_EXPECTED").unwrap() == "1";
    let event = create_manual_reset_event(&name).unwrap();

    assert_eq!(event_signaled(&event), Some(expected));
}

#[test]
fn hidden_and_reused_windows_fail_closed() {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
    let effect = PointerEffect::new(200, 240, "●").unwrap();
    let other_window = PointerEffect::new(400, 440, "◎").unwrap();
    let handle = effect.hwnd.0 as usize as u64;
    let process_id = unsafe { GetCurrentProcessId() };

    let wrong_process = ensure_target_foreground(handle, process_id.saturating_add(1)).unwrap_err();
    assert_eq!(wrong_process.code, ComputerUseErrorCode::InvalidTarget);

    let occluded =
        prepare_point_target(200, 240, effect.hwnd, process_id.saturating_add(1)).unwrap_err();
    assert_eq!(occluded.code, ComputerUseErrorCode::InvalidTarget);

    assert!(!point_belongs_to_target(
        process_id,
        effect.hwnd,
        process_id,
        other_window.hwnd,
    ));

    let _ = unsafe { ShowWindow(effect.hwnd, SW_HIDE) };
    let hidden = available_target_rect(effect.hwnd).unwrap_err();
    assert_eq!(hidden.code, ComputerUseErrorCode::MissingWindow);
    assert!(validate_target_identity(effect.hwnd, process_id).is_ok());
}

#[test]
fn persistent_control_cursor_does_not_occlude_the_next_nearby_action() {
    let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
    let cursor_overlay = PointerEffect::new(300, 300, "●").unwrap();

    assert!(is_control_overlay_window(cursor_overlay.hwnd));
    assert!(is_input_transparent_window(cursor_overlay.hwnd));
}

#[test]
fn protected_system_ui_is_not_eligible_for_transient_occlusion_recovery() {
    assert!(protected_input_blocker(
        "PickerHost.exe",
        "Shell_SystemDialog"
    ));
    assert!(protected_input_blocker("consent.exe", "#32770"));
    assert!(protected_input_blocker("explorer.exe", "Shell_SystemDim"));
    assert!(!protected_input_blocker("nuke.exe", "Qt5152QWindowIcon"));
    assert!(!protected_input_blocker("chrome.exe", "Chrome_WidgetWin_1"));
}

#[test]
fn focus_recovery_policy_attempts_all_cross_process_blockers() {
    assert!(focus_recovery_allowed(100, 200).unwrap());

    let same_process = focus_recovery_allowed(100, 100).unwrap_err();
    assert_eq!(same_process.code, ComputerUseErrorCode::FocusLost);
}

#[test]
fn point_recovery_failure_preserves_protected_ui_boundary() {
    let protected = point_recovery_failure(
        "PickerHost.exe",
        "Shell_SystemDim",
        "PickerHost / Shell_SystemDim",
    );
    assert_eq!(protected.code, ComputerUseErrorCode::InvalidTarget);
    assert!(protected.message.contains("protected system UI"));

    let ordinary = point_recovery_failure(
        "ChatGPT.exe",
        "Chrome_WidgetWin_1",
        "ChatGPT / Chrome_WidgetWin_1",
    );
    assert_eq!(ordinary.code, ComputerUseErrorCode::InvalidTarget);
    assert!(ordinary.message.contains("occluded by"));
}

#[test]
fn minimized_window_is_identity_checked_then_restored_for_input() {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE;

    let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
    let effect = PointerEffect::new(200, 240, "●").unwrap();
    let process_id = unsafe { GetCurrentProcessId() };
    let _ = unsafe { ShowWindow(effect.hwnd, SW_MINIMIZE) };
    assert!(unsafe { IsIconic(effect.hwnd) }.as_bool());

    let wrong_process =
        restore_target_for_input(effect.hwnd, process_id.saturating_add(1)).unwrap_err();
    assert_eq!(wrong_process.code, ComputerUseErrorCode::InvalidTarget);
    assert!(unsafe { IsIconic(effect.hwnd) }.as_bool());

    restore_target_for_input(effect.hwnd, process_id).unwrap();
    assert!(!unsafe { IsIconic(effect.hwnd) }.as_bool());
}

#[test]
fn exact_window_state_can_restore_and_show_without_a_screenshot() {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_MINIMIZE};

    let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
    let effect = PointerEffect::new(240, 280, "●").unwrap();
    let process_id = unsafe { GetCurrentProcessId() };
    let window_handle = effect.hwnd.0 as usize as u64;

    let _ = unsafe { ShowWindow(effect.hwnd, SW_MINIMIZE) };
    let minimized = scoped_window_state(window_handle, process_id).unwrap();
    assert!(minimized.exists);
    assert!(minimized.minimized);

    let wrong_process = transition_scoped_window(
        window_handle,
        process_id.saturating_add(1),
        ScopedWindowOperation::Restore,
    )
    .unwrap_err();
    assert_eq!(wrong_process.code, ComputerUseErrorCode::InvalidTarget);
    assert!(unsafe { IsIconic(effect.hwnd) }.as_bool());

    if !desktop_interactive() {
        return;
    }

    let restored =
        transition_scoped_window(window_handle, process_id, ScopedWindowOperation::Restore)
            .unwrap();
    assert!(!restored.minimized);

    let _ = unsafe { ShowWindow(effect.hwnd, SW_HIDE) };
    let hidden = scoped_window_state(window_handle, process_id).unwrap();
    assert!(!hidden.visible);
    let activate_hidden =
        transition_scoped_window(window_handle, process_id, ScopedWindowOperation::Activate)
            .unwrap_err();
    assert_eq!(activate_hidden.code, ComputerUseErrorCode::InvalidAction);

    let shown =
        transition_scoped_window(window_handle, process_id, ScopedWindowOperation::Show).unwrap();
    assert!(shown.visible);
}

#[test]
fn pointer_mapping_revalidates_the_target_before_using_observation_coordinates() {
    let virtual_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let virtual_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let virtual_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(2);
    let observation = ComputerUseObservation {
        observation_id: "outside:1".to_string(),
        window_handle: 1,
        process_id: 1,
        window_title: "outside".to_string(),
        width: 100,
        height: 100,
        source_rect: [
            virtual_x.saturating_add(virtual_width).saturating_add(100),
            virtual_y,
            100,
            100,
        ],
        dpi_scale: 1.0,
        window_dpi: 96,
        capture_backend: "test".to_string(),
        timestamp_ms: 0,
        desktop_generation: 1,
        session_id: None,
    };

    let error = ensure_observation_target(observation.window_handle, &observation).unwrap_err();
    assert_eq!(error.code, ComputerUseErrorCode::MissingWindow);
}

#[test]
fn target_window_must_intersect_a_real_monitor() {
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }.unwrap();
    let visible = windows::Win32::Foundation::RECT {
        left: cursor.x,
        top: cursor.y,
        right: cursor.x.saturating_add(1),
        bottom: cursor.y.saturating_add(1),
    };
    assert!(rect_intersects_monitor(&visible));

    let offscreen = windows::Win32::Foundation::RECT {
        left: 1_000_000,
        top: 1_000_000,
        right: 1_000_100,
        bottom: 1_000_100,
    };
    assert!(!rect_intersects_monitor(&offscreen));
}

// ── Session color tests ──────────────────────────────────────────────────────

#[test]
fn session_color_is_deterministic() {
    let color_a = session_color("maya-session-1");
    let color_b = session_color("maya-session-1");
    assert_eq!(color_a, color_b);
}

#[test]
fn session_color_differs_for_different_ids() {
    // With 16 palette entries, two random strings are very unlikely to collide.
    let color_a = session_color("blender-instance-42");
    let color_b = session_color("maya-instance-7");
    assert_ne!(color_a, color_b);
}

#[test]
fn session_color_handles_empty_string() {
    let color = session_color("");
    // Must not panic; empty string maps to palette[0].
    let _ = color;
}

#[test]
fn glow_from_accent_is_lighter() {
    let accent = CONTROL_ACCENT_COLOR;
    let glow = glow_from_accent(accent);
    // Glow should have higher RGB values (lighter)
    let accent_sum = (accent.0 & 0xFF)
        + ((accent.0 >> 8) & 0xFF)
        + ((accent.0 >> 16) & 0xFF);
    let glow_sum =
        (glow.0 & 0xFF) + ((glow.0 >> 8) & 0xFF) + ((glow.0 >> 16) & 0xFF);
    assert!(glow_sum > accent_sum);
}

#[test]
fn cursor_from_accent_is_different() {
    let accent = CONTROL_ACCENT_COLOR;
    let cursor = cursor_from_accent(accent);
    assert_ne!(accent, cursor);
}

#[test]
fn all_palette_colors_are_reachable() {
    // Verify that hashing 100 random-ish IDs hits at least 2 distinct colors.
    let ids: Vec<String> = (0..100)
        .map(|i| format!("test-session-{}", i * 7 + 13))
        .collect();
    let mut colors: Vec<u32> = ids.iter().map(|id| session_color(id).0).collect();
    colors.sort_unstable();
    colors.dedup();
    assert!(colors.len() >= 2, "expected at least 2 distinct colors from palette");
}
