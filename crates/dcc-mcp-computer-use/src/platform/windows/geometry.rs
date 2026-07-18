use super::*;

pub(super) fn available_target_rect_for_process(
    target: HWND,
    expected_process_id: u32,
) -> ComputerUseResult<RECT> {
    validate_target_identity(target, expected_process_id)?;
    available_target_rect(target)
}

pub(super) fn restore_target_for_input(
    target: HWND,
    expected_process_id: u32,
) -> ComputerUseResult<()> {
    validate_target_identity(target, expected_process_id)?;
    if !unsafe { IsIconic(target) }.as_bool() {
        return Ok(());
    }

    let _ = unsafe { ShowWindow(target, SW_RESTORE) };
    let deadline = Instant::now() + TARGET_RESTORE_TIMEOUT;
    let mut previous_rect = None;
    loop {
        ensure_interactive_desktop()?;
        validate_target_identity(target, expected_process_id)?;
        if Instant::now() >= deadline {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::FocusLost,
                "the scoped DCC window could not be restored before input",
            ));
        }
        if !unsafe { IsIconic(target) }.as_bool()
            && let Ok(rect) = available_target_rect_for_process(target, expected_process_id)
        {
            let current_rect = [
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ];
            if previous_rect == Some(current_rect) {
                return Ok(());
            }
            previous_rect = Some(current_rect);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn validate_target_identity(
    target: HWND,
    expected_process_id: u32,
) -> ComputerUseResult<()> {
    if !unsafe { IsWindow(Some(target)) }.as_bool() {
        return Err(target_unavailable());
    }
    let mut actual_process_id = 0_u32;
    unsafe { GetWindowThreadProcessId(target, Some(&mut actual_process_id)) };
    if actual_process_id != expected_process_id {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "the scoped HWND was reused by another process",
        ));
    }
    Ok(())
}

pub(super) fn available_target_rect(target: HWND) -> ComputerUseResult<RECT> {
    if !unsafe { IsWindow(Some(target)) }.as_bool()
        || !unsafe { IsWindowVisible(target) }.as_bool()
        || unsafe { IsIconic(target) }.as_bool()
    {
        return Err(target_unavailable());
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(target, &mut rect) }.map_err(|error| {
        ComputerUseError::new(
            ComputerUseErrorCode::MissingWindow,
            format!("the scoped DCC window is unavailable: {error}"),
        )
    })?;
    if !rect_has_positive_area(&rect) || !rect_intersects_monitor(&rect) {
        return Err(target_unavailable());
    }
    Ok(rect)
}

pub(super) fn rect_has_positive_area(rect: &RECT) -> bool {
    rect.right > rect.left && rect.bottom > rect.top
}

fn monitor_for_rect(rect: &RECT) -> Option<HMONITOR> {
    let monitor = unsafe { MonitorFromRect(rect, MONITOR_DEFAULTTONULL) };
    (!monitor.is_invalid()).then_some(monitor)
}

pub(super) fn rect_intersects_monitor(rect: &RECT) -> bool {
    monitor_for_rect(rect).is_some()
}

fn monitor_work_area(rect: &RECT) -> Option<(HMONITOR, RECT)> {
    let monitor = monitor_for_rect(rect)?;
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
        return None;
    }
    let work = info.rcWork;
    let area = if work.right > work.left && work.bottom > work.top {
        work
    } else {
        info.rcMonitor
    };
    Some((monitor, area))
}

pub(super) fn monitor_dpi(monitor: HMONITOR, target: Option<HWND>) -> u32 {
    let mut dpi_x = 0_u32;
    let mut dpi_y = 0_u32;
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y) }
        .is_ok()
        && dpi_x != 0
    {
        return dpi_x;
    }
    target
        .map(|hwnd| unsafe { GetDpiForWindow(hwnd) })
        .filter(|dpi| *dpi != 0)
        .unwrap_or(96)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct MonitorStamp {
    monitor_rect: [i32; 4],
    work_rect: [i32; 4],
    dpi: u32,
}

unsafe extern "system" fn collect_monitor_stamp(
    monitor: HMONITOR,
    _device_context: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let Some(stamps) = (unsafe { (data.0 as *mut Vec<MonitorStamp>).as_mut() }) else {
        return false.into();
    };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
        return true.into();
    }
    stamps.push(MonitorStamp {
        monitor_rect: [
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right,
            info.rcMonitor.bottom,
        ],
        work_rect: [
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right,
            info.rcWork.bottom,
        ],
        dpi: monitor_dpi(monitor, None),
    });
    true.into()
}

pub(super) fn display_environment_stamp() -> ComputerUseResult<Vec<MonitorStamp>> {
    let mut stamps = Vec::new();
    let enumerated = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor_stamp),
            LPARAM((&raw mut stamps) as isize),
        )
    };
    if !enumerated.as_bool() || stamps.is_empty() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::DesktopUnavailable,
            "Windows did not report an interactive monitor topology",
        ));
    }
    stamps.sort_unstable();
    Ok(stamps)
}

pub(super) fn scaled_pixels(pixels: i32, dpi: u32) -> i32 {
    let scaled = (i64::from(pixels) * i64::from(dpi.max(96)) + 48) / 96;
    scaled.clamp(1, i64::from(i32::MAX)) as i32
}

pub(super) fn breathing_alpha(base_alpha: u8, floor_percent: u8, elapsed_ms: u64) -> u8 {
    let floor = f32::from(floor_percent.min(100)) / 100.0;
    let phase = (elapsed_ms % CONTROL_PULSE_PERIOD_MS) as f32 / CONTROL_PULSE_PERIOD_MS as f32;
    let wave = 0.5 - 0.5 * (std::f32::consts::TAU * phase).cos();
    let scale = floor + (1.0 - floor) * wave;
    (f32::from(base_alpha) * scale)
        .round()
        .clamp(0.0, f32::from(u8::MAX)) as u8
}

pub(super) fn target_unavailable() -> ComputerUseError {
    ComputerUseError::new(
        ComputerUseErrorCode::MissingWindow,
        "the scoped DCC window is minimized, closed, or unavailable",
    )
}

pub(super) fn overlay_geometries(
    target: HWND,
    target_rect: &RECT,
) -> ComputerUseResult<(OverlayGeometry, BorderGeometries)> {
    let (monitor, display_rect) = monitor_work_area(target_rect).ok_or_else(target_unavailable)?;
    let dpi = monitor_dpi(monitor, Some(target));
    Ok((
        banner_geometry(target_rect, &display_rect, dpi),
        border_geometries(target_rect, dpi),
    ))
}

pub(super) fn banner_geometry(rect: &RECT, display_rect: &RECT, dpi: u32) -> OverlayGeometry {
    let target_width = rect.right.saturating_sub(rect.left).max(1);
    let display_width = display_rect.right.saturating_sub(display_rect.left).max(1);
    let display_height = display_rect.bottom.saturating_sub(display_rect.top).max(1);
    let width = target_width
        .max(scaled_pixels(520, dpi))
        .min(scaled_pixels(1040, dpi))
        .min(display_width);
    let height = scaled_pixels(62, dpi).min(display_height);
    let centered_x = rect
        .left
        .saturating_add(target_width.saturating_sub(width) / 2);
    let x = centered_x.clamp(display_rect.left, display_rect.right.saturating_sub(width));
    let y = rect
        .top
        .saturating_add(scaled_pixels(18, dpi))
        .clamp(display_rect.top, display_rect.bottom.saturating_sub(height));
    (x, y, width, height)
}

pub(super) fn border_geometries(rect: &RECT, dpi: u32) -> BorderGeometries {
    let maximum_thickness = scaled_pixels(BORDER_THICKNESS, dpi);
    let width = rect
        .right
        .saturating_sub(rect.left)
        .max(maximum_thickness.saturating_mul(2));
    let height = rect
        .bottom
        .saturating_sub(rect.top)
        .max(maximum_thickness.saturating_mul(2));
    let layers = [
        (BORDER_THICKNESS, 34_u8, true),
        (36, 42_u8, true),
        (26, 54_u8, true),
        (16, 70_u8, true),
        (7, CONTROL_BORDER_ALPHA, false),
    ];
    layers
        .into_iter()
        .flat_map(|(logical_thickness, alpha, focus)| {
            let thickness = scaled_pixels(logical_thickness, dpi);
            [
                ((rect.left, rect.top, width, thickness), alpha, focus),
                (
                    (
                        rect.left,
                        rect.bottom.saturating_sub(thickness),
                        width,
                        thickness,
                    ),
                    alpha,
                    focus,
                ),
                ((rect.left, rect.top, thickness, height), alpha, focus),
                (
                    (
                        rect.right.saturating_sub(thickness),
                        rect.top,
                        thickness,
                        height,
                    ),
                    alpha,
                    focus,
                ),
            ]
        })
        .collect()
}

pub(super) fn point_in_rect(point: POINT, rect: &RECT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

pub(super) fn pointer_mask_geometry(screen_x: i32, screen_y: i32) -> OverlayGeometry {
    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: screen_x,
                y: screen_y,
            },
            MONITOR_DEFAULTTONULL,
        )
    };
    let dpi = if monitor.is_invalid() {
        96
    } else {
        monitor_dpi(monitor, None)
    };
    let size = scaled_pixels(POINTER_EFFECT_SIZE, dpi);
    let offset = size / 2;
    (
        screen_x.saturating_sub(offset),
        screen_y.saturating_sub(offset),
        size,
        size,
    )
}

pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
