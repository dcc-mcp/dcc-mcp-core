use super::*;

fn trusted_pid_scope(process_id: u32) -> ComputerUseTargetScope {
    ComputerUseTargetScope::new(Some(process_id), None).unwrap()
}

fn test_session(process_id: u32) -> ComputerUseSession {
    ComputerUseSession::new(
        trusted_pid_scope(process_id),
        Some(process_id),
        None,
        None,
        None,
        None,
    )
    .unwrap()
}

#[test]
fn status_uses_public_dcc_ui_control_name() {
    let hint = test_session(std::process::id()).status()["hint"]
        .as_str()
        .unwrap()
        .to_string();

    assert!(hint.starts_with("DCC UI Control is controlling "));
    assert!(!hint.contains("Computer Use"));
}

#[test]
fn public_perform_cannot_bypass_the_game_navigation_host_fence() {
    let session = test_session(std::process::id());
    let error = session
        .perform(&ComputerUseAction {
            action: "game_navigation".to_owned(),
            observation_id: Some("untrusted-observation".to_owned()),
            x: None,
            y: None,
            button: None,
            scroll_x: None,
            scroll_y: None,
            path: Vec::new(),
            text: None,
            keys: vec!["W".to_owned()],
            duration_ms: Some(120),
        })
        .expect_err("public perform must not execute Host-governed game navigation");

    assert_eq!(error.code, ComputerUseErrorCode::InvalidAction);
    assert!(error.message.contains("scoped UI Control Host"));
}

#[test]
fn desktop_transitions_advance_generation_once_per_change() {
    let state = AtomicU64::new(desktop_state_value(1, true));

    assert!(!record_desktop_transition(&state, true));
    assert_eq!(desktop_state_snapshot(&state), (true, 1));

    assert!(record_desktop_transition(&state, false));
    assert_eq!(desktop_state_snapshot(&state), (false, 2));

    assert!(!record_desktop_transition(&state, false));
    assert_eq!(desktop_state_snapshot(&state), (false, 2));

    assert!(record_desktop_transition(&state, true));
    assert_eq!(desktop_state_snapshot(&state), (true, 3));
}

#[test]
fn display_environment_changes_advance_generation_without_suspending() {
    let state = AtomicU64::new(desktop_state_value(4, true));

    assert_eq!(record_desktop_environment_change(&state), 5);
    assert_eq!(desktop_state_snapshot(&state), (true, 5));
}

#[test]
fn desktop_barrier_acknowledges_every_earlier_fence() {
    let barrier = platform::DesktopEventBarrier::default();
    let first = barrier.request_sequence();
    let second = barrier.request_sequence();

    barrier.acknowledge(second);

    assert!(barrier.is_acknowledged(first));
    assert!(barrier.is_acknowledged(second));
    assert!(!barrier.is_acknowledged(barrier.request_sequence()));
}

#[test]
fn capture_failure_prefers_desktop_unavailable_after_disconnect() {
    let desktop_error = ComputerUseError::new(
        ComputerUseErrorCode::DesktopUnavailable,
        "the remote desktop disconnected",
    );

    let error = resolve_capture_error("PrintWindow timed out", Err(desktop_error));

    assert_eq!(error.code, ComputerUseErrorCode::DesktopUnavailable);
    assert!(error.message.contains("disconnected"));
}

#[test]
fn capture_failure_is_preserved_while_desktop_remains_interactive() {
    let error = resolve_capture_error("PrintWindow timed out", Ok(7));

    assert_eq!(error.code, ComputerUseErrorCode::CaptureFailed);
    assert!(error.message.contains("PrintWindow timed out"));
}

#[test]
fn observations_expire_across_desktop_transitions() {
    let observation = ComputerUseObservation {
        observation_id: "window:7".to_string(),
        window_handle: 0x1234,
        process_id: 42,
        window_title: "Godot".to_string(),
        width: 800,
        height: 600,
        source_rect: [0, 0, 800, 600],
        dpi_scale: 1.0,
        window_dpi: 96,
        capture_backend: "window".to_string(),
        timestamp_ms: 1,
        desktop_generation: 7,
        session_id: None,
    };

    assert!(validate_observation_desktop(&observation, 7).is_ok());
    let error = validate_observation_desktop(&observation, 8).unwrap_err();
    assert_eq!(error.code, ComputerUseErrorCode::StaleObservation);
    assert!(error.message.contains("desktop changed"));
}

#[test]
fn beginning_a_new_capture_invalidates_the_previous_observation() {
    let mut state = SessionState {
        observation: Some(ComputerUseObservation {
            observation_id: "window:7".to_string(),
            window_handle: 0x1234,
            process_id: 42,
            window_title: "Godot".to_string(),
            width: 800,
            height: 600,
            source_rect: [0, 0, 800, 600],
            dpi_scale: 1.0,
            window_dpi: 96,
            capture_backend: "window".to_string(),
            timestamp_ms: 1,
            desktop_generation: 7,
            session_id: None,
        }),
        ..SessionState::default()
    };

    invalidate_observation_before_capture(&mut state);

    assert!(state.observation.is_none());
}

#[test]
fn beginning_an_action_consumes_the_observation_before_native_preflight() {
    let mut state = SessionState {
        observation: Some(ComputerUseObservation {
            observation_id: "window:8".to_string(),
            window_handle: 0x1234,
            process_id: 42,
            window_title: "Godot".to_string(),
            width: 800,
            height: 600,
            source_rect: [0, 0, 800, 600],
            dpi_scale: 1.0,
            window_dpi: 96,
            capture_backend: "window".to_string(),
            timestamp_ms: 1,
            desktop_generation: 8,
            session_id: None,
        }),
        ..SessionState::default()
    };

    let observation = take_observation_for_action(&mut state).unwrap();

    assert_eq!(observation.observation_id, "window:8");
    assert!(state.observation.is_none());
}

#[test]
fn failed_banner_start_retains_a_pending_control_thread_for_cleanup() {
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let _ = release_rx.recv();
    });
    let mut state = SessionState::default();

    let error = retain_pending_control_thread(
        &mut state,
        Err(platform::ControlBannerStartError {
            error: ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                "banner startup failed",
            ),
            thread: Some(thread),
        }),
    )
    .unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::BackendUnavailable);
    assert!(state.overlay_thread.is_some());
    release_tx.send(()).unwrap();
    state.overlay_thread.take().unwrap().join().unwrap();
}

#[test]
fn action_payload_round_trips_all_computer_use_fields() {
    let raw = json!({
        "action": "drag",
        "observation_id": "window:7",
        "button": "left",
        "path": [{"x": 10.0, "y": 20.0}, {"x": 40.0, "y": 60.0}],
        "duration_ms": 250,
    });
    let action: ComputerUseAction = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(action.action, "drag");
    assert_eq!(action.path.len(), 2);
    assert_eq!(serde_json::to_value(action).unwrap(), raw);
}

#[test]
fn session_requires_an_adapter_runtime_bound_native_scope() {
    let error = ComputerUseTargetScope::new(None, None).unwrap_err();
    assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);

    let error = ComputerUseTargetScope::new(Some(0), None).unwrap_err();
    assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);

    let session = ComputerUseSession::new(
        trusted_pid_scope(7),
        None,
        None,
        None,
        Some("Godot".to_string()),
        None,
    )
    .unwrap();
    assert_eq!(session.spec.process_id, Some(7));

    let error = ComputerUseSession::new(
        trusted_pid_scope(7),
        Some(8),
        None,
        None,
        Some("Godot".to_string()),
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);
}

#[test]
fn sensitive_windows_are_denied_before_native_capture_or_input() {
    for process_name in [
        "PowerShell.exe",
        "powershell_ise.exe",
        "cmd",
        "Bitwarden",
        "SecHealthUI",
    ] {
        let process_name = process_name.trim_end_matches(".exe");
        assert!(
            denied_target_reason(process_name, "ApplicationFrameWindow", "Workspace").is_some()
        );
    }
    assert!(denied_target_reason("Godot", "ConsoleWindowClass", "Project").is_some());
    assert!(denied_target_reason("explorer", "#32770", "Run").is_some());
    assert!(denied_target_reason("explorer", "#32770", "\u{8fd0}\u{884c}").is_some());
    assert!(denied_target_reason("explorer", "#32770", "\u{30d5}\u{30a1}\u{30a4}\u{30eb}\u{540d}\u{3092}\u{6307}\u{5b9a}\u{3057}\u{3066}\u{5b9f}\u{884c}").is_some());
    for (process, class, title) in [
        ("Godot", "Qt5152QWindowIcon", "Project"),
        ("maya", "Qt5152QWindowIcon", "Autodesk Maya"),
        ("Photoshop", "Photoshop", "Adobe Photoshop"),
    ] {
        assert!(denied_target_reason(process, class, title).is_none());
    }
}

#[test]
fn trusted_scope_is_rechecked_against_the_resolved_native_identity() {
    let target = WindowInfo {
        handle: 0x1234,
        pid: 42,
        title: "Project - Godot Engine".to_string(),
        rect: [0, 0, 800, 600],
    };
    let exact = ComputerUseTargetScope::new(Some(42), Some(0x1234)).unwrap();
    assert!(exact.validate_target(&target).is_ok());

    for scope in [
        ComputerUseTargetScope::new(Some(99), None).unwrap(),
        ComputerUseTargetScope::new(None, Some(0x5678)).unwrap(),
    ] {
        let error = scope.validate_target(&target).unwrap_err();
        assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);
    }
}

#[cfg(windows)]
#[test]
fn public_perform_restores_a_minimized_observed_window_before_rect_validation() {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, IsIconic, SW_MINIMIZE, ShowWindow, WINDOW_EX_STYLE,
        WINDOW_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    use windows::core::PCWSTR;

    struct TestWindow(windows::Win32::Foundation::HWND);
    impl Drop for TestWindow {
        fn drop(&mut self) {
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    let _interrupt_guard = user_interrupt_test_guard();
    let _dpi_awareness = platform::ThreadDpiAwareness::enter().unwrap();
    platform::clear_user_interrupt().unwrap();
    let class = "STATIC\0".encode_utf16().collect::<Vec<_>>();
    let title = "DCC MCP minimized restore target\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0),
            200,
            200,
            640,
            480,
            None,
            None,
            None,
            None,
        )
    }
    .unwrap();
    let target_window = TestWindow(hwnd);
    let handle = hwnd.0 as usize as u64;
    let process_id = unsafe { GetCurrentProcessId() };
    let session = ComputerUseSession::new(
        ComputerUseTargetScope::new(Some(process_id), Some(handle)).unwrap(),
        Some(process_id),
        Some(handle),
        None,
        Some("test DCC".to_string()),
        None,
    )
    .unwrap();
    session.start().unwrap();

    let target = WindowFinder::new().info_for_handle(handle).unwrap();
    let desktop_generation = session.status()["desktop_generation"].as_u64().unwrap();
    let observed_dpi = platform::window_dpi(handle).unwrap();
    let observation = ComputerUseObservation {
        observation_id: "minimized-restore-observation".to_string(),
        window_handle: handle,
        process_id,
        window_title: target.title.clone(),
        width: target.rect[2] as u32,
        height: target.rect[3] as u32,
        source_rect: target.rect,
        dpi_scale: 1.0,
        window_dpi: observed_dpi,
        capture_backend: "test".to_string(),
        timestamp_ms: 1,
        desktop_generation,
        session_id: None,
    };
    session.lock_state().observation = Some(observation);

    let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
    assert!(unsafe { IsIconic(hwnd) }.as_bool());
    let result = session.perform(&ComputerUseAction {
        action: "wait".to_string(),
        observation_id: Some("minimized-restore-observation".to_string()),
        x: None,
        y: None,
        button: None,
        scroll_x: None,
        scroll_y: None,
        path: Vec::new(),
        text: None,
        keys: Vec::new(),
        duration_ms: Some(1),
    });

    let restored = WindowFinder::new().info_for_handle(handle).unwrap();
    let restored_dpi = platform::window_dpi(handle).unwrap();
    assert!(
        result.is_ok(),
        "{result:?}; observed_rect={:?}; restored_rect={:?}; observed_dpi={}; restored_dpi={restored_dpi}",
        target.rect,
        restored.rect,
        observed_dpi,
    );
    assert!(!unsafe { IsIconic(hwnd) }.as_bool());
    let _ = session.stop();
    drop(target_window);
    platform::clear_user_interrupt().unwrap();
}

#[cfg(windows)]
#[test]
fn minimized_target_start_owns_input_and_esc_watcher_before_restore() {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, IsIconic, SW_MINIMIZE, ShowWindow, WINDOW_EX_STYLE,
        WINDOW_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    use windows::core::PCWSTR;

    struct TestWindow(windows::Win32::Foundation::HWND);
    impl Drop for TestWindow {
        fn drop(&mut self) {
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    let _interrupt_guard = user_interrupt_test_guard();
    let _dpi_awareness = platform::ThreadDpiAwareness::enter().unwrap();
    platform::clear_user_interrupt().unwrap();
    let class = "STATIC\0".encode_utf16().collect::<Vec<_>>();
    let title = "DCC MCP minimized pre-snapshot owner target\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0),
            200,
            200,
            640,
            480,
            None,
            None,
            None,
            None,
        )
    }
    .unwrap();
    let _target_window = TestWindow(hwnd);
    let handle = hwnd.0 as usize as u64;
    let process_id = unsafe { GetCurrentProcessId() };
    let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
    assert!(unsafe { IsIconic(hwnd) }.as_bool());

    let session = ComputerUseSession::new(
        ComputerUseTargetScope::new(Some(process_id), Some(handle)).unwrap(),
        Some(process_id),
        Some(handle),
        None,
        Some("test DCC".to_string()),
        None,
    )
    .unwrap();
    let started = session.start().unwrap();

    assert_eq!(started["active"], true);
    assert_eq!(started["overlay_visible"], false);
    assert_ne!(session.lock_state().desktop_barrier.window_handle(), 0);
    assert!(platform::input_owner_is_busy_for_test());

    let _ = session.stop();
    platform::clear_user_interrupt().unwrap();
}

#[cfg(windows)]
#[test]
fn two_exact_window_sessions_share_the_process_input_owner() {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WS_OVERLAPPEDWINDOW,
        WS_VISIBLE,
    };
    use windows::core::PCWSTR;

    struct TestWindow(windows::Win32::Foundation::HWND);
    impl Drop for TestWindow {
        fn drop(&mut self) {
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    fn create_test_window(title: &str, x: i32) -> TestWindow {
        let class = "STATIC\0".encode_utf16().collect::<Vec<_>>();
        let title = format!("{title}\0").encode_utf16().collect::<Vec<_>>();
        TestWindow(
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    PCWSTR(class.as_ptr()),
                    PCWSTR(title.as_ptr()),
                    WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0),
                    x,
                    220,
                    480,
                    320,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap(),
        )
    }

    let _interrupt_guard = user_interrupt_test_guard();
    let _dpi_awareness = platform::ThreadDpiAwareness::enter().unwrap();
    platform::clear_user_interrupt().unwrap();
    let first_window = create_test_window("DCC MCP first session", 120);
    let second_window = create_test_window("DCC MCP second session", 640);
    let process_id = unsafe { GetCurrentProcessId() };
    let first_handle = first_window.0.0 as usize as u64;
    let second_handle = second_window.0.0 as usize as u64;
    let first = ComputerUseSession::new(
        ComputerUseTargetScope::new(Some(process_id), Some(first_handle)).unwrap(),
        Some(process_id),
        Some(first_handle),
        None,
        Some("first DCC".to_owned()),
        None,
    )
    .unwrap();
    let second = ComputerUseSession::new(
        ComputerUseTargetScope::new(Some(process_id), Some(second_handle)).unwrap(),
        Some(process_id),
        Some(second_handle),
        None,
        Some("second DCC".to_owned()),
        None,
    )
    .unwrap();

    first.start().unwrap();
    let second_start = second.start();
    assert!(
        second_start.is_ok(),
        "a second exact-window session should share the process input owner: {second_start:?}"
    );
    assert!(platform::input_owner_is_busy_for_test());

    let _ = first.stop();
    assert_eq!(second.status()["active"], true);
    assert!(platform::input_owner_is_busy_for_test());
    let _ = second.stop();
    platform::clear_user_interrupt().unwrap();
}

#[test]
fn target_constraints_are_an_intersection() {
    let spec = TargetSpec {
        process_id: Some(42),
        window_handle: Some(0x1234),
        window_title: Some("godot".to_string()),
        app_name: "Godot".to_string(),
    };
    let matching = WindowInfo {
        handle: 0x1234,
        pid: 42,
        title: "Project - Godot Engine".to_string(),
        rect: [0, 0, 800, 600],
    };
    assert!(target_matches(&spec, &matching));

    for rejected in [
        WindowInfo {
            pid: 99,
            ..matching.clone()
        },
        WindowInfo {
            handle: 0x5678,
            ..matching.clone()
        },
        WindowInfo {
            title: "Another application".to_string(),
            ..matching.clone()
        },
    ] {
        let error = validate_target_constraints(&spec, &rejected).unwrap_err();
        assert_eq!(error.code, ComputerUseErrorCode::InvalidTarget);
    }
}

#[test]
fn pid_only_scope_rejects_multiple_visible_windows() {
    let spec = TargetSpec {
        process_id: Some(42),
        window_handle: None,
        window_title: None,
        app_name: "Godot".to_string(),
    };
    let windows = [
        WindowInfo {
            handle: 0x1234,
            pid: 42,
            title: "Godot Project Manager".to_string(),
            rect: [0, 0, 800, 600],
        },
        WindowInfo {
            handle: 0x5678,
            pid: 42,
            title: "Project - Godot Engine".to_string(),
            rect: [100, 100, 1200, 800],
        },
    ];

    let error = select_unique_target(&spec, windows).unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::InvalidTarget);
    assert!(error.message.contains("exact window_handle"));
}

#[test]
fn lost_target_is_not_reported_as_user_interruption() {
    let session = test_session(7);
    let state = SessionState {
        active: true,
        ..SessionState::default()
    };
    state.interrupted.store(true, Ordering::Release);

    let error = session.ensure_running(&state).unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::MissingWindow);
}

#[test]
fn stop_cancels_a_long_wait_before_taking_the_state_lock() {
    let _interrupt_guard = user_interrupt_test_guard();
    platform::clear_user_interrupt().unwrap();
    let session = Arc::new(test_session(7));
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
    let worker_session = Arc::clone(&session);
    let worker = std::thread::spawn(move || {
        let _state = worker_session.lock_state();
        started_tx.send(()).unwrap();
        cancellable_wait(Duration::from_secs(5), &worker_session.stop_requested)
    });
    started_rx.recv().unwrap();

    let started = Instant::now();
    let stopped = session.stop();
    let elapsed = started.elapsed();
    let error = worker.join().unwrap().unwrap_err();

    assert!(elapsed < Duration::from_secs(1), "stop took {elapsed:?}");
    assert_eq!(error.code, ComputerUseErrorCode::BackendUnavailable);
    assert_eq!(stopped["active"], false);
    platform::clear_user_interrupt().unwrap();
}

#[test]
fn recording_fence_rejects_desktop_or_target_generation_changes() {
    let stop_requested = Arc::new(AtomicBool::new(false));
    let interrupted = Arc::new(AtomicBool::new(false));
    let desktop_state = Arc::new(AtomicU64::new(desktop_state_value(7, true)));
    let target_available = Arc::new(AtomicBool::new(true));
    let fence = ComputerUseRecordingFence {
        stop_requested,
        interrupted,
        desktop_state: Arc::clone(&desktop_state),
        target_available: Arc::clone(&target_available),
        desktop_generation: 7,
    };

    fence.check().expect("initial fence");
    record_desktop_environment_change(&desktop_state);
    assert_eq!(
        fence.check().unwrap_err().code,
        ComputerUseErrorCode::StaleObservation
    );

    let target_fence = ComputerUseRecordingFence {
        stop_requested: Arc::new(AtomicBool::new(false)),
        interrupted: Arc::new(AtomicBool::new(false)),
        desktop_state: Arc::new(AtomicU64::new(desktop_state_value(9, true))),
        target_available: Arc::clone(&target_available),
        desktop_generation: 9,
    };
    target_available.store(false, Ordering::Release);
    assert_eq!(
        target_fence.check().unwrap_err().code,
        ComputerUseErrorCode::MissingWindow
    );
}

#[test]
fn control_thread_join_is_bounded_when_a_worker_is_stuck() {
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let _ = release_rx.recv();
    });

    let started = Instant::now();
    assert!(join_control_thread_with_timeout(thread, Duration::from_millis(30)).is_some());
    assert!(started.elapsed() < Duration::from_millis(250));

    release_tx.send(()).unwrap();
}

#[test]
fn control_thread_join_waits_for_normal_cleanup() {
    let thread = std::thread::spawn(|| {});

    assert!(join_control_thread_with_timeout(thread, Duration::from_millis(250)).is_none());
}

#[test]
fn start_does_not_reset_stop_while_previous_cleanup_is_pending() {
    let session = test_session(7);
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    {
        let mut state = session.lock_state();
        state.cleanup_pending.store(true, Ordering::Release);
        state.overlay_thread = Some(std::thread::spawn(move || {
            let _ = release_rx.recv();
        }));
    }
    session.stop_requested.store(true, Ordering::Release);

    let error = session.start().unwrap_err();

    assert_eq!(error.code, ComputerUseErrorCode::BackendUnavailable);
    assert!(session.stop_requested.load(Ordering::Acquire));
    assert_eq!(session.status()["cleanup_pending"], true);

    release_tx.send(()).unwrap();
    let _ = session.stop();
}

#[test]
fn screenshot_scale_bounds_large_frames_without_upscaling() {
    assert_eq!(screenshot_scale([0, 0, 800, 600]), 1.0);

    let scale = screenshot_scale([0, 0, 2400, 1500]);
    assert!(scale < 0.65);
    assert!(scale > 0.64);
}

#[test]
fn captured_target_identity_and_rect_must_remain_stable() {
    let before = WindowInfo {
        handle: 0x1234,
        pid: 42,
        title: "Godot".to_string(),
        rect: [100, 200, 800, 600],
    };
    assert_eq!(
        validate_captured_target(&before, &before, Some(before.rect)).unwrap(),
        before.rect
    );

    let reused = WindowInfo {
        pid: 99,
        ..before.clone()
    };
    assert_eq!(
        validate_captured_target(&before, &reused, Some(before.rect))
            .unwrap_err()
            .code,
        ComputerUseErrorCode::InvalidTarget
    );

    let moved = WindowInfo {
        rect: [110, 200, 800, 600],
        ..before.clone()
    };
    assert_eq!(
        validate_captured_target(&before, &moved, Some(before.rect))
            .unwrap_err()
            .code,
        ComputerUseErrorCode::StaleObservation
    );
}

#[test]
fn native_action_limits_are_enforced_beyond_the_schema() {
    let mut request = ComputerUseAction {
        action: "drag".to_string(),
        observation_id: None,
        x: None,
        y: None,
        button: None,
        scroll_x: None,
        scroll_y: None,
        path: vec![ComputerUsePoint { x: 1.0, y: 1.0 }; MAX_DRAG_POINTS + 1],
        text: None,
        keys: Vec::new(),
        duration_ms: None,
    };
    assert_eq!(
        validate_action_limits(&request).unwrap_err().code,
        ComputerUseErrorCode::InvalidAction
    );

    request.path.clear();
    request.keys = vec!["CTRL+SHIFT+ALT+A+B+C+D+E+F+G+H+I+J+K+L+M+N".to_string()];
    assert_eq!(
        validate_action_limits(&request).unwrap_err().code,
        ComputerUseErrorCode::InvalidAction
    );

    request.keys.clear();
    request.text = Some("x".repeat(MAX_TEXT_UTF16_UNITS + 1));
    assert_eq!(
        validate_action_limits(&request).unwrap_err().code,
        ComputerUseErrorCode::InvalidAction
    );
}
