use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    COLORREF, CloseHandle, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_ABANDONED,
    WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CombineRgn, CreateEllipticRgn, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint,
    EnumDisplayMonitors, FW_SEMIBOLD, GetMonitorInfoW, GetStockObject, HBRUSH, HDC, HGDIOBJ,
    HMONITOR, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint, MonitorFromRect, NULL_BRUSH,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, RGN_DIFF, RGN_ERROR, SelectObject, SetBkMode, SetTextColor,
    SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTS_CONNECTSTATE_CLASS, WTS_CURRENT_SESSION, WTSActive,
    WTSConnectState, WTSFreeMemory, WTSQuerySessionInformationW, WTSRegisterSessionNotification,
    WTSUnRegisterSessionNotification,
};
use windows::Win32::System::StationsAndDesktops::{
    GetThreadDesktop, GetUserObjectInformationW, UOI_IO,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, CreateEventW, CreateMutexW, GetCurrentThreadId, OpenProcess,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    ReleaseMutex, ResetEvent, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
    GetDpiForWindow, MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, RegisterHotKey,
    SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GA_ROOT,
    GW_HWNDPREV, GWL_EXSTYLE, GWL_USERDATA, GetAncestor, GetClassInfoW, GetClassNameW,
    GetClientRect, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindow, IsWindowVisible,
    LWA_ALPHA, MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostMessageW, RegisterClassW,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE,
    SW_RESTORE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_PAINT,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT,
    WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
    WindowFromPoint,
};
use windows::core::{BOOL, PCWSTR, PWSTR, w};

#[cfg(test)]
use crate::drag_path::drag_step_count;
use crate::drag_path::interpolated_drag_path;
use crate::{
    ComputerUseAction, ComputerUseError, ComputerUseErrorCode, ComputerUseObservation,
    ComputerUsePoint, ComputerUseResult, PreInputFence, denied_target_reason,
    desktop_state_snapshot, record_desktop_environment_change, record_desktop_transition,
};

use super::{
    ControlBannerSignals, ControlBannerStartError, ControlBannerStartResult, DesktopEventBarrier,
    ScopedWindowOperation, ScopedWindowState,
};
use overlay::ControlOverlay;

static ACTIVE_SESSION: AtomicBool = AtomicBool::new(false);
static USER_INTERRUPTED: AtomicBool = AtomicBool::new(false);
// Mutex-wrapped so that clear_user_interrupt() can re-initialize the event
// after a transient CreateEventW failure (OnceLock would permanently store
// None on the first failure, making every subsequent session fail-closed).
static USER_INTERRUPT_EVENT: Mutex<Option<OwnedKernelHandle>> = Mutex::new(None);
static USER_INTERRUPT_EVENT_FAILED: AtomicBool = AtomicBool::new(false);
static INPUT_LOCK: Mutex<()> = Mutex::new(());
static PENDING_INPUT_RELEASES: Mutex<Vec<INPUT>> = Mutex::new(Vec::new());

const INPUT_OWNER_MUTEX_NAME: &str = "Local\\DccMcpComputerUseInputOwner-v1";
const USER_INTERRUPT_EVENT_NAME: &str = "Local\\DccMcpComputerUseUserInterrupted-v1";
#[cfg(test)]
const TEST_ISOLATION_MUTEX_NAME: &str = "Local\\DccMcpComputerUseTestIsolation-v1";
const HOTKEY_ID: i32 = 0x4443;
const STOP_HOTKEY_LABEL: &str = "Esc";
const STOP_HOTKEY_MODIFIERS: HOT_KEY_MODIFIERS = HOT_KEY_MODIFIERS(0);
const CORNER_GLOW_THICKNESS: i32 = 42;
const CORNER_MID_THICKNESS: i32 = 28;
const CORNER_ACCENT_THICKNESS: i32 = 12;
const CORNER_GLOW_LENGTH: i32 = 232;
const CORNER_MID_LENGTH: i32 = 208;
const CORNER_ACCENT_LENGTH: i32 = 180;
const POINTER_EFFECT_SIZE: i32 = 72;
const POINTER_RING_SIZE: i32 = 52;
const CONTROL_OVERLAY_ALPHA: u8 = 185;
const CONTROL_BORDER_ALPHA: u8 = 232;
const CONTROL_CAPSULE_ALPHA: u8 = 244;
const CONTROL_CAPSULE_GLOW_ALPHAS: [u8; 3] = [44, 78, 118];
const CONTROL_CURSOR_ALPHA: u8 = 226;
const CONTROL_CAPSULE_FONT_SIZE: i32 = 16;
const CONTROL_PULSE_PERIOD_MS: u64 = 3_200;
const CONTROL_BORDER_PULSE_FLOOR_PERCENT: u8 = 88;
const CONTROL_CAPSULE_PULSE_FLOOR_PERCENT: u8 = 94;
const CONTROL_CURSOR_PULSE_FLOOR_PERCENT: u8 = 90;
const CONTROL_ACCENT_COLOR: COLORREF = COLORREF(0x00FF_840A);
const CONTROL_GLOW_COLOR: COLORREF = COLORREF(0x00FA_A560);
const CONTROL_CURSOR_COLOR: COLORREF = COLORREF(0x0043_9FFF);
const CONTROL_OVERLAY_CLASS: PCWSTR = w!("DccMcpComputerUseOverlay");
const CONTROL_GLOW_CLASS: PCWSTR = w!("DccMcpComputerUseGlowOverlay");
const CONTROL_CURSOR_CLASS: PCWSTR = w!("DccMcpComputerUseCursorOverlay");
const LAST_ACTION_DOT_CLASS: PCWSTR = w!("DccMcpComputerUseLastActionDot");
const LAST_ACTION_DOT_SIZE: i32 = 16;
const LAST_ACTION_DOT_FADE_MS: u64 = 2_000;
const CONTROL_SCOPE_ANIMATION_MS: u64 = 1_500;
const DEFAULT_POINTER_EFFECT_DWELL_MS: u64 = 350;
const TARGET_RESTORE_TIMEOUT: Duration = Duration::from_millis(500);
const DESKTOP_BARRIER_MESSAGE: u32 = WM_APP + 0x443;
const DESKTOP_BARRIER_TIMEOUT: Duration = Duration::from_millis(500);
const PROCESS_PATH_CAPACITY: usize = 32_768;

/// 16-color palette for session color coding.
/// Index is selected deterministically from the session_id hash.
const SESSION_PALETTE: [COLORREF; 16] = [
    COLORREF(0x00FF_840A), // orange (original accent)
    COLORREF(0x0043_9FFF), // blue
    COLORREF(0x0016_A34A), // green
    COLORREF(0x00D9_3F3F), // red
    COLORREF(0x00C0_5BF3), // purple
    COLORREF(0x0000_BCD4), // teal
    COLORREF(0x00FF_9800), // amber
    COLORREF(0x00C6_28A8), // pink
    COLORREF(0x008B_C34A), // light green
    COLORREF(0x00FF_5722), // deep orange
    COLORREF(0x0079_55B0), // deep purple
    COLORREF(0x0000_8B8B), // dark cyan
    COLORREF(0x00B8_860B), // dark goldenrod
    COLORREF(0x00E9_1E63), // magenta-pink
    COLORREF(0x000D_47A1), // indigo
    COLORREF(0x00F4_43A5), // rose
];

/// Deterministic color from a session_id string.
fn session_color(session_id: &str) -> COLORREF {
    let hash = session_id
        .bytes()
        .fold(0_u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    SESSION_PALETTE[(hash % SESSION_PALETTE.len() as u64) as usize]
}

/// Derive a lighter glow color from an accent color by blending with white.
fn glow_from_accent(accent: COLORREF) -> COLORREF {
    let r = ((accent.0 & 0xFF) as u32 + 102).min(255);
    let g = (((accent.0 >> 8) & 0xFF) as u32 + 102).min(255);
    let b = (((accent.0 >> 16) & 0xFF) as u32 + 102).min(255);
    COLORREF((b << 16) | (g << 8) | r)
}

/// Derive a cursor ring color from an accent color by rotating channels.
fn cursor_from_accent(accent: COLORREF) -> COLORREF {
    let r = accent.0 & 0xFF;
    let g = (accent.0 >> 8) & 0xFF;
    let b = (accent.0 >> 16) & 0xFF;
    // Rotate RGB -> BRG
    COLORREF((g << 16) | (r << 8) | b)
}

fn is_control_overlay_window(hwnd: HWND) -> bool {
    let mut class_name = [0_u16; 64];
    let length = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
    matches!(
        String::from_utf16_lossy(&class_name[..length]).as_str(),
        "DccMcpComputerUseOverlay"
            | "DccMcpComputerUseGlowOverlay"
            | "DccMcpComputerUseCursorOverlay"
            | "DccMcpComputerUseLastActionDot"
    )
}

fn is_input_transparent_window(hwnd: HWND) -> bool {
    if is_control_overlay_window(hwnd) {
        return true;
    }
    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    ex_style & WS_EX_TRANSPARENT.0 != 0
}

fn input_blocker_identity(hwnd: HWND) -> String {
    let mut process_id = 0_u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    let process = process_name(process_id).unwrap_or_else(|_| format!("process {process_id}"));
    let mut class_name = [0_u16; 128];
    let length = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
    let class_name = String::from_utf16_lossy(&class_name[..length]);
    if class_name.is_empty() {
        process
    } else {
        format!("{process} / {class_name}")
    }
}

fn first_input_receiving_window_above_target_at_point(
    target: HWND,
    screen_x: i32,
    screen_y: i32,
) -> Option<HWND> {
    let mut candidate = unsafe { GetWindow(target, GW_HWNDPREV) }.ok();
    while let Some(hwnd) = candidate {
        if unsafe { IsWindowVisible(hwnd).as_bool() } && !is_input_transparent_window(hwnd) {
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok()
                && screen_x >= rect.left
                && screen_x < rect.right
                && screen_y >= rect.top
                && screen_y < rect.bottom
            {
                return Some(hwnd);
            }
        }
        candidate = unsafe { GetWindow(hwnd, GW_HWNDPREV) }.ok();
    }
    None
}

type OverlayGeometry = (i32, i32, i32, i32);
type CornerGeometries = Vec<(OverlayGeometry, u8, bool)>;

struct OwnedKernelHandle {
    raw: usize,
}

impl OwnedKernelHandle {
    fn new(handle: HANDLE) -> Self {
        Self {
            raw: handle.0 as usize,
        }
    }

    fn get(&self) -> HANDLE {
        HANDLE(self.raw as *mut core::ffi::c_void)
    }
}

impl Drop for OwnedKernelHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.get()) };
    }
}

fn process_name(process_id: u32) -> ComputerUseResult<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
        .map(OwnedKernelHandle::new)
        .map_err(|error| {
            ComputerUseError::new(
                ComputerUseErrorCode::PermissionDenied,
                format!("the scoped target process identity could not be verified: {error}"),
            )
        })?;
    let mut path = vec![0_u16; PROCESS_PATH_CAPACITY];
    let mut length = path.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process.get(),
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    }
    .map_err(|error| {
        ComputerUseError::new(
            ComputerUseErrorCode::PermissionDenied,
            format!("the scoped target process identity could not be verified: {error}"),
        )
    })?;
    let executable = String::from_utf16_lossy(&path[..length as usize]);
    Ok(std::path::Path::new(&executable)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(&executable)
        .to_string())
}

pub(crate) fn validate_target_policy(
    window_handle: u64,
    process_id: u32,
    window_title: &str,
) -> ComputerUseResult<()> {
    let process_name = process_name(process_id)?;
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    let mut class_name = [0_u16; 256];
    let class_length = unsafe { GetClassNameW(hwnd, &mut class_name) };
    if class_length == 0 {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::PermissionDenied,
            "the scoped target window class could not be verified",
        ));
    }
    let class_name = String::from_utf16_lossy(&class_name[..class_length as usize]);
    if let Some(reason) = denied_target_reason(&process_name, &class_name, window_title) {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::PermissionDenied,
            reason,
        ));
    }
    Ok(())
}

struct NamedMutexOwner {
    handle: OwnedKernelHandle,
}

enum NamedMutexAcquisition {
    Acquired(NamedMutexOwner),
    Abandoned(NamedMutexOwner),
    Busy,
}

impl Drop for NamedMutexOwner {
    fn drop(&mut self) {
        let _ = unsafe { ReleaseMutex(self.handle.get()) };
    }
}

struct InputOwnerLease {
    owner: Option<NamedMutexOwner>,
    stop: Arc<AtomicBool>,
}

impl InputOwnerLease {
    fn new(owner: NamedMutexOwner, stop: Arc<AtomicBool>) -> Self {
        Self {
            owner: Some(owner),
            stop,
        }
    }
}

impl Drop for InputOwnerLease {
    fn drop(&mut self) {
        // The named owner must outlive every SendInput call from this process.
        // Stop new actions, drain the only local input critical section, then
        // release the cross-process owner while queued local calls stay gated.
        // The public stop path bounds its join and leaves this thread holding
        // the owner fail-closed if a native input call never returns.
        self.stop.store(true, Ordering::Release);

        // Bounded retry with exponential backoff. If the desktop is locked
        // (e.g. Winlogon session) `flush_pending_input_releases_locked()` will
        // keep failing and we must not hold the cross-process named mutex
        // indefinitely — that would prevent any adapter process from starting a
        // new Computer Use session until this process exits.
        //
        // Hard deadline: 5 seconds total. After that we accept that pending
        // releases cannot be confirmed right now, release the owner mutex, and
        // exit. The deferred releases remain in PENDING_INPUT_RELEASES and the
        // next `acquire_input_owner()` call will attempt to flush them.
        const HARD_DEADLINE: Duration = Duration::from_secs(5);
        const MAX_SLEEP: Duration = Duration::from_millis(500);
        let deadline = Instant::now() + HARD_DEADLINE;
        let mut sleep_ms = 100u64;

        loop {
            let input_guard = INPUT_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if input::flush_pending_input_releases_locked().is_ok() {
                drop(self.owner.take());
                drop(input_guard);
                return;
            }
            drop(input_guard);

            if Instant::now() >= deadline {
                // Hard deadline exceeded — release the owner mutex unconditionally
                // so no new session is blocked. Deferred releases will be retried
                // by the next session's acquire_input_owner().
                tracing::warn!(
                    "InputOwnerLease::drop: timed out flushing pending input releases \
                     after {}s; releasing owner mutex to unblock future sessions",
                    HARD_DEADLINE.as_secs()
                );
                drop(self.owner.take());
                return;
            }

            // Exponential backoff capped at MAX_SLEEP.
            thread::sleep(Duration::from_millis(sleep_ms));
            sleep_ms = (sleep_ms * 2).min(MAX_SLEEP.as_millis() as u64);
        }
    }
}

fn create_manual_reset_event(name: &str) -> windows::core::Result<OwnedKernelHandle> {
    let name = wide(name);
    let handle = unsafe { CreateEventW(None, true, false, PCWSTR(name.as_ptr())) }?;
    Ok(OwnedKernelHandle::new(handle))
}

/// Returns `Some(true)` when the kernel object is signaled, `Some(false)` when
/// it is not, and `None` when the wait itself fails (e.g. the handle is invalid).
#[allow(dead_code)]
fn event_signaled(event: &OwnedKernelHandle) -> Option<bool> {
    match unsafe { WaitForSingleObject(event.get(), 0) } {
        WAIT_OBJECT_0 => Some(true),
        WAIT_TIMEOUT => Some(false),
        _ => None,
    }
}

/// Acquire (and if necessary, initialize) the cross-process interrupt event.
///
/// Returns the raw HANDLE value so callers do not need to hold the mutex lock
/// while performing Win32 operations. The handle is valid for the lifetime of
/// the process because `USER_INTERRUPT_EVENT` is a process-static Mutex.
///
/// Unlike a `OnceLock`-based approach, this function can re-initialize the
/// event after a transient `CreateEventW` failure: `clear_user_interrupt()`
/// resets the `Option` to `None`, allowing the next caller to retry creation
/// and recover without a process restart.
fn user_interrupt_event_raw() -> Option<HANDLE> {
    let mut guard = USER_INTERRUPT_EVENT
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if guard.is_none() {
        *guard = create_manual_reset_event(USER_INTERRUPT_EVENT_NAME).ok();
    }
    guard.as_ref().map(OwnedKernelHandle::get)
}

fn require_user_interrupt_event_raw() -> ComputerUseResult<HANDLE> {
    user_interrupt_event_raw().ok_or_else(|| {
        USER_INTERRUPT_EVENT_FAILED.store(true, Ordering::Release);
        ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            "Windows could not create the cross-process DCC UI Control stop latch; restart the adapter before enabling native input",
        )
    })
}

fn set_user_interrupt() {
    USER_INTERRUPTED.store(true, Ordering::Release);
    let signaled =
        user_interrupt_event_raw().is_some_and(|event| unsafe { SetEvent(event) }.is_ok());
    if !signaled {
        USER_INTERRUPT_EVENT_FAILED.store(true, Ordering::Release);
    }
}

fn try_acquire_named_mutex(name: &str) -> windows::core::Result<NamedMutexAcquisition> {
    let name = wide(name);
    let handle =
        OwnedKernelHandle::new(unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr()))? });
    match unsafe { WaitForSingleObject(handle.get(), 0) } {
        WAIT_OBJECT_0 => Ok(NamedMutexAcquisition::Acquired(NamedMutexOwner { handle })),
        WAIT_ABANDONED => Ok(NamedMutexAcquisition::Abandoned(NamedMutexOwner { handle })),
        WAIT_TIMEOUT => Ok(NamedMutexAcquisition::Busy),
        _ => Err(windows::core::Error::from_thread()),
    }
}

fn acquire_input_owner() -> ComputerUseResult<NamedMutexOwner> {
    acquire_input_owner_impl(false)
}

fn acquire_input_owner_after_user_approval() -> ComputerUseResult<NamedMutexOwner> {
    acquire_input_owner_impl(true)
}

fn acquire_input_owner_impl(allow_abandoned: bool) -> ComputerUseResult<NamedMutexOwner> {
    let acquisition = try_acquire_named_mutex(INPUT_OWNER_MUTEX_NAME).map_err(|error| {
        ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            format!("failed to create the Windows DCC UI Control input-owner mutex: {error}"),
        )
    })?;
    resolve_input_owner(acquisition, allow_abandoned)
}

fn resolve_input_owner(
    acquisition: NamedMutexAcquisition,
    allow_abandoned: bool,
) -> ComputerUseResult<NamedMutexOwner> {
    match acquisition {
        NamedMutexAcquisition::Acquired(owner) => Ok(owner),
        NamedMutexAcquisition::Abandoned(owner) => {
            // An abandoned mutex means the previous owner thread exited
            // without completing normal input cleanup. Its key/button state
            // is unknowable in this process, so latch every adapter in this
            // Windows logon session until a user explicitly approves reset.
            set_user_interrupt();
            if allow_abandoned {
                Ok(owner)
            } else {
                drop(owner);
                Err(ComputerUseError::new(
                    ComputerUseErrorCode::UserInterrupted,
                    "the previous DCC UI Control input owner exited unexpectedly; explicit user approval is required before native input can resume",
                ))
            }
        }
        NamedMutexAcquisition::Busy => Err(ComputerUseError::new(
            ComputerUseErrorCode::PermissionDenied,
            "another DCC UI Control process already owns system input",
        )),
    }
}

fn user_interrupted_error() -> ComputerUseError {
    ComputerUseError::new(
        ComputerUseErrorCode::UserInterrupted,
        format!(
            "the user pressed {STOP_HOTKEY_LABEL}; explicit user approval is required before DCC UI Control can resume"
        ),
    )
}

fn require_interactive_desktop(interactive: bool) -> ComputerUseResult<()> {
    if interactive {
        return Ok(());
    }
    Err(ComputerUseError::new(
        ComputerUseErrorCode::DesktopUnavailable,
        "the Windows desktop is locked, disconnected, or not interactive; no UI input was sent",
    ))
}

fn thread_desktop_receives_input() -> bool {
    let Ok(desktop) = (unsafe { GetThreadDesktop(GetCurrentThreadId()) }) else {
        return false;
    };
    let mut receives_input = 0_i32;
    let mut needed = 0_u32;
    unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_IO,
            Some((&raw mut receives_input).cast()),
            size_of::<i32>() as u32,
            Some(&raw mut needed),
        )
    }
    .is_ok()
        && receives_input != 0
}

fn current_session_is_active() -> bool {
    let mut buffer = PWSTR::null();
    let mut bytes = 0_u32;
    if unsafe {
        WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSConnectState,
            &raw mut buffer,
            &raw mut bytes,
        )
    }
    .is_err()
        || buffer.0.is_null()
    {
        return false;
    }
    let active = bytes >= size_of::<WTS_CONNECTSTATE_CLASS>() as u32
        && unsafe { *buffer.0.cast::<WTS_CONNECTSTATE_CLASS>() } == WTSActive;
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    active
}

pub(crate) fn desktop_interactive() -> bool {
    current_session_is_active() && thread_desktop_receives_input()
}

pub(crate) fn ensure_interactive_desktop() -> ComputerUseResult<()> {
    require_interactive_desktop(desktop_interactive())
}

pub(crate) fn synchronize_desktop_events(
    barrier: &DesktopEventBarrier,
    stop_requested: &Arc<AtomicBool>,
) -> ComputerUseResult<()> {
    crate::check_action_cancellation(stop_requested)?;
    ensure_interactive_desktop()?;
    let window_handle = barrier.window_handle();
    if window_handle == 0 {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            "the DCC UI Control thread is not ready to synchronize desktop events",
        ));
    }
    let sequence = barrier.request_sequence();
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    unsafe {
        PostMessageW(
            Some(hwnd),
            DESKTOP_BARRIER_MESSAGE,
            WPARAM(sequence as usize),
            LPARAM(0),
        )
    }
    .map_err(|error| {
        ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            format!("failed to synchronize the DCC UI Control desktop message queue: {error}"),
        )
    })?;

    let deadline = Instant::now() + DESKTOP_BARRIER_TIMEOUT;
    while !barrier.is_acknowledged(sequence) {
        crate::check_action_cancellation(stop_requested)?;
        ensure_interactive_desktop()?;
        if barrier.window_handle() != window_handle {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                "the DCC UI Control thread exited while synchronizing desktop events",
            ));
        }
        if Instant::now() >= deadline {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                "timed out synchronizing pending Windows desktop events; no UI input was sent",
            ));
        }
        thread::sleep(Duration::from_millis(2));
    }
    ensure_interactive_desktop()
}

pub(crate) struct ThreadDpiAwareness {
    previous: DPI_AWARENESS_CONTEXT,
}

impl ThreadDpiAwareness {
    pub(crate) fn enter() -> ComputerUseResult<Self> {
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.0.is_null() {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                "Windows refused per-monitor-v2 DPI awareness for the DCC UI Control thread",
            ));
        }
        Ok(Self { previous })
    }
}

impl Drop for ThreadDpiAwareness {
    fn drop(&mut self) {
        let _ = unsafe { SetThreadDpiAwarenessContext(self.previous) };
    }
}

pub(crate) fn prepare_target_window(window_handle: u64) -> ComputerUseResult<()> {
    ensure_interactive_desktop()?;
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(target_unavailable());
    }
    let _ = available_target_rect(hwnd)?;
    Ok(())
}

pub(crate) fn scoped_window_state(
    window_handle: u64,
    expected_process_id: u32,
) -> ComputerUseResult<ScopedWindowState> {
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Ok(ScopedWindowState {
            process_id: expected_process_id,
            window_handle,
            exists: false,
            visible: false,
            minimized: false,
            foreground: false,
        });
    }
    geometry::validate_target_identity(hwnd, expected_process_id)?;
    let title_length = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
    let mut title = vec![0_u16; title_length.saturating_add(1)];
    let copied = unsafe { GetWindowTextW(hwnd, &mut title) }.max(0) as usize;
    title.truncate(copied);
    validate_target_policy(
        window_handle,
        expected_process_id,
        &String::from_utf16_lossy(&title),
    )?;
    Ok(ScopedWindowState {
        process_id: expected_process_id,
        window_handle,
        exists: true,
        visible: unsafe { IsWindowVisible(hwnd) }.as_bool(),
        minimized: unsafe { IsIconic(hwnd) }.as_bool(),
        foreground: unsafe { GetForegroundWindow() } == hwnd,
    })
}

pub(crate) fn transition_scoped_window(
    window_handle: u64,
    expected_process_id: u32,
    operation: ScopedWindowOperation,
) -> ComputerUseResult<ScopedWindowState> {
    let before = scoped_window_state(window_handle, expected_process_id)?;
    if !before.exists {
        return Err(target_unavailable());
    }
    ensure_interactive_desktop()?;
    if user_interrupted() {
        return Err(user_interrupted_error());
    }
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    match operation {
        ScopedWindowOperation::Restore => {
            if before.minimized {
                let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
            }
        }
        ScopedWindowOperation::Show => {
            let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        }
        ScopedWindowOperation::Activate => {
            if !before.visible || before.minimized {
                return Err(ComputerUseError::new(
                    ComputerUseErrorCode::InvalidAction,
                    "show and restore the exact scoped DCC window before activating it",
                ));
            }
            input::set_target_foreground(hwnd);
        }
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        ensure_interactive_desktop()?;
        let state = scoped_window_state(window_handle, expected_process_id)?;
        let complete = match operation {
            ScopedWindowOperation::Restore => !state.minimized,
            ScopedWindowOperation::Show => state.visible,
            ScopedWindowOperation::Activate => state.foreground,
        };
        if complete {
            return Ok(state);
        }
        if Instant::now() >= deadline {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::FocusLost,
                "Windows did not apply the requested exact-window state transition",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
pub(crate) struct TestIsolationGuard {
    _owner: NamedMutexOwner,
}

#[cfg(test)]
pub(crate) fn acquire_test_isolation_guard() -> ComputerUseResult<TestIsolationGuard> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let acquisition = try_acquire_named_mutex(TEST_ISOLATION_MUTEX_NAME).map_err(|error| {
            ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                format!("failed to create the DCC UI Control test-isolation mutex: {error}"),
            )
        })?;
        match acquisition {
            NamedMutexAcquisition::Acquired(owner) | NamedMutexAcquisition::Abandoned(owner) => {
                return Ok(TestIsolationGuard { _owner: owner });
            }
            NamedMutexAcquisition::Busy if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            NamedMutexAcquisition::Busy => {
                return Err(ComputerUseError::new(
                    ComputerUseErrorCode::BackendUnavailable,
                    "timed out waiting for cross-process DCC UI Control test isolation",
                ));
            }
        }
    }
}

pub(crate) fn prepare_target_for_input(
    window_handle: u64,
    expected_process_id: u32,
) -> ComputerUseResult<()> {
    ensure_interactive_desktop()?;
    let hwnd = HWND(window_handle as *mut core::ffi::c_void);
    restore_target_for_input(hwnd, expected_process_id)?;
    let _ = available_target_rect_for_process(hwnd, expected_process_id)?;
    Ok(())
}

pub(crate) fn user_interrupted() -> bool {
    if USER_INTERRUPTED.load(Ordering::Acquire)
        || USER_INTERRUPT_EVENT_FAILED.load(Ordering::Acquire)
    {
        return true;
    }
    let Ok(event) = require_user_interrupt_event_raw() else {
        // Cross-process interruption can no longer be proven. Fail closed so
        // no process silently resumes native input with a broken latch.
        return true;
    };
    match unsafe { WaitForSingleObject(event, 0) } {
        WAIT_OBJECT_0 => true,
        WAIT_TIMEOUT => false,
        _ => {
            USER_INTERRUPT_EVENT_FAILED.store(true, Ordering::Release);
            true
        }
    }
}

pub(crate) fn clear_user_interrupt() -> ComputerUseResult<()> {
    // Holding the owner mutex during reset proves input is idle and prevents
    // clearing the stop latch for a new owner that starts concurrently.
    // Explicit approval is the only path allowed to accept an abandoned
    // previous owner; ordinary session start always fails closed instead.
    let _input_owner = acquire_input_owner_after_user_approval()?;
    // Drop the event handle and re-initialize: if a previous CreateEventW
    // call failed (storing None in the Mutex), this clears the stale None so
    // user_interrupt_event_raw() retries creation on the next call, allowing
    // recovery from transient kernel-object exhaustion without a process restart.
    {
        let mut guard = USER_INTERRUPT_EVENT
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = create_manual_reset_event(USER_INTERRUPT_EVENT_NAME).ok();
        }
    }
    let event = require_user_interrupt_event_raw()?;
    unsafe { ResetEvent(event) }.map_err(|error| {
        USER_INTERRUPT_EVENT_FAILED.store(true, Ordering::Release);
        ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            format!("failed to reset the cross-process DCC UI Control stop latch: {error}"),
        )
    })?;
    USER_INTERRUPTED.store(false, Ordering::Release);
    USER_INTERRUPT_EVENT_FAILED.store(false, Ordering::Release);
    Ok(())
}

mod banner;
mod geometry;
mod input;
mod overlay;

#[allow(unused_imports)]
use banner::session_event_blocked;
pub(crate) use banner::start_control_banner;
use geometry::*;
pub(crate) use input::{flush_pending_input_releases, perform_action, window_dpi};
use overlay::*;
