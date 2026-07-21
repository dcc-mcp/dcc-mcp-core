use super::*;

mod send_input;

pub(crate) use send_input::flush_pending_input_releases;
pub(super) use send_input::flush_pending_input_releases_locked;
#[cfg(test)]
use send_input::{compensating_releases, flush_pending_input_releases_with, send_inputs_with};
use send_input::{defer_input_releases, send_inputs};

struct PointerEffect {
    hwnd: HWND,
}

impl PointerEffect {
    fn new(screen_x: i32, screen_y: i32, glyph: &str) -> ComputerUseResult<Self> {
        let (x, y, size, _) = pointer_mask_geometry(screen_x, screen_y);
        let hwnd = create_color_overlay(
            glyph,
            (x, y, size, size),
            CONTROL_OVERLAY_ALPHA,
            true,
            OverlayTone::Cursor,
            None,
        )?;
        Ok(Self { hwnd })
    }

    fn reposition(&self, screen_x: i32, screen_y: i32) -> ComputerUseResult<()> {
        let (x, y, size, _) = pointer_mask_geometry(screen_x, screen_y);
        position_overlay(self.hwnd, (x, y, size, size), true)?;
        pump_overlay_messages(self.hwnd);
        Ok(())
    }

    fn dwell(&self, guard: &ActionGuard<'_>, duration: Duration) -> ComputerUseResult<()> {
        let deadline = Instant::now() + duration;
        loop {
            pump_overlay_messages(self.hwnd);
            guard.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
}

impl Drop for PointerEffect {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

pub(crate) fn perform_action(
    window_handle: u64,
    observation: &ComputerUseObservation,
    request: &ComputerUseAction,
    stop_requested: &Arc<AtomicBool>,
    desktop_state: &Arc<AtomicU64>,
    desktop_barrier: &Arc<DesktopEventBarrier>,
    mut pre_input_fence: Option<&mut PreInputFence<'_>>,
    last_action_point: &Arc<std::sync::Mutex<Option<(i32, i32, std::time::Instant)>>>,
) -> ComputerUseResult<()> {
    if matches!(
        request.action.as_str(),
        "click" | "raw_coordinate_click" | "double_click" | "scroll" | "drag"
    ) && !crate::keyboard_policy::are_pointer_modifiers(&request.keys)
    {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "pointer action keys only allow Ctrl, Shift, Alt, and their left/right variants",
        ));
    }
    let _input_guard = INPUT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    flush_pending_input_releases_locked()?;
    let guard = ActionGuard::new(
        stop_requested,
        desktop_state,
        desktop_barrier,
        observation.desktop_generation,
    );
    guard.synchronize()?;
    let _focus_elevation = focus_target(window_handle, observation.process_id)?;
    guard.check()?;
    ensure_observation_target(window_handle, observation)?;

    match request.action.as_str() {
        "move" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(
                window_handle,
                observation,
                point,
                &guard,
                true,
                &mut pre_input_fence,
            )?;
            if let Ok(mut pt) = last_action_point.lock() {
                *pt = Some((screen_x, screen_y, std::time::Instant::now()));
            }
            let effect = PointerEffect::new(screen_x, screen_y, "●")?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "click" | "raw_coordinate_click" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(
                window_handle,
                observation,
                point,
                &guard,
                true,
                &mut pre_input_fence,
            )?;
            click(
                window_handle,
                observation,
                (screen_x, screen_y),
                request,
                1,
                &guard,
                &mut pre_input_fence,
            )?;
            if let Ok(mut pt) = last_action_point.lock() {
                *pt = Some((screen_x, screen_y, std::time::Instant::now()));
            }
            let effect = PointerEffect::new(screen_x, screen_y, "●")?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "double_click" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(
                window_handle,
                observation,
                point,
                &guard,
                true,
                &mut pre_input_fence,
            )?;
            click(
                window_handle,
                observation,
                (screen_x, screen_y),
                request,
                2,
                &guard,
                &mut pre_input_fence,
            )?;
            if let Ok(mut pt) = last_action_point.lock() {
                *pt = Some((screen_x, screen_y, std::time::Instant::now()));
            }
            let effect = PointerEffect::new(screen_x, screen_y, "◎")?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "scroll" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(
                window_handle,
                observation,
                point,
                &guard,
                true,
                &mut pre_input_fence,
            )?;
            scroll(
                window_handle,
                observation,
                screen_x,
                screen_y,
                request,
                &guard,
                &mut pre_input_fence,
            )?;
            if let Ok(mut pt) = last_action_point.lock() {
                *pt = Some((screen_x, screen_y, std::time::Instant::now()));
            }
            let effect = PointerEffect::new(screen_x, screen_y, "↕")?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "drag" => drag(
            window_handle,
            observation,
            request,
            &guard,
            &mut pre_input_fence,
        )?,
        "type" => type_text(
            window_handle,
            observation.process_id,
            request.text.as_deref().unwrap_or(""),
            &guard,
            &mut pre_input_fence,
        )?,
        "keypress" | "keyboard_shortcut" => keypress(
            window_handle,
            observation.process_id,
            &request.keys,
            &guard,
            &mut pre_input_fence,
        )?,
        "wait" => guard.sleep(Duration::from_millis(
            request.duration_ms.unwrap_or(1000).min(60_000),
        ))?,
        action => {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::InvalidAction,
                format!("unsupported native computer-use action {action:?}"),
            ));
        }
    }
    guard.check()
}

struct ActionGuard<'a> {
    stop_requested: &'a Arc<AtomicBool>,
    desktop_state: &'a Arc<AtomicU64>,
    desktop_barrier: &'a Arc<DesktopEventBarrier>,
    desktop_generation: u64,
}

impl<'a> ActionGuard<'a> {
    fn new(
        stop_requested: &'a Arc<AtomicBool>,
        desktop_state: &'a Arc<AtomicU64>,
        desktop_barrier: &'a Arc<DesktopEventBarrier>,
        desktop_generation: u64,
    ) -> Self {
        Self {
            stop_requested,
            desktop_state,
            desktop_barrier,
            desktop_generation,
        }
    }

    fn check(&self) -> ComputerUseResult<()> {
        crate::check_action_cancellation(self.stop_requested)?;
        ensure_interactive_desktop()?;
        let (interactive, generation) = desktop_state_snapshot(self.desktop_state);
        if !interactive {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::DesktopUnavailable,
                "the Windows desktop became locked, disconnected, or non-interactive; the action was paused",
            ));
        }
        if generation != self.desktop_generation {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::StaleObservation,
                "the Windows desktop or display environment changed during the action; take a fresh screenshot",
            ));
        }
        Ok(())
    }

    fn synchronize(&self) -> ComputerUseResult<()> {
        synchronize_desktop_events(self.desktop_barrier, self.stop_requested)?;
        self.check()
    }

    fn sleep(&self, duration: Duration) -> ComputerUseResult<()> {
        let deadline = std::time::Instant::now() + duration;
        while std::time::Instant::now() < deadline {
            self.check()?;
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            thread::sleep(remaining.min(Duration::from_millis(10)));
        }
        self.check()
    }
}

fn pointer_effect_dwell(request: &ComputerUseAction) -> Duration {
    Duration::from_millis(
        request
            .duration_ms
            .unwrap_or(DEFAULT_POINTER_EFFECT_DWELL_MS)
            .clamp(100, 2_000),
    )
}

pub(super) fn set_target_foreground(hwnd: HWND) {
    // AttachThreadInput requires a USER message queue. Adapter worker threads
    // often do not have one until they call PeekMessage at least once.
    let mut queue_probe = MSG::default();
    let _ = unsafe { PeekMessageW(&mut queue_probe, None, 0, 0, PM_NOREMOVE) };
    let current_thread = unsafe { GetCurrentThreadId() };
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
    let foreground = unsafe { GetForegroundWindow() };
    let foreground_thread = if foreground.0.is_null() {
        0
    } else {
        unsafe { GetWindowThreadProcessId(foreground, None) }
    };
    let attached_foreground = foreground_thread != 0
        && foreground_thread != current_thread
        && unsafe { AttachThreadInput(current_thread, foreground_thread, true) }.as_bool();
    let attached_target = target_thread != 0
        && target_thread != current_thread
        && unsafe { AttachThreadInput(current_thread, target_thread, true) }.as_bool();

    let _ = unsafe { BringWindowToTop(hwnd) };
    let _ = unsafe { SetForegroundWindow(hwnd) };
    if attached_target {
        let _ = unsafe { AttachThreadInput(current_thread, target_thread, false) };
    }
    if attached_foreground {
        let _ = unsafe { AttachThreadInput(current_thread, foreground_thread, false) };
    }
}

fn focus_recovery_allowed(
    target_process_id: u32,
    blocker_process_id: u32,
) -> ComputerUseResult<bool> {
    if blocker_process_id == 0 {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::FocusLost,
            "Windows did not report a foreground window after focusing the scoped DCC",
        ));
    }
    if blocker_process_id == target_process_id {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::FocusLost,
            "another window owned by the scoped DCC has foreground focus; resolve the in-app modal before retrying",
        ));
    }
    Ok(true)
}

fn focus_target(
    window_handle: u64,
    process_id: u32,
) -> ComputerUseResult<Option<TransientTopmostTarget>> {
    ensure_interactive_desktop()?;
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    restore_target_for_input(hwnd, process_id)?;
    let _ = available_target_rect_for_process(hwnd, process_id)?;
    if unsafe { GetForegroundWindow() } == hwnd {
        return Ok(None);
    }

    set_target_foreground(hwnd);
    thread::sleep(Duration::from_millis(30));
    let blocker = unsafe { GetForegroundWindow() };
    if blocker == hwnd {
        return Ok(None);
    }
    let mut blocker_process_id = 0_u32;
    if !blocker.0.is_null() {
        unsafe { GetWindowThreadProcessId(blocker, Some(&mut blocker_process_id)) };
    }
    let (blocker_process, blocker_class) = if blocker.0.is_null() {
        (String::new(), String::new())
    } else {
        blocker_process_and_class(blocker)
    };
    focus_recovery_allowed(process_id, blocker_process_id)?;

    let elevation = TransientTopmostTarget::raise(hwnd)?;
    set_target_foreground(hwnd);
    thread::sleep(Duration::from_millis(30));
    if unsafe { GetForegroundWindow() } != hwnd {
        if protected_input_blocker(&blocker_process, &blocker_class) {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::InvalidTarget,
                format!(
                    "the scoped DCC could not recover foreground focus through protected system UI: {blocker_process} / {blocker_class}"
                ),
            ));
        }
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::FocusLost,
            format!(
                "the scoped DCC window did not remain in the foreground after recovering from {blocker_process} / {blocker_class}"
            ),
        ));
    }
    Ok(Some(elevation))
}

fn protected_input_blocker(process: &str, class_name: &str) -> bool {
    matches!(
        process.to_ascii_lowercase().as_str(),
        "consent.exe"
            | "credentialuibroker.exe"
            | "lockapp.exe"
            | "logonui.exe"
            | "pickerhost.exe"
            | "securityhealthsystray.exe"
    ) || matches!(
        class_name,
        "Credential Dialog Xaml Host" | "Shell_SystemDialog" | "Shell_SystemDim"
    )
}

fn point_recovery_failure(
    blocker_process: &str,
    blocker_class: &str,
    blocker_identity: &str,
) -> ComputerUseError {
    if protected_input_blocker(blocker_process, blocker_class) {
        return ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            format!(
                "the requested pointer coordinate remains blocked by protected system UI: {blocker_identity}"
            ),
        );
    }
    ComputerUseError::new(
        ComputerUseErrorCode::InvalidTarget,
        format!("the requested pointer coordinate is occluded by {blocker_identity}"),
    )
}

fn point_belongs_to_target(
    hit_process_id: u32,
    hit_root: HWND,
    target_process_id: u32,
    target: HWND,
) -> bool {
    hit_process_id == target_process_id && hit_root == target
}

fn blocker_process_and_class(hwnd: HWND) -> (String, String) {
    let mut process_id = 0_u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    let process = process_name(process_id).unwrap_or_else(|_| format!("process {process_id}"));
    let mut class_name = [0_u16; 128];
    let length = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
    (process, String::from_utf16_lossy(&class_name[..length]))
}

#[derive(Debug)]
struct TransientTopmostTarget {
    hwnd: HWND,
    restore: bool,
}

impl TransientTopmostTarget {
    fn raise(hwnd: HWND) -> ComputerUseResult<Self> {
        let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        let restore = ex_style & WS_EX_TOPMOST.0 == 0;
        if restore {
            unsafe {
                SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )
            }
            .map_err(|error| {
                ComputerUseError::new(
                    ComputerUseErrorCode::FocusLost,
                    format!("could not temporarily raise the scoped DCC window: {error}"),
                )
            })?;
        }
        Ok(Self { hwnd, restore })
    }
}

impl Drop for TransientTopmostTarget {
    fn drop(&mut self) {
        if self.restore {
            let _ = unsafe {
                SetWindowPos(
                    self.hwnd,
                    Some(HWND_NOTOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )
            };
        }
    }
}

fn ensure_target_foreground(window_handle: u64, process_id: u32) -> ComputerUseResult<()> {
    ensure_interactive_desktop()?;
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    let _ = available_target_rect_for_process(hwnd, process_id)?;
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::FocusLost,
            "the scoped DCC window lost foreground focus; no further input was sent",
        ));
    }
    Ok(())
}

fn prepare_point_target(
    screen_x: i32,
    screen_y: i32,
    target: HWND,
    process_id: u32,
) -> ComputerUseResult<Option<TransientTopmostTarget>> {
    let hit = unsafe {
        WindowFromPoint(POINT {
            x: screen_x,
            y: screen_y,
        })
    };
    if hit.0.is_null() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "no visible window owns the requested pointer coordinate",
        ));
    }
    // The persistent cursor halo is intentionally a separate, topmost,
    // click-through window. WindowFromPoint can still return it even though
    // native input passes through. Ignore any WS_EX_TRANSPARENT presentation
    // layer (including our registered overlays), then fail closed if a real
    // input-receiving top-level window is above the scoped DCC here.
    let hit = if is_input_transparent_window(hit) {
        first_input_receiving_window_above_target_at_point(target, screen_x, screen_y)
            .unwrap_or(target)
    } else {
        hit
    };
    let mut hit_process_id = 0_u32;
    unsafe { GetWindowThreadProcessId(hit, Some(&mut hit_process_id)) };
    if hit_process_id != process_id {
        let elevation = TransientTopmostTarget::raise(target)?;
        thread::sleep(Duration::from_millis(16));
        let retry = unsafe {
            WindowFromPoint(POINT {
                x: screen_x,
                y: screen_y,
            })
        };
        let retry = if is_input_transparent_window(retry) {
            first_input_receiving_window_above_target_at_point(target, screen_x, screen_y)
                .unwrap_or(target)
        } else {
            retry
        };
        let mut retry_process_id = 0_u32;
        unsafe { GetWindowThreadProcessId(retry, Some(&mut retry_process_id)) };
        let retry_root = unsafe { GetAncestor(retry, GA_ROOT) };
        if point_belongs_to_target(retry_process_id, retry_root, process_id, target) {
            return Ok(Some(elevation));
        }
        let (retry_process, retry_class) = blocker_process_and_class(retry);
        return Err(point_recovery_failure(
            &retry_process,
            &retry_class,
            &input_blocker_identity(retry),
        ));
    }
    let hit_root = unsafe { GetAncestor(hit, GA_ROOT) };
    if !point_belongs_to_target(hit_process_id, hit_root, process_id, target) {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "the requested pointer coordinate is outside the scoped top-level window",
        ));
    }
    Ok(None)
}

fn ensure_cursor_at(screen_x: i32, screen_y: i32) -> ComputerUseResult<()> {
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }.map_err(|error| {
        ComputerUseError::new(
            ComputerUseErrorCode::InputFailed,
            format!("GetCursorPos failed before input injection: {error}"),
        )
    })?;
    if cursor.x != screen_x || cursor.y != screen_y {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "the pointer moved after observation; take a new screenshot before clicking",
        ));
    }
    Ok(())
}

fn current_window_rect(window_handle: u64) -> ComputerUseResult<[i32; 4]> {
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    let rect = available_target_rect(hwnd)?;
    Ok([
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    ])
}

pub(crate) fn window_dpi(window_handle: u64) -> ComputerUseResult<u32> {
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "the scoped DCC window no longer exists",
        ));
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "Windows could not resolve the scoped DCC window DPI",
        ));
    }
    Ok(dpi)
}

fn ensure_observation_rect(
    observation: &ComputerUseObservation,
    current_rect: [i32; 4],
) -> ComputerUseResult<()> {
    if current_rect != observation.source_rect {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::StaleObservation,
            "target window moved or resized while being focused; take a new screenshot",
        ));
    }
    Ok(())
}

fn ensure_observation_target_state(
    observation: &ComputerUseObservation,
    current_rect: [i32; 4],
    current_dpi: u32,
) -> ComputerUseResult<()> {
    ensure_observation_rect(observation, current_rect)?;
    if current_dpi != observation.window_dpi {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::StaleObservation,
            "target window DPI changed after the screenshot; take a fresh screenshot",
        ));
    }
    Ok(())
}

fn ensure_observation_target(
    window_handle: u64,
    observation: &ComputerUseObservation,
) -> ComputerUseResult<()> {
    ensure_observation_target_state(
        observation,
        current_window_rect(window_handle)?,
        window_dpi(window_handle)?,
    )
}

fn required_point(request: &ComputerUseAction) -> ComputerUseResult<ComputerUsePoint> {
    match (request.x, request.y) {
        (Some(x), Some(y)) => Ok(ComputerUsePoint { x, y }),
        _ => Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            format!("{} requires x and y screenshot coordinates", request.action),
        )),
    }
}

fn move_to(
    window_handle: u64,
    observation: &ComputerUseObservation,
    point: ComputerUsePoint,
    guard: &ActionGuard<'_>,
    require_target_hit: bool,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<(i32, i32)> {
    let mapped = mapped_pointer_point(observation, point)?;
    move_to_mapped(
        window_handle,
        observation,
        mapped,
        guard,
        require_target_hit,
        pre_input_fence,
    )
}

fn move_to_mapped(
    window_handle: u64,
    observation: &ComputerUseObservation,
    mapped: MappedPointerPoint,
    guard: &ActionGuard<'_>,
    require_target_hit: bool,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<(i32, i32)> {
    guard.synchronize()?;
    ensure_observation_target(window_handle, observation)?;
    // No input has been sent yet. Reacquire the already-scoped target after
    // the desktop-barrier handshake so a caller window cannot steal focus in
    // the small gap between initial preparation and the first pointer move.
    let _focus_elevation = focus_target(window_handle, observation.process_id)?;
    ensure_observation_target(window_handle, observation)?;
    let _elevation = if require_target_hit {
        prepare_point_target(
            mapped.screen_x,
            mapped.screen_y,
            HWND(window_handle as *mut core::ffi::c_void),
            observation.process_id,
        )?
    } else {
        None
    };
    guard.check()?;
    run_pre_input_fence(pre_input_fence)?;
    send_mouse(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        mapped.absolute_x,
        mapped.absolute_y,
        0,
    )?;
    Ok((mapped.screen_x, mapped.screen_y))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MappedPointerPoint {
    screen_x: i32,
    screen_y: i32,
    absolute_x: i32,
    absolute_y: i32,
}

fn mapped_pointer_point(
    observation: &ComputerUseObservation,
    point: ComputerUsePoint,
) -> ComputerUseResult<MappedPointerPoint> {
    mapped_pointer_point_for_desktop(observation, point, virtual_desktop_geometry())
}

fn mapped_pointer_point_for_desktop(
    observation: &ComputerUseObservation,
    point: ComputerUsePoint,
    virtual_desktop: [i32; 4],
) -> ComputerUseResult<MappedPointerPoint> {
    let Some((screen_x, screen_y)) = screenshot_point_to_screen(
        point,
        [observation.width, observation.height],
        observation.source_rect,
    ) else {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "pointer coordinates are outside the latest screenshot",
        ));
    };
    let [virtual_x, virtual_y, virtual_width, virtual_height] = virtual_desktop;
    let virtual_width = virtual_width.max(2);
    let virtual_height = virtual_height.max(2);
    let virtual_right = virtual_x.saturating_add(virtual_width);
    let virtual_bottom = virtual_y.saturating_add(virtual_height);
    if !(virtual_x..virtual_right).contains(&screen_x)
        || !(virtual_y..virtual_bottom).contains(&screen_y)
    {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "the mapped pointer coordinate is outside the visible virtual desktop",
        ));
    }
    let absolute_x = ((screen_x - virtual_x) as i64 * 65_535 / (virtual_width - 1) as i64)
        .clamp(0, 65_535) as i32;
    let absolute_y = ((screen_y - virtual_y) as i64 * 65_535 / (virtual_height - 1) as i64)
        .clamp(0, 65_535) as i32;
    Ok(MappedPointerPoint {
        screen_x,
        screen_y,
        absolute_x,
        absolute_y,
    })
}

fn virtual_desktop_geometry() -> [i32; 4] {
    [
        unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
        unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
        unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(2),
        unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(2),
    ]
}

fn preflight_drag_path_for_desktop(
    observation: &ComputerUseObservation,
    path: &[ComputerUsePoint],
    duration_ms: u64,
    virtual_desktop: [i32; 4],
) -> ComputerUseResult<Vec<MappedPointerPoint>> {
    if path.len() < 2 {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "drag requires at least two path points",
        ));
    }
    std::iter::once(path[0])
        .chain(interpolated_drag_path(path, duration_ms))
        .map(|point| mapped_pointer_point_for_desktop(observation, point, virtual_desktop))
        .collect()
}

fn screenshot_point_to_screen(
    point: ComputerUsePoint,
    screenshot_size: [u32; 2],
    source_rect: [i32; 4],
) -> Option<(i32, i32)> {
    let [width, height] = screenshot_size;
    let [left, top, rect_width, rect_height] = source_rect;
    if width == 0
        || height == 0
        || rect_width <= 0
        || rect_height <= 0
        || !point.x.is_finite()
        || !point.y.is_finite()
        || !(0.0..width as f64).contains(&point.x)
        || !(0.0..height as f64).contains(&point.y)
    {
        return None;
    }

    let map_axis = |value: f64, image_extent: u32, origin: i32, rect_extent: i32| {
        let offset = (value * f64::from(rect_extent) / f64::from(image_extent)).floor() as i64;
        let min = i64::from(origin);
        let max = min + i64::from(rect_extent) - 1;
        (min + offset).clamp(min, max) as i32
    };
    Some((
        map_axis(point.x, width, left, rect_width),
        map_axis(point.y, height, top, rect_height),
    ))
}

fn click(
    window_handle: u64,
    observation: &ComputerUseObservation,
    screen: (i32, i32),
    request: &ComputerUseAction,
    click_count: usize,
    guard: &ActionGuard<'_>,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<()> {
    let (screen_x, screen_y) = screen;
    guard.synchronize()?;
    let inputs = click_inputs(request.button.as_deref().unwrap_or("left"))?;
    ensure_target_foreground(window_handle, observation.process_id)?;
    ensure_observation_target(window_handle, observation)?;
    ensure_cursor_at(screen_x, screen_y)?;
    let _elevation = prepare_point_target(
        screen_x,
        screen_y,
        HWND(window_handle as *mut core::ffi::c_void),
        observation.process_id,
    )?;
    guard.check()?;
    run_pre_input_fence(pre_input_fence)?;
    let mut held_keys = HeldKeys::press(&request.keys)?;
    for index in 0..click_count {
        if index != 0 {
            guard.sleep(Duration::from_millis(60))?;
            guard.synchronize()?;
            ensure_target_foreground(window_handle, observation.process_id)?;
            ensure_observation_target(window_handle, observation)?;
            ensure_cursor_at(screen_x, screen_y)?;
            let _elevation = prepare_point_target(
                screen_x,
                screen_y,
                HWND(window_handle as *mut core::ffi::c_void),
                observation.process_id,
            )?;
            guard.check()?;
            run_pre_input_fence(pre_input_fence)?;
        }
        send_inputs(&inputs)?;
        guard.check()?;
    }
    held_keys.release()?;
    guard.check()
}

fn drag(
    window_handle: u64,
    observation: &ComputerUseObservation,
    request: &ComputerUseAction,
    guard: &ActionGuard<'_>,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<()> {
    let duration_ms = request.duration_ms.unwrap_or(0);
    // Complete mapping is a no-input preflight: a late invalid point cannot
    // turn a partial drag into a click or edit.
    let mapped_path = preflight_drag_path_for_desktop(
        observation,
        &request.path,
        duration_ms,
        virtual_desktop_geometry(),
    )?;
    let Some((start, drag_path)) = mapped_path.split_first() else {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "drag requires at least two path points",
        ));
    };
    let button = request.button.as_deref().unwrap_or("left");
    let (screen_x, screen_y) = move_to_mapped(
        window_handle,
        observation,
        *start,
        guard,
        true,
        pre_input_fence,
    )?;
    let effect = PointerEffect::new(screen_x, screen_y, "◆")?;
    guard.synchronize()?;
    ensure_target_foreground(window_handle, observation.process_id)?;
    ensure_observation_target(window_handle, observation)?;
    ensure_cursor_at(screen_x, screen_y)?;
    let _elevation = prepare_point_target(
        screen_x,
        screen_y,
        HWND(window_handle as *mut core::ffi::c_void),
        observation.process_id,
    )?;
    let step_count = drag_path.len();
    guard.check()?;
    run_pre_input_fence(pre_input_fence)?;
    let mut held_keys = HeldKeys::press(&request.keys)?;
    let mut held_button = HeldMouseButton::press(button)?;
    let started = Instant::now();
    let mut previous_screen = (screen_x, screen_y);
    for (index, point) in drag_path.iter().copied().enumerate() {
        let step = index + 1;
        let deadline = started
            + Duration::from_millis(duration_ms.saturating_mul(step as u64) / step_count as u64);
        guard.sleep(deadline.saturating_duration_since(Instant::now()))?;
        guard.check()?;
        ensure_target_foreground(window_handle, observation.process_id)?;
        ensure_observation_target(window_handle, observation)?;
        ensure_cursor_at(previous_screen.0, previous_screen.1)?;
        let _step_elevation = prepare_point_target(
            point.screen_x,
            point.screen_y,
            HWND(window_handle as *mut core::ffi::c_void),
            observation.process_id,
        )?;
        guard.check()?;
        send_mouse(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            point.absolute_x,
            point.absolute_y,
            0,
        )?;
        previous_screen = (point.screen_x, point.screen_y);
        effect.reposition(point.screen_x, point.screen_y)?;
    }
    held_button.release()?;
    held_keys.release()?;
    guard.check()?;
    effect.dwell(
        guard,
        Duration::from_millis(DEFAULT_POINTER_EFFECT_DWELL_MS),
    )
}

fn scroll(
    window_handle: u64,
    observation: &ComputerUseObservation,
    screen_x: i32,
    screen_y: i32,
    request: &ComputerUseAction,
    guard: &ActionGuard<'_>,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<()> {
    let horizontal = request.scroll_x.unwrap_or(0);
    let vertical = request.scroll_y.unwrap_or(0);
    let mut held_keys = None;
    if vertical != 0 {
        guard.synchronize()?;
        ensure_target_foreground(window_handle, observation.process_id)?;
        ensure_observation_target(window_handle, observation)?;
        ensure_cursor_at(screen_x, screen_y)?;
        let _elevation = prepare_point_target(
            screen_x,
            screen_y,
            HWND(window_handle as *mut core::ffi::c_void),
            observation.process_id,
        )?;
        run_pre_input_fence(pre_input_fence)?;
        held_keys = Some(HeldKeys::press(&request.keys)?);
        send_mouse(MOUSEEVENTF_WHEEL, 0, 0, vertical_wheel_data(vertical))?;
    }
    if horizontal != 0 {
        guard.synchronize()?;
        ensure_target_foreground(window_handle, observation.process_id)?;
        ensure_observation_target(window_handle, observation)?;
        ensure_cursor_at(screen_x, screen_y)?;
        let _elevation = prepare_point_target(
            screen_x,
            screen_y,
            HWND(window_handle as *mut core::ffi::c_void),
            observation.process_id,
        )?;
        run_pre_input_fence(pre_input_fence)?;
        if held_keys.is_none() {
            held_keys = Some(HeldKeys::press(&request.keys)?);
        }
        send_mouse(MOUSEEVENTF_HWHEEL, 0, 0, horizontal as u32)?;
    }
    if let Some(mut held_keys) = held_keys {
        held_keys.release()?;
    }
    Ok(())
}

fn vertical_wheel_data(vertical: i32) -> u32 {
    vertical.saturating_neg() as u32
}

fn type_text(
    window_handle: u64,
    process_id: u32,
    text: &str,
    guard: &ActionGuard<'_>,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<()> {
    for unit in text.encode_utf16() {
        guard.synchronize()?;
        ensure_target_foreground(window_handle, process_id)?;
        guard.check()?;
        run_pre_input_fence(pre_input_fence)?;
        send_inputs(&[keyboard_unicode(unit, false), keyboard_unicode(unit, true)])?;
        guard.check()?;
    }
    Ok(())
}

fn keypress(
    window_handle: u64,
    process_id: u32,
    keys: &[String],
    guard: &ActionGuard<'_>,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<()> {
    let inputs = keypress_inputs(keys)?;
    guard.synchronize()?;
    // This is the first and only input in a keypress action, so reacquiring the
    // validated target after the barrier is safe. Multi-step actions continue
    // to fail closed if focus changes after their first input.
    let _focus_elevation = focus_target(window_handle, process_id)?;
    guard.check()?;
    run_pre_input_fence(pre_input_fence)?;
    send_inputs(&inputs)?;
    guard.check()
}

fn run_pre_input_fence(
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<()> {
    if let Some(fence) = pre_input_fence.as_mut() {
        (**fence)()?;
    }
    Ok(())
}

fn keypress_inputs(keys: &[String]) -> ComputerUseResult<Vec<INPUT>> {
    if crate::keyboard_policy::is_unmodified_text_entry(keys) {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "keypress cannot synthesize ordinary text; use the separately governed text-entry path",
        ));
    }
    if crate::keyboard_policy::shortcut_risk(keys) == crate::keyboard_policy::ShortcutRisk::HardDeny
    {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "system or scope-escape keyboard shortcuts are not allowed in Computer Use",
        ));
    }
    let (mut inputs, releases) = held_key_inputs(keys)?;
    if inputs.is_empty() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "keypress requires at least one key",
        ));
    }
    inputs.extend(releases);
    Ok(inputs)
}

fn held_key_inputs(keys: &[String]) -> ComputerUseResult<(Vec<INPUT>, Vec<INPUT>)> {
    let flattened: Vec<String> = keys
        .iter()
        .flat_map(|item| item.split('+'))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();
    let pressed = flattened
        .iter()
        .map(|key| virtual_key(key))
        .collect::<ComputerUseResult<Vec<_>>>()?;
    let inputs = pressed
        .iter()
        .copied()
        .map(|vk| keyboard_vk(vk, false))
        .collect();
    let releases = pressed
        .iter()
        .rev()
        .copied()
        .map(|vk| keyboard_vk(vk, true))
        .collect();
    Ok((inputs, releases))
}

fn pointer_modifier_inputs(keys: &[String]) -> ComputerUseResult<(Vec<INPUT>, Vec<INPUT>)> {
    let inputs = held_key_inputs(keys)?;
    if inputs.0.iter().any(|input| {
        let virtual_key = unsafe { input.Anonymous.ki.wVk.0 };
        !matches!(virtual_key, 0x10..=0x12 | 0xA0..=0xA5)
    }) {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "pointer action keys only allow Ctrl, Shift, Alt, and their left/right variants",
        ));
    }
    Ok(inputs)
}

fn virtual_key(key: &str) -> ComputerUseResult<VIRTUAL_KEY> {
    let upper = key.to_ascii_uppercase();
    if let Some(common) = crate::keyboard_policy::common_key(key) {
        return common.virtual_key().map(VIRTUAL_KEY).ok_or_else(|| {
            ComputerUseError::new(
                ComputerUseErrorCode::InvalidAction,
                format!("system key {key:?} is not allowed in Computer Use"),
            )
        });
    }
    if let Some(virtual_key) = crate::keyboard_policy::function_key_virtual_key(key) {
        return Ok(VIRTUAL_KEY(virtual_key));
    }
    if let Some(digit) = upper
        .strip_prefix("KP_")
        .filter(|value| value.len() == 1)
        .and_then(|value| value.as_bytes().first().copied())
        .filter(u8::is_ascii_digit)
    {
        return Ok(VIRTUAL_KEY(0x60 + u16::from(digit - b'0')));
    }
    let raw = match upper.as_str() {
        "ENTER" | "RETURN" => 0x0D,
        "BACKSPACE" => 0x08,
        "DELETE" | "DEL" => 0x2E,
        "INSERT" | "INS" => 0x2D,
        "LEFT" | "ARROWLEFT" | "ARROW_LEFT" => 0x25,
        "UP" | "ARROWUP" | "ARROW_UP" => 0x26,
        "RIGHT" | "ARROWRIGHT" | "ARROW_RIGHT" => 0x27,
        "DOWN" | "ARROWDOWN" | "ARROW_DOWN" => 0x28,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" | "PAGE_UP" | "PGUP" => 0x21,
        "PAGEDOWN" | "PAGE_DOWN" | "PGDN" => 0x22,
        "CAPSLOCK" | "CAPS_LOCK" => 0x14,
        "NUMLOCK" | "NUM_LOCK" => 0x90,
        "SCROLLLOCK" | "SCROLL_LOCK" => 0x91,
        "PAUSE" => 0x13,
        "KP_DECIMAL" | "KPDECIMAL" | "NUMPAD_DECIMAL" => 0x6E,
        ";" | "SEMICOLON" => 0xBA,
        "=" | "EQUAL" | "EQUALS" => 0xBB,
        "," | "COMMA" => 0xBC,
        "-" | "MINUS" => 0xBD,
        "." | "PERIOD" | "DOT" => 0xBE,
        "/" | "SLASH" => 0xBF,
        "`" | "GRAVE" | "BACKTICK" => 0xC0,
        "[" | "LEFTBRACKET" | "BRACKETLEFT" => 0xDB,
        "\\" | "BACKSLASH" => 0xDC,
        "]" | "RIGHTBRACKET" | "BRACKETRIGHT" => 0xDD,
        "'" | "APOSTROPHE" | "QUOTE" => 0xDE,
        value
            if value.len() == 1
                && value
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric) =>
        {
            u16::from(value.as_bytes()[0])
        }
        _ => return Err(invalid_key(key)),
    };
    Ok(VIRTUAL_KEY(raw))
}

fn invalid_key(key: &str) -> ComputerUseError {
    ComputerUseError::new(
        ComputerUseErrorCode::InvalidAction,
        format!("unsupported key {key:?}; use type for literal Unicode text"),
    )
}

fn keyboard_vk(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    let mut flags = if extended_virtual_key(vk) {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn extended_virtual_key(vk: VIRTUAL_KEY) -> bool {
    matches!(
        vk.0,
        0x21..=0x28 // Page/Home/End/Arrow keys.
            | 0x2D..=0x2E // Insert/Delete.
            | 0xA3 // Right Control.
            | 0xA5 // Right Alt.
    )
}

fn keyboard_unicode(unit: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wScan: unit,
                dwFlags: if key_up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                ..Default::default()
            },
        },
    }
}

fn button_flags(button: &str) -> ComputerUseResult<(MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS)> {
    match button.to_ascii_lowercase().as_str() {
        "left" | "primary" => Ok((MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)),
        "right" | "secondary" => Ok((MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)),
        "middle" => Ok((MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP)),
        _ => Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            format!("unsupported mouse button {button:?}"),
        )),
    }
}

/// Keys held while a pointer action runs. Releases are ordered in reverse and
/// use the same deferred cleanup fence as held mouse buttons.
struct HeldKeys {
    releases: Vec<INPUT>,
}

impl HeldKeys {
    fn press(keys: &[String]) -> ComputerUseResult<Self> {
        let (presses, releases) = pointer_modifier_inputs(keys)?;
        if !presses.is_empty() {
            send_inputs(&presses)?;
        }
        Ok(Self { releases })
    }

    fn release(&mut self) -> ComputerUseResult<()> {
        let releases = std::mem::take(&mut self.releases);
        if releases.is_empty() {
            return Ok(());
        }
        if let Err(error) = send_inputs(&releases) {
            defer_input_releases(&releases);
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for HeldKeys {
    fn drop(&mut self) {
        let releases = std::mem::take(&mut self.releases);
        if !releases.is_empty() && send_inputs(&releases).is_err() {
            defer_input_releases(&releases);
        }
    }
}

/// A mouse button held across a drag operation.
///
/// # Drag-only use
///
/// This type intentionally splits the DOWN and UP events across two separate
/// `SendInput` calls because drag operations require meaningful time between
/// press and release (cursor movement happens in between). For click-like
/// operations that don't need a held period, use `click_inputs()` which batches
/// DOWN+UP into a single atomic `SendInput` call — see `click()` above.
///
/// # Accepted risk and bounded cleanup
///
/// Because DOWN and UP are in separate calls, the desktop could become
/// unavailable between them, leaving a button held in the kernel input state.
/// Two mechanisms bound this risk:
///
/// 1. `release()` / `Drop` — attempts `SendInput` for the UP event and, on
///    failure, defers it to `PENDING_INPUT_RELEASES` for later retry.
/// 2. `InputOwnerLease::drop()` — on session teardown, retries
///    `flush_pending_input_releases_locked()` with a hard 5-second deadline
///    before unconditionally releasing the cross-process named mutex.
///
/// These two layers together ensure that a stuck drag cannot block new Computer
/// Use sessions indefinitely.
struct HeldMouseButton {
    release: Option<INPUT>,
}

impl HeldMouseButton {
    fn press(button: &str) -> ComputerUseResult<Self> {
        let (down, up) = button_flags(button)?;
        send_inputs(&[mouse_input(down, 0, 0, 0)])?;
        Ok(Self {
            release: Some(mouse_input(up, 0, 0, 0)),
        })
    }

    fn release(&mut self) -> ComputerUseResult<()> {
        let Some(release) = self.release.take() else {
            return Ok(());
        };
        if let Err(error) = send_inputs(&[release]) {
            defer_input_releases(&[release]);
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for HeldMouseButton {
    fn drop(&mut self) {
        let Some(release) = self.release.take() else {
            return;
        };
        if send_inputs(&[release]).is_err() {
            defer_input_releases(&[release]);
        }
    }
}

fn send_mouse(
    flags: MOUSE_EVENT_FLAGS,
    dx: i32,
    dy: i32,
    mouse_data: u32,
) -> ComputerUseResult<()> {
    send_inputs(&[mouse_input(flags, dx, dy, mouse_data)])
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, mouse_data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: mouse_data,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn click_inputs(button: &str) -> ComputerUseResult<[INPUT; 2]> {
    let (down, up) = button_flags(button)?;
    Ok([mouse_input(down, 0, 0, 0), mouse_input(up, 0, 0, 0)])
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
