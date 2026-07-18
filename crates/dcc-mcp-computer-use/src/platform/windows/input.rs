use super::*;

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
            false,
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
) -> ComputerUseResult<()> {
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
    focus_target(window_handle, observation.process_id)?;
    guard.check()?;
    ensure_observation_target(window_handle, observation)?;

    match request.action.as_str() {
        "move" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(window_handle, observation, point, &guard, true)?;
            let effect = PointerEffect::new(screen_x, screen_y, "●")?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "click" | "raw_coordinate_click" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(window_handle, observation, point, &guard, true)?;
            click(
                window_handle,
                observation,
                screen_x,
                screen_y,
                request.button.as_deref().unwrap_or("left"),
                &guard,
            )?;
            let effect = PointerEffect::new(screen_x, screen_y, "●")?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "double_click" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(window_handle, observation, point, &guard, true)?;
            click(
                window_handle,
                observation,
                screen_x,
                screen_y,
                request.button.as_deref().unwrap_or("left"),
                &guard,
            )?;
            guard.sleep(Duration::from_millis(60))?;
            click(
                window_handle,
                observation,
                screen_x,
                screen_y,
                request.button.as_deref().unwrap_or("left"),
                &guard,
            )?;
            let effect = PointerEffect::new(screen_x, screen_y, "◎")?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "scroll" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(window_handle, observation, point, &guard, true)?;
            scroll(
                window_handle,
                observation,
                screen_x,
                screen_y,
                request.scroll_x.unwrap_or(0),
                request.scroll_y.unwrap_or(0),
                &guard,
            )?;
            let effect = PointerEffect::new(screen_x, screen_y, "↕")?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "drag" => drag(window_handle, observation, request, &guard)?,
        "type" => type_text(
            window_handle,
            observation.process_id,
            request.text.as_deref().unwrap_or(""),
            &guard,
        )?,
        "keypress" | "keyboard_shortcut" => {
            keypress(window_handle, observation.process_id, &request.keys, &guard)?
        }
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

fn focus_target(window_handle: u64, process_id: u32) -> ComputerUseResult<()> {
    ensure_interactive_desktop()?;
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    restore_target_for_input(hwnd, process_id)?;
    let _ = available_target_rect_for_process(hwnd, process_id)?;
    if unsafe { GetForegroundWindow() } == hwnd {
        return Ok(());
    }

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
    thread::sleep(Duration::from_millis(30));
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::FocusLost,
            "the scoped DCC window did not remain in the foreground",
        ));
    }
    Ok(())
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

fn ensure_point_targets_window(
    screen_x: i32,
    screen_y: i32,
    target: HWND,
    process_id: u32,
) -> ComputerUseResult<()> {
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
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "the requested pointer coordinate is occluded by another process",
        ));
    }
    let hit_root = unsafe { GetAncestor(hit, GA_ROOT) };
    if hit_root != target {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "the requested pointer coordinate is outside the scoped top-level window",
        ));
    }
    Ok(())
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
) -> ComputerUseResult<(i32, i32)> {
    guard.synchronize()?;
    ensure_observation_target(window_handle, observation)?;
    let (screen_x, screen_y, absolute_x, absolute_y) = mapped_pointer_point(observation, point)?;
    // No input has been sent yet. Reacquire the already-scoped target after
    // the desktop-barrier handshake so a caller window cannot steal focus in
    // the small gap between initial preparation and the first pointer move.
    focus_target(window_handle, observation.process_id)?;
    ensure_observation_target(window_handle, observation)?;
    if require_target_hit {
        ensure_point_targets_window(
            screen_x,
            screen_y,
            HWND(window_handle as *mut core::ffi::c_void),
            observation.process_id,
        )?;
    }
    guard.check()?;
    send_mouse(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        absolute_x,
        absolute_y,
        0,
    )?;
    Ok((screen_x, screen_y))
}

fn mapped_pointer_point(
    observation: &ComputerUseObservation,
    point: ComputerUsePoint,
) -> ComputerUseResult<(i32, i32, i32, i32)> {
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
    let virtual_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let virtual_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let virtual_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(2);
    let virtual_height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(2);
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
    Ok((screen_x, screen_y, absolute_x, absolute_y))
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
    screen_x: i32,
    screen_y: i32,
    button: &str,
    guard: &ActionGuard<'_>,
) -> ComputerUseResult<()> {
    guard.synchronize()?;
    let inputs = click_inputs(button)?;
    ensure_target_foreground(window_handle, observation.process_id)?;
    ensure_observation_target(window_handle, observation)?;
    ensure_cursor_at(screen_x, screen_y)?;
    ensure_point_targets_window(
        screen_x,
        screen_y,
        HWND(window_handle as *mut core::ffi::c_void),
        observation.process_id,
    )?;
    guard.check()?;
    send_inputs(&inputs)?;
    guard.check()
}

fn drag(
    window_handle: u64,
    observation: &ComputerUseObservation,
    request: &ComputerUseAction,
    guard: &ActionGuard<'_>,
) -> ComputerUseResult<()> {
    if request.path.len() < 2 {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "drag requires at least two path points",
        ));
    }
    let button = request.button.as_deref().unwrap_or("left");
    let (screen_x, screen_y) = move_to(window_handle, observation, request.path[0], guard, true)?;
    let effect = PointerEffect::new(screen_x, screen_y, "◆")?;
    guard.synchronize()?;
    ensure_target_foreground(window_handle, observation.process_id)?;
    ensure_observation_target(window_handle, observation)?;
    ensure_cursor_at(screen_x, screen_y)?;
    ensure_point_targets_window(
        screen_x,
        screen_y,
        HWND(window_handle as *mut core::ffi::c_void),
        observation.process_id,
    )?;
    let duration_ms = request.duration_ms.unwrap_or(0);
    let drag_path = interpolated_drag_path(&request.path, duration_ms);
    let step_count = drag_path.len();
    guard.check()?;
    let mut held_button = HeldMouseButton::press(button)?;
    let started = Instant::now();
    let mut previous_screen = (screen_x, screen_y);
    for (index, point) in drag_path.into_iter().enumerate() {
        let step = index + 1;
        let deadline = started
            + Duration::from_millis(duration_ms.saturating_mul(step as u64) / step_count as u64);
        guard.sleep(deadline.saturating_duration_since(Instant::now()))?;
        guard.check()?;
        ensure_target_foreground(window_handle, observation.process_id)?;
        ensure_observation_target(window_handle, observation)?;
        ensure_cursor_at(previous_screen.0, previous_screen.1)?;
        let (mapped_x, mapped_y, absolute_x, absolute_y) =
            mapped_pointer_point(observation, point)?;
        ensure_point_targets_window(
            mapped_x,
            mapped_y,
            HWND(window_handle as *mut core::ffi::c_void),
            observation.process_id,
        )?;
        guard.check()?;
        send_mouse(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            absolute_x,
            absolute_y,
            0,
        )?;
        previous_screen = (mapped_x, mapped_y);
        effect.reposition(mapped_x, mapped_y)?;
    }
    held_button.release()?;
    guard.check()?;
    effect.dwell(
        guard,
        Duration::from_millis(DEFAULT_POINTER_EFFECT_DWELL_MS),
    )
}

fn drag_step_count(path_len: usize, duration_ms: u64) -> usize {
    path_len
        .saturating_sub(1)
        .max(duration_ms.div_ceil(DRAG_UPDATE_INTERVAL_MS) as usize)
}

fn interpolated_drag_path(path: &[ComputerUsePoint], duration_ms: u64) -> Vec<ComputerUsePoint> {
    let segment_count = path.len() - 1;
    let step_count = drag_step_count(path.len(), duration_ms);
    let mut points = Vec::with_capacity(step_count);
    let mut allocated = 0;
    for segment in 0..segment_count {
        let segment_end = (segment + 1) * step_count / segment_count;
        let segment_steps = segment_end - allocated;
        let from = path[segment];
        let to = path[segment + 1];
        for step in 1..=segment_steps {
            let fraction = step as f64 / segment_steps as f64;
            points.push(ComputerUsePoint {
                x: from.x + (to.x - from.x) * fraction,
                y: from.y + (to.y - from.y) * fraction,
            });
        }
        allocated = segment_end;
    }
    points
}

fn scroll(
    window_handle: u64,
    observation: &ComputerUseObservation,
    screen_x: i32,
    screen_y: i32,
    horizontal: i32,
    vertical: i32,
    guard: &ActionGuard<'_>,
) -> ComputerUseResult<()> {
    if vertical != 0 {
        guard.synchronize()?;
        ensure_target_foreground(window_handle, observation.process_id)?;
        ensure_observation_target(window_handle, observation)?;
        ensure_cursor_at(screen_x, screen_y)?;
        ensure_point_targets_window(
            screen_x,
            screen_y,
            HWND(window_handle as *mut core::ffi::c_void),
            observation.process_id,
        )?;
        send_mouse(MOUSEEVENTF_WHEEL, 0, 0, vertical_wheel_data(vertical))?;
    }
    if horizontal != 0 {
        guard.synchronize()?;
        ensure_target_foreground(window_handle, observation.process_id)?;
        ensure_observation_target(window_handle, observation)?;
        ensure_cursor_at(screen_x, screen_y)?;
        ensure_point_targets_window(
            screen_x,
            screen_y,
            HWND(window_handle as *mut core::ffi::c_void),
            observation.process_id,
        )?;
        send_mouse(MOUSEEVENTF_HWHEEL, 0, 0, horizontal as u32)?;
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
) -> ComputerUseResult<()> {
    for unit in text.encode_utf16() {
        guard.synchronize()?;
        ensure_target_foreground(window_handle, process_id)?;
        guard.check()?;
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
) -> ComputerUseResult<()> {
    let inputs = keypress_inputs(keys)?;
    guard.synchronize()?;
    // This is the first and only input in a keypress action, so reacquiring the
    // validated target after the barrier is safe. Multi-step actions continue
    // to fail closed if focus changes after their first input.
    focus_target(window_handle, process_id)?;
    guard.check()?;
    send_inputs(&inputs)?;
    guard.check()
}

fn keypress_inputs(keys: &[String]) -> ComputerUseResult<Vec<INPUT>> {
    let flattened: Vec<String> = keys
        .iter()
        .flat_map(|item| item.split('+'))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();
    if flattened.is_empty() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "keypress requires at least one key",
        ));
    }
    let pressed = flattened
        .iter()
        .map(|key| virtual_key(key))
        .collect::<ComputerUseResult<Vec<_>>>()?;
    let mut inputs = Vec::with_capacity(pressed.len() * 2);
    inputs.extend(pressed.iter().copied().map(|vk| keyboard_vk(vk, false)));
    inputs.extend(
        pressed
            .iter()
            .rev()
            .copied()
            .map(|vk| keyboard_vk(vk, true)),
    );
    Ok(inputs)
}

fn virtual_key(key: &str) -> ComputerUseResult<VIRTUAL_KEY> {
    let upper = key.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "META" | "WIN" | "WINDOWS" | "SUPER" | "CMD" | "COMMAND"
    ) {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            format!("system key {key:?} is not allowed in Computer Use"),
        ));
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
        "CTRL" | "CONTROL" => 0x11,
        "LCTRL" | "LEFTCTRL" | "LEFT_CTRL" | "CTRL_L" | "CONTROL_L" => 0xA2,
        "RCTRL" | "RIGHTCTRL" | "RIGHT_CTRL" | "CTRL_R" | "CONTROL_R" => 0xA3,
        "SHIFT" => 0x10,
        "LSHIFT" | "LEFTSHIFT" | "LEFT_SHIFT" | "SHIFT_L" => 0xA0,
        "RSHIFT" | "RIGHTSHIFT" | "RIGHT_SHIFT" | "SHIFT_R" => 0xA1,
        "ALT" => 0x12,
        "LALT" | "LEFTALT" | "LEFT_ALT" | "ALT_L" => 0xA4,
        "RALT" | "RIGHTALT" | "RIGHT_ALT" | "ALT_R" | "ALTGR" => 0xA5,
        "ENTER" | "RETURN" => 0x0D,
        "TAB" => 0x09,
        "BACKSPACE" => 0x08,
        "DELETE" | "DEL" => 0x2E,
        "INSERT" | "INS" => 0x2D,
        "SPACE" => 0x20,
        "LEFT" | "ARROWLEFT" | "ARROW_LEFT" => 0x25,
        "UP" | "ARROWUP" | "ARROW_UP" => 0x26,
        "RIGHT" | "ARROWRIGHT" | "ARROW_RIGHT" => 0x27,
        "DOWN" | "ARROWDOWN" | "ARROW_DOWN" => 0x28,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" | "PAGE_UP" | "PGUP" => 0x21,
        "PAGEDOWN" | "PAGE_DOWN" | "PGDN" => 0x22,
        "ESC" | "ESCAPE" => 0x1B,
        "CAPSLOCK" | "CAPS_LOCK" => 0x14,
        "NUMLOCK" | "NUM_LOCK" => 0x90,
        "SCROLLLOCK" | "SCROLL_LOCK" => 0x91,
        "PRINTSCREEN" | "PRINT_SCREEN" | "PRTSC" => 0x2C,
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
        value if value.starts_with('F') => value[1..]
            .parse::<u16>()
            .ok()
            .filter(|number| (1..=24).contains(number))
            .map(|number| 0x70 + number - 1)
            .ok_or_else(|| invalid_key(key))?,
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

fn send_inputs(inputs: &[INPUT]) -> ComputerUseResult<()> {
    send_inputs_with(
        inputs,
        |batch| unsafe { SendInput(batch, size_of::<INPUT>() as i32) },
        desktop_interactive,
        defer_input_releases,
    )
}

pub(crate) fn flush_pending_input_releases() -> ComputerUseResult<()> {
    let _input_guard = INPUT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    flush_pending_input_releases_locked()
}

pub(super) fn flush_pending_input_releases_locked() -> ComputerUseResult<()> {
    let mut pending = PENDING_INPUT_RELEASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    flush_pending_input_releases_with(
        &mut pending,
        |batch| unsafe { SendInput(batch, size_of::<INPUT>() as i32) },
        desktop_interactive,
    )
}

fn defer_input_releases(releases: &[INPUT]) {
    if releases.is_empty() {
        return;
    }
    PENDING_INPUT_RELEASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .extend_from_slice(releases);
}

fn flush_pending_input_releases_with(
    pending: &mut Vec<INPUT>,
    mut inject: impl FnMut(&[INPUT]) -> u32,
    mut is_desktop_interactive: impl FnMut() -> bool,
) -> ComputerUseResult<()> {
    if pending.is_empty() {
        return Ok(());
    }
    require_interactive_desktop(is_desktop_interactive())?;
    let original_count = pending.len();
    while !pending.is_empty() {
        if !is_desktop_interactive() {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::DesktopUnavailable,
                format!(
                    "the Windows desktop is unavailable with {} pending input-release events; no new input may run until they are confirmed",
                    pending.len()
                ),
            ));
        }
        let released = (inject(pending) as usize).min(pending.len());
        if released == 0 {
            let code = if is_desktop_interactive() {
                ComputerUseErrorCode::InputFailed
            } else {
                ComputerUseErrorCode::DesktopUnavailable
            };
            return Err(ComputerUseError::new(
                code,
                format!(
                    "Windows could not confirm {} of {original_count} pending input-release events; no new input was sent",
                    pending.len()
                ),
            ));
        }
        pending.drain(..released);
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PressedInput {
    LeftMouse,
    RightMouse,
    MiddleMouse,
    Keyboard {
        virtual_key: u16,
        scan: u16,
        unicode: bool,
    },
}

fn update_pressed_inputs(pressed: &mut Vec<PressedInput>, item: PressedInput, released: bool) {
    if released {
        if let Some(index) = pressed.iter().rposition(|candidate| *candidate == item) {
            pressed.remove(index);
        }
    } else {
        pressed.push(item);
    }
}

fn compensating_releases(inserted: &[INPUT]) -> Vec<INPUT> {
    let mut pressed = Vec::new();
    for input in inserted {
        if input.r#type == INPUT_KEYBOARD {
            let keyboard = unsafe { input.Anonymous.ki };
            update_pressed_inputs(
                &mut pressed,
                PressedInput::Keyboard {
                    virtual_key: keyboard.wVk.0,
                    scan: keyboard.wScan,
                    unicode: keyboard.dwFlags.contains(KEYEVENTF_UNICODE),
                },
                keyboard.dwFlags.contains(KEYEVENTF_KEYUP),
            );
        } else if input.r#type == INPUT_MOUSE {
            let flags = unsafe { input.Anonymous.mi.dwFlags };
            for (down, up, item) in [
                (
                    MOUSEEVENTF_LEFTDOWN,
                    MOUSEEVENTF_LEFTUP,
                    PressedInput::LeftMouse,
                ),
                (
                    MOUSEEVENTF_RIGHTDOWN,
                    MOUSEEVENTF_RIGHTUP,
                    PressedInput::RightMouse,
                ),
                (
                    MOUSEEVENTF_MIDDLEDOWN,
                    MOUSEEVENTF_MIDDLEUP,
                    PressedInput::MiddleMouse,
                ),
            ] {
                if flags.contains(down) {
                    update_pressed_inputs(&mut pressed, item, false);
                }
                if flags.contains(up) {
                    update_pressed_inputs(&mut pressed, item, true);
                }
            }
        }
    }
    pressed
        .into_iter()
        .rev()
        .map(|item| match item {
            PressedInput::LeftMouse => mouse_input(MOUSEEVENTF_LEFTUP, 0, 0, 0),
            PressedInput::RightMouse => mouse_input(MOUSEEVENTF_RIGHTUP, 0, 0, 0),
            PressedInput::MiddleMouse => mouse_input(MOUSEEVENTF_MIDDLEUP, 0, 0, 0),
            PressedInput::Keyboard {
                virtual_key,
                scan,
                unicode,
            } => {
                if unicode {
                    keyboard_unicode(scan, true)
                } else {
                    keyboard_vk(VIRTUAL_KEY(virtual_key), true)
                }
            }
        })
        .collect()
}

fn send_inputs_with(
    inputs: &[INPUT],
    mut inject: impl FnMut(&[INPUT]) -> u32,
    mut is_desktop_interactive: impl FnMut() -> bool,
    mut defer_releases: impl FnMut(&[INPUT]),
) -> ComputerUseResult<()> {
    require_interactive_desktop(is_desktop_interactive())?;
    let sent = inject(inputs);
    if sent == inputs.len() as u32 {
        return Ok(());
    }

    let inserted_count = (sent as usize).min(inputs.len());
    let releases = compensating_releases(&inputs[..inserted_count]);
    if !is_desktop_interactive() {
        defer_releases(&releases);
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::DesktopUnavailable,
            format!(
                "the Windows desktop became unavailable after SendInput inserted {inserted_count} of {} events; {} release events are pending and block new input until confirmed",
                inputs.len(),
                releases.len()
            ),
        ));
    }

    let mut released_count = 0_usize;
    while released_count < releases.len() {
        if !is_desktop_interactive() {
            defer_releases(&releases[released_count..]);
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::DesktopUnavailable,
                format!(
                    "the Windows desktop became unavailable after SendInput inserted {inserted_count} of {} events; only {released_count} of {} compensating releases were confirmed and the remainder block new input",
                    inputs.len(),
                    releases.len()
                ),
            ));
        }
        let released =
            (inject(&releases[released_count..]) as usize).min(releases.len() - released_count);
        if released == 0 {
            break;
        }
        released_count += released;
    }
    let cleanup_confirmed = released_count == releases.len();
    if !cleanup_confirmed {
        defer_releases(&releases[released_count..]);
    }
    if !is_desktop_interactive() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::DesktopUnavailable,
            format!(
                "the Windows desktop became unavailable after SendInput inserted {inserted_count} of {} events; unconfirmed compensating releases block new input",
                inputs.len()
            ),
        ));
    }

    let cleanup = if releases.is_empty() {
        "no pressed input required compensation"
    } else if cleanup_confirmed {
        "compensating release events were sent"
    } else {
        "compensating release events could not be confirmed"
    };
    Err(ComputerUseError::new(
        ComputerUseErrorCode::InputFailed,
        format!(
            "SendInput inserted {inserted_count} of {} events; {cleanup}. Windows does not identify whether a short write was caused by UIPI, a desktop transition, or another input policy; take a fresh screenshot before retrying",
            inputs.len()
        ),
    ))
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
