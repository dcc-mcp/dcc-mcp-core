use super::*;

struct RegisteredHotKey {
    hwnd: HWND,
}

impl Drop for RegisteredHotKey {
    fn drop(&mut self) {
        let _ = unsafe { UnregisterHotKey(Some(self.hwnd), HOTKEY_ID) };
    }
}

struct RegisteredSessionNotifications {
    hwnd: HWND,
}

struct RegisteredDesktopBarrier {
    barrier: Arc<DesktopEventBarrier>,
    window_handle: usize,
}

impl RegisteredDesktopBarrier {
    fn new(barrier: Arc<DesktopEventBarrier>, hwnd: HWND) -> Self {
        let window_handle = hwnd.0 as usize;
        barrier.register_window(window_handle);
        Self {
            barrier,
            window_handle,
        }
    }
}

impl Drop for RegisteredDesktopBarrier {
    fn drop(&mut self) {
        self.barrier.clear_window(self.window_handle);
    }
}

impl RegisteredSessionNotifications {
    fn new(hwnd: HWND) -> ComputerUseResult<Self> {
        unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) }.map_err(
            |error| {
                ComputerUseError::new(
                    ComputerUseErrorCode::BackendUnavailable,
                    format!("failed to monitor Windows lock and unlock events: {error}"),
                )
            },
        )?;
        Ok(Self { hwnd })
    }
}

impl Drop for RegisteredSessionNotifications {
    fn drop(&mut self) {
        let _ = unsafe { WTSUnRegisterSessionNotification(self.hwnd) };
    }
}

pub(super) fn session_event_blocked(event: u32) -> Option<bool> {
    match event {
        WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => Some(true),
        WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => Some(false),
        _ => None,
    }
}

struct BannerRuntimeSignals {
    stop: Arc<AtomicBool>,
    interrupted: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
    desktop_state: Arc<AtomicU64>,
    desktop_barrier: Arc<DesktopEventBarrier>,
}

pub(crate) fn start_control_banner(
    window_handle: u64,
    process_id: u32,
    app_name: String,
    signals: ControlBannerSignals,
) -> ControlBannerStartResult {
    let ControlBannerSignals {
        stop,
        interrupted,
        visible,
        desktop_state,
        desktop_barrier,
        target_available,
        cleanup_pending,
        session_id,
        last_action_point,
    } = signals;
    cleanup_pending.store(true, Ordering::Release);
    let _ = require_user_interrupt_event_raw().inspect_err(|_| {
        cleanup_pending.store(false, Ordering::Release);
    })?;
    if user_interrupted() {
        cleanup_pending.store(false, Ordering::Release);
        return Err(user_interrupted_error().into());
    }
    if ACTIVE_SESSION
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        cleanup_pending.store(false, Ordering::Release);
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::PermissionDenied,
            "another DCC UI Control session already owns system input",
        )
        .into());
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let runtime = BannerRuntimeSignals {
        stop,
        interrupted,
        visible,
        desktop_state,
        desktop_barrier,
    };
    let startup_stop = Arc::clone(&runtime.stop);
    let thread_stop = Arc::clone(&runtime.stop);
    let thread_cleanup_pending = Arc::clone(&cleanup_pending);
    let thread_last_action_point = Arc::clone(&last_action_point);
    let thread = thread::Builder::new()
        .name("dcc-mcp-computer-use-banner".to_string())
        .spawn(move || {
            let result = (|| {
                // Windows mutex ownership is thread-affine. Keep the guard on
                // the banner thread until local SendInput work has drained.
                let input_owner = acquire_input_owner()?;
                let _input_owner = InputOwnerLease::new(input_owner, Arc::clone(&thread_stop));
                if user_interrupted() {
                    return Err(user_interrupted_error());
                }
                flush_pending_input_releases()?;
                let _dpi_awareness = ThreadDpiAwareness::enter()?;
                run_banner(
                    window_handle,
                    process_id,
                    &app_name,
                    &runtime,
                    &ready_tx,
                    session_id.as_deref(),
                    &thread_last_action_point,
                )
            })();
            if let Err(error) = result {
                if matches!(
                    error.code,
                    ComputerUseErrorCode::MissingWindow | ComputerUseErrorCode::InvalidTarget
                ) {
                    target_available.store(false, Ordering::Release);
                }
                runtime.stop.store(true, Ordering::Release);
                let _ = ready_tx.try_send(Err(error));
            }
            runtime.visible.store(false, Ordering::Release);
            ACTIVE_SESSION.store(false, Ordering::Release);
            thread_cleanup_pending.store(false, Ordering::Release);
        })
        .map_err(|error| {
            ACTIVE_SESSION.store(false, Ordering::Release);
            cleanup_pending.store(false, Ordering::Release);
            ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                format!("failed to start the DCC UI Control thread: {error}"),
            )
        })?;

    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(thread),
        Ok(Err(error)) => {
            startup_stop.store(true, Ordering::Release);
            Err(ControlBannerStartError {
                error,
                thread: crate::join_control_thread(thread),
            })
        }
        Err(_) => {
            startup_stop.store(true, Ordering::Release);
            Err(ControlBannerStartError {
                error: ComputerUseError::new(
                    ComputerUseErrorCode::BackendUnavailable,
                    "timed out while starting the DCC UI Control capsule",
                ),
                thread: crate::join_control_thread(thread),
            })
        }
    }
}

fn run_banner(
    window_handle: u64,
    process_id: u32,
    app_name: &str,
    signals: &BannerRuntimeSignals,
    ready: &std::sync::mpsc::SyncSender<ComputerUseResult<()>>,
    session_id: Option<&str>,
    last_action_point: &Arc<std::sync::Mutex<Option<(i32, i32, std::time::Instant)>>>,
) -> ComputerUseResult<()> {
    ensure_interactive_desktop()?;
    let target = HWND(window_handle as *mut core::ffi::c_void);
    let caption = format!("DCC UI Control  ·  {app_name}  ·  {STOP_HOTKEY_LABEL} to stop");
    let mut rect = available_target_rect_for_process(target, process_id)?;
    let mut overlay = ControlOverlay::new(target, &rect, &caption, session_id)?;
    let overlay_window = overlay.window_handle();

    let hotkey_result = unsafe {
        RegisterHotKey(
            Some(overlay_window),
            HOTKEY_ID,
            STOP_HOTKEY_MODIFIERS,
            VK_ESCAPE.0 as u32,
        )
    };
    if let Err(error) = hotkey_result {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            format!("failed to reserve {STOP_HOTKEY_LABEL} for DCC UI Control: {error}"),
        ));
    }
    let _hotkey = RegisteredHotKey {
        hwnd: overlay_window,
    };
    let _session_notifications = RegisteredSessionNotifications::new(overlay_window)?;
    let _desktop_barrier =
        RegisteredDesktopBarrier::new(Arc::clone(&signals.desktop_barrier), overlay_window);
    let mut display_stamp = display_environment_stamp()?;

    record_desktop_transition(&signals.desktop_state, true);
    signals.visible.store(true, Ordering::Release);
    let _ = ready.send(Ok(()));

    let mut message = MSG::default();
    let mut session_blocked = false;
    let mut display_refresh_pending = false;
    let mut barrier_sequence = None;
    while !signals.stop.load(Ordering::Acquire) {
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            if message.message == DESKTOP_BARRIER_MESSAGE {
                barrier_sequence = Some(message.wParam.0 as u32);
                continue;
            }
            if message.message == WM_HOTKEY && message.wParam.0 == HOTKEY_ID as usize {
                set_user_interrupt();
                signals.interrupted.store(true, Ordering::Release);
                signals.stop.store(true, Ordering::Release);
                break;
            }
            if message.message == WM_WTSSESSION_CHANGE
                && let Some(blocked) = session_event_blocked(message.wParam.0 as u32)
            {
                session_blocked = blocked;
                if session_blocked {
                    record_desktop_transition(&signals.desktop_state, false);
                    if let Err(e) = overlay.set_visible(false) {
                        tracing::warn!(
                            "run_banner: overlay.set_visible(false) failed on WM_WTSSESSION_CHANGE \
                             (session blocked); session continues: {e}"
                        );
                    }
                    signals.visible.store(false, Ordering::Release);
                } else {
                    display_refresh_pending = true;
                }
            }
            display_refresh_pending |= matches!(message.message, WM_DISPLAYCHANGE | WM_DPICHANGED);
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if signals.stop.load(Ordering::Acquire) {
            break;
        }
        let interactive = !session_blocked && desktop_interactive();
        if !interactive {
            let desktop_changed = record_desktop_transition(&signals.desktop_state, false);
            if desktop_changed || signals.visible.load(Ordering::Acquire) {
                // Overlay visibility is cosmetic. A transient window-manager race
                // during a lock/disconnect transition must not kill the banner
                // thread — the safety guarantees (hotkey, session monitoring,
                // input owner) must survive cosmetic failures.
                if let Err(e) = overlay.set_visible(false) {
                    tracing::warn!(
                        "run_banner: overlay.set_visible(false) failed on non-interactive \
                         desktop (transient); session continues: {e}"
                    );
                }
                signals.visible.store(false, Ordering::Release);
            }
            thread::sleep(Duration::from_millis(16));
            continue;
        }
        if display_refresh_pending || barrier_sequence.is_some() {
            match display_environment_stamp() {
                Ok(current_display_stamp) => {
                    if current_display_stamp != display_stamp {
                        display_stamp = current_display_stamp;
                        record_desktop_environment_change(&signals.desktop_state);
                    }
                    display_refresh_pending = false;
                }
                Err(error) if error.code == ComputerUseErrorCode::DesktopUnavailable => {
                    record_desktop_transition(&signals.desktop_state, false);
                    if signals.visible.load(Ordering::Acquire) {
                        if let Err(e) = overlay.set_visible(false) {
                            tracing::warn!(
                                "run_banner: overlay.set_visible(false) failed on \
                                 DesktopUnavailable; session continues: {e}"
                            );
                        }
                        signals.visible.store(false, Ordering::Release);
                    }
                    thread::sleep(Duration::from_millis(16));
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        rect = match available_target_rect_for_process(target, process_id) {
            Ok(rect) => rect,
            Err(error) if error.code == ComputerUseErrorCode::MissingWindow => {
                validate_target_identity(target, process_id)?;
                if let Err(e) = overlay.set_visible(false) {
                    tracing::warn!(
                        "run_banner: overlay.set_visible(false) failed on MissingWindow; \
                         session continues: {e}"
                    );
                }
                signals.visible.store(false, Ordering::Release);
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(error) => return Err(error),
        };
        // Poll for new last-action points from the input thread
        if let Ok(mut point) = last_action_point.lock() {
            if let Some((screen_x, screen_y, _timestamp)) = point.take() {
                overlay.record_last_action(screen_x, screen_y);
            }
        }
        overlay.reposition(target, &rect)?;
        if !signals.visible.load(Ordering::Acquire) {
            overlay.set_visible(true)?;
            signals.visible.store(true, Ordering::Release);
        }
        record_desktop_transition(&signals.desktop_state, true);
        if let Some(sequence) = barrier_sequence.take() {
            signals.desktop_barrier.acknowledge(sequence);
        }
        thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}
