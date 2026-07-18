use std::cell::Cell;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    COLORREF, CloseHandle, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_ABANDONED,
    WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateRoundRectRgn,
    CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE,
    DT_VCENTER, DeleteObject, DrawTextW, EndPaint, EnumDisplayMonitors, FW_SEMIBOLD,
    GetMonitorInfoW, HDC, HGDIOBJ, HMONITOR, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint,
    MonitorFromRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor,
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
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
    MOUSEINPUT, RegisterHotKey, SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GA_ROOT,
    GW_HWNDPREV, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetClientRect, GetCursorPos,
    GetForegroundWindow, GetSystemMetrics, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST,
    IsIconic, IsWindow, IsWindowVisible, LWA_ALPHA, MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW,
    PostMessageW, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_HIDE, SW_RESTORE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetLayeredWindowAttributes,
    SetWindowDisplayAffinity, SetWindowPos, ShowWindow, TranslateMessage, WDA_EXCLUDEFROMCAPTURE,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_PAINT,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT,
    WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
    WindowFromPoint,
};
use windows::core::{BOOL, PCWSTR, PWSTR, w};

use crate::{
    ComputerUseAction, ComputerUseError, ComputerUseErrorCode, ComputerUseObservation,
    ComputerUsePoint, ComputerUseResult, denied_target_reason, desktop_state_snapshot,
    record_desktop_environment_change, record_desktop_transition,
};

use super::{
    ControlBannerSignals, ControlBannerStartError, ControlBannerStartResult, DesktopEventBarrier,
};

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
const STOP_HOTKEY_LABEL: &str = "Ctrl+Alt+Esc";
const STOP_HOTKEY_MODIFIERS: HOT_KEY_MODIFIERS =
    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0);
const BORDER_THICKNESS: i32 = 48;
const POINTER_EFFECT_SIZE: i32 = 120;
const CONTROL_OVERLAY_ALPHA: u8 = 185;
const CONTROL_BORDER_ALPHA: u8 = 112;
const CONTROL_BANNER_ALPHA: u8 = 238;
const CONTROL_CURSOR_ALPHA: u8 = 140;
const CONTROL_BANNER_FONT_SIZE: i32 = 22;
const CONTROL_PULSE_PERIOD_MS: u64 = 2_400;
const CONTROL_BORDER_PULSE_FLOOR_PERCENT: u8 = 55;
const CONTROL_BANNER_PULSE_FLOOR_PERCENT: u8 = 84;
const CONTROL_CURSOR_PULSE_FLOOR_PERCENT: u8 = 72;
const CONTROL_ACCENT_COLOR: COLORREF = COLORREF(0x00EB_6325);
const CONTROL_FOCUS_COLOR: COLORREF = COLORREF(0x0027_1811);
const CONTROL_OVERLAY_CLASS: PCWSTR = w!("DccMcpComputerUseOverlay");
const CONTROL_FOCUS_CLASS: PCWSTR = w!("DccMcpComputerUseFocusOverlay");
const DEFAULT_POINTER_EFFECT_DWELL_MS: u64 = 350;
const DRAG_UPDATE_INTERVAL_MS: u64 = 16;
const TARGET_RESTORE_TIMEOUT: Duration = Duration::from_millis(500);
const DESKTOP_BARRIER_MESSAGE: u32 = WM_APP + 0x443;
const DESKTOP_BARRIER_TIMEOUT: Duration = Duration::from_millis(500);
const PROCESS_PATH_CAPACITY: usize = 32_768;

fn is_control_overlay_window(hwnd: HWND) -> bool {
    let mut class_name = [0_u16; 64];
    let length = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
    matches!(
        String::from_utf16_lossy(&class_name[..length]).as_str(),
        "DccMcpComputerUseOverlay" | "DccMcpComputerUseFocusOverlay"
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
type BorderGeometries = Vec<(OverlayGeometry, u8, bool)>;

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
            "Windows could not create the cross-process Computer Use stop latch; restart the adapter before enabling native input",
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
            format!("failed to create the Windows Computer Use input-owner mutex: {error}"),
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
                    "the previous Computer Use input owner exited unexpectedly; explicit user approval is required before native input can resume",
                ))
            }
        }
        NamedMutexAcquisition::Busy => Err(ComputerUseError::new(
            ComputerUseErrorCode::PermissionDenied,
            "another DCC MCP Computer Use process already owns system input",
        )),
    }
}

fn user_interrupted_error() -> ComputerUseError {
    ComputerUseError::new(
        ComputerUseErrorCode::UserInterrupted,
        format!(
            "the user pressed {STOP_HOTKEY_LABEL}; explicit user approval is required before Computer Use can resume"
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
            "the Computer Use control thread is not ready to synchronize desktop events",
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
            format!("failed to synchronize the Computer Use desktop message queue: {error}"),
        )
    })?;

    let deadline = Instant::now() + DESKTOP_BARRIER_TIMEOUT;
    while !barrier.is_acknowledged(sequence) {
        crate::check_action_cancellation(stop_requested)?;
        ensure_interactive_desktop()?;
        if barrier.window_handle() != window_handle {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                "the Computer Use control thread exited while synchronizing desktop events",
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
                "Windows refused per-monitor-v2 DPI awareness for the Computer Use thread",
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
                format!("failed to create the Computer Use test-isolation mutex: {error}"),
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
                    "timed out waiting for cross-process Computer Use test isolation",
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
            "another DCC MCP Computer Use session already owns system input",
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
                run_banner(window_handle, process_id, &app_name, &runtime, &ready_tx)
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
                format!("failed to start the Computer Use control thread: {error}"),
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
                    "timed out while starting the Computer Use control banner",
                ),
                thread: crate::join_control_thread(thread),
            })
        }
    }
}

struct OverlayLayer {
    hwnd: HWND,
    base_alpha: u8,
    applied_alpha: Cell<u8>,
}

impl OverlayLayer {
    fn new(hwnd: HWND, base_alpha: u8, applied_alpha: u8) -> Self {
        Self {
            hwnd,
            base_alpha,
            applied_alpha: Cell::new(applied_alpha),
        }
    }

    fn apply_pulse(&self, floor_percent: u8, elapsed_ms: u64) -> ComputerUseResult<()> {
        let alpha = breathing_alpha(self.base_alpha, floor_percent, elapsed_ms);
        if alpha != self.applied_alpha.get() {
            set_overlay_alpha(self.hwnd, alpha)?;
            self.applied_alpha.set(alpha);
        }
        Ok(())
    }
}

struct ControlOverlay {
    banner: OverlayLayer,
    borders: Vec<OverlayLayer>,
    cursor_mask: OverlayLayer,
    cursor_visible: Cell<bool>,
    pulse_started: Instant,
}

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

impl ControlOverlay {
    fn new(
        target: HWND,
        target_rect: &windows::Win32::Foundation::RECT,
        caption: &str,
    ) -> ComputerUseResult<Self> {
        let (banner_geometry, border_geometries) = overlay_geometries(target, target_rect)?;
        let banner_alpha =
            breathing_alpha(CONTROL_BANNER_ALPHA, CONTROL_BANNER_PULSE_FLOOR_PERCENT, 0);
        let banner = OverlayLayer::new(
            create_color_overlay(caption, banner_geometry, banner_alpha, false, true)?,
            CONTROL_BANNER_ALPHA,
            banner_alpha,
        );
        let mut borders = Vec::with_capacity(border_geometries.len());
        for (geometry, alpha, focus) in border_geometries {
            let initial_alpha = breathing_alpha(alpha, CONTROL_BORDER_PULSE_FLOOR_PERCENT, 0);
            match create_color_overlay("", geometry, initial_alpha, false, focus) {
                Ok(hwnd) => borders.push(OverlayLayer::new(hwnd, alpha, initial_alpha)),
                Err(error) => {
                    for layer in borders {
                        let _ = unsafe { DestroyWindow(layer.hwnd) };
                    }
                    let _ = unsafe { DestroyWindow(banner.hwnd) };
                    return Err(error);
                }
            }
        }
        let mut cursor = POINT::default();
        if let Err(error) = unsafe { GetCursorPos(&mut cursor) } {
            for layer in borders {
                let _ = unsafe { DestroyWindow(layer.hwnd) };
            }
            let _ = unsafe { DestroyWindow(banner.hwnd) };
            return Err(overlay_backend_error(
                "locate the pointer for",
                error.to_string(),
            ));
        }
        let cursor_alpha =
            breathing_alpha(CONTROL_CURSOR_ALPHA, CONTROL_CURSOR_PULSE_FLOOR_PERCENT, 0);
        let cursor_mask = match create_color_overlay(
            "",
            pointer_mask_geometry(cursor.x, cursor.y),
            cursor_alpha,
            false,
            false,
        ) {
            Ok(hwnd) => OverlayLayer::new(hwnd, CONTROL_CURSOR_ALPHA, cursor_alpha),
            Err(error) => {
                for layer in borders {
                    let _ = unsafe { DestroyWindow(layer.hwnd) };
                }
                let _ = unsafe { DestroyWindow(banner.hwnd) };
                return Err(error);
            }
        };
        let cursor_visible = point_in_rect(cursor, target_rect);
        let overlay = Self {
            banner,
            borders,
            cursor_mask,
            cursor_visible: Cell::new(cursor_visible),
            pulse_started: Instant::now(),
        };
        overlay.set_visible(true)?;
        Ok(overlay)
    }

    fn reposition(
        &self,
        target: HWND,
        target_rect: &windows::Win32::Foundation::RECT,
    ) -> ComputerUseResult<()> {
        let (banner_geometry, border_geometries) = overlay_geometries(target, target_rect)?;
        position_overlay(self.banner.hwnd, banner_geometry, false)?;
        for (layer, (geometry, _alpha, _focus)) in self.borders.iter().zip(border_geometries) {
            position_overlay(layer.hwnd, geometry, false)?;
        }
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor) }
            .map_err(|error| overlay_backend_error("locate the pointer for", error.to_string()))?;
        let cursor_visible = point_in_rect(cursor, target_rect);
        if cursor_visible {
            position_overlay(
                self.cursor_mask.hwnd,
                pointer_mask_geometry(cursor.x, cursor.y),
                false,
            )?;
        }
        if cursor_visible != self.cursor_visible.get() {
            set_overlay_visible(self.cursor_mask.hwnd, cursor_visible)?;
            self.cursor_visible.set(cursor_visible);
        }
        let elapsed_ms = self.pulse_started.elapsed().as_millis() as u64;
        self.banner
            .apply_pulse(CONTROL_BANNER_PULSE_FLOOR_PERCENT, elapsed_ms)?;
        for layer in &self.borders {
            layer.apply_pulse(CONTROL_BORDER_PULSE_FLOOR_PERCENT, elapsed_ms)?;
        }
        self.cursor_mask
            .apply_pulse(CONTROL_CURSOR_PULSE_FLOOR_PERCENT, elapsed_ms)?;
        Ok(())
    }

    fn set_visible(&self, visible: bool) -> ComputerUseResult<()> {
        set_overlay_visible(self.banner.hwnd, visible)?;
        for layer in &self.borders {
            set_overlay_visible(layer.hwnd, visible)?;
        }
        if visible {
            if self.cursor_visible.get() {
                set_overlay_visible(self.cursor_mask.hwnd, true)?;
            }
        } else if self.cursor_visible.replace(false) {
            set_overlay_visible(self.cursor_mask.hwnd, false)?;
        }
        Ok(())
    }
}

impl Drop for ControlOverlay {
    fn drop(&mut self) {
        for layer in self.borders.drain(..) {
            let _ = unsafe { DestroyWindow(layer.hwnd) };
        }
        let _ = unsafe { DestroyWindow(self.cursor_mask.hwnd) };
        let _ = unsafe { DestroyWindow(self.banner.hwnd) };
    }
}

fn register_color_overlay_classes() -> ComputerUseResult<()> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let instance = unsafe { GetModuleHandleW(None) }
                .map_err(|error| format!("resolve module handle: {error}"))?;
            for (class_name, color) in [
                (CONTROL_OVERLAY_CLASS, CONTROL_ACCENT_COLOR),
                (CONTROL_FOCUS_CLASS, CONTROL_FOCUS_COLOR),
            ] {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(overlay_window_proc),
                    hInstance: instance.into(),
                    hbrBackground: unsafe { CreateSolidBrush(color) },
                    lpszClassName: class_name,
                    ..Default::default()
                };
                let atom = unsafe { RegisterClassW(&class) };
                if atom == 0 {
                    return Err(format!(
                        "register overlay window class: {}",
                        windows::core::Error::from_thread()
                    ));
                }
            }
            Ok(())
        })
        .clone()
        .map_err(|detail| overlay_backend_error("register", detail))
}

unsafe extern "system" fn overlay_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_PAINT {
        let mut paint = PAINTSTRUCT::default();
        let device = unsafe { BeginPaint(hwnd, &raw mut paint) };
        if !device.0.is_null() {
            let mut bounds = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &raw mut bounds) };
            let mut class_name = [0_u16; 64];
            let class_length = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
            let color = if String::from_utf16_lossy(&class_name[..class_length])
                == "DccMcpComputerUseFocusOverlay"
            {
                CONTROL_FOCUS_COLOR
            } else {
                CONTROL_ACCENT_COLOR
            };
            let brush = unsafe { CreateSolidBrush(color) };
            let _ = unsafe { windows::Win32::Graphics::Gdi::FillRect(device, &bounds, brush) };
            let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };

            let text_length = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
            if text_length > 0 {
                let mut text = vec![0_u16; text_length + 1];
                let copied = unsafe { GetWindowTextW(hwnd, &mut text) }.max(0) as usize;
                text.truncate(copied);
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                let font = unsafe {
                    CreateFontW(
                        -scaled_pixels(CONTROL_BANNER_FONT_SIZE, dpi),
                        0,
                        0,
                        0,
                        FW_SEMIBOLD.0 as i32,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET,
                        OUT_DEFAULT_PRECIS,
                        CLIP_DEFAULT_PRECIS,
                        CLEARTYPE_QUALITY,
                        u32::from(DEFAULT_PITCH.0),
                        w!("Segoe UI Semibold"),
                    )
                };
                if !font.0.is_null() {
                    let old_font = unsafe { SelectObject(device, HGDIOBJ(font.0)) };
                    let _ = unsafe { SetBkMode(device, TRANSPARENT) };
                    let _ = unsafe { SetTextColor(device, COLORREF(0x00FF_FFFF)) };
                    let format = windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(
                        DT_CENTER.0 | DT_VCENTER.0 | DT_SINGLELINE.0 | DT_END_ELLIPSIS.0,
                    );
                    let _ = unsafe { DrawTextW(device, &mut text, &raw mut bounds, format) };
                    let _ = unsafe { SelectObject(device, old_font) };
                    let _ = unsafe { DeleteObject(HGDIOBJ(font.0)) };
                }
            }
        }
        let _ = unsafe { EndPaint(hwnd, &paint) };
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn create_color_overlay(
    caption: &str,
    (x, y, width, height): OverlayGeometry,
    alpha: u8,
    show: bool,
    focus: bool,
) -> ComputerUseResult<HWND> {
    register_color_overlay_classes()?;
    let caption = wide(caption);
    let style = WINDOW_STYLE(WS_POPUP.0);
    let ex_style = WINDOW_EX_STYLE(
        WS_EX_TOPMOST.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TRANSPARENT.0
            | WS_EX_LAYERED.0,
    );
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            if focus {
                CONTROL_FOCUS_CLASS
            } else {
                CONTROL_OVERLAY_CLASS
            },
            PCWSTR(caption.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(|error| overlay_backend_error("create", error.to_string()))?;
    if let Err(error) = exclude_overlay_from_capture(hwnd) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    if let Err(error) = set_overlay_alpha(hwnd, alpha) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    let radius = width.min(height).max(1);
    let region = unsafe { CreateRoundRectRgn(0, 0, width, height, radius, radius) };
    if region.0.is_null() || unsafe { SetWindowRgn(hwnd, Some(region), true) } == 0 {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(overlay_backend_error(
            "round",
            "Windows did not accept the overlay region",
        ));
    }
    if let Err(error) = position_overlay(hwnd, (x, y, width, height), show) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    pump_overlay_messages(hwnd);
    Ok(hwnd)
}

fn set_overlay_alpha(hwnd: HWND, alpha: u8) -> ComputerUseResult<()> {
    unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) }
        .map_err(|error| overlay_backend_error("configure transparency for", error.to_string()))
}

fn exclude_overlay_from_capture(hwnd: HWND) -> ComputerUseResult<()> {
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|error| overlay_backend_error("exclude from capture", error.to_string()))
}

fn position_overlay(
    hwnd: HWND,
    (x, y, width, height): OverlayGeometry,
    show: bool,
) -> ComputerUseResult<()> {
    let flags = if show {
        SWP_NOACTIVATE | SWP_SHOWWINDOW
    } else {
        SWP_NOACTIVATE
    };
    unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, flags) }
        .map_err(|error| overlay_backend_error("position", error.to_string()))?;
    let mut actual = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut actual) }
        .map_err(|error| overlay_backend_error("verify the position of", error.to_string()))?;
    if [
        actual.left,
        actual.top,
        actual.right - actual.left,
        actual.bottom - actual.top,
    ] != [x, y, width, height]
    {
        return Err(overlay_backend_error(
            "verify the position of",
            "Windows reported unexpected overlay bounds",
        ));
    }
    if show && !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return Err(overlay_backend_error(
            "show",
            "Windows did not make the overlay visible",
        ));
    }
    Ok(())
}

fn set_overlay_visible(hwnd: HWND, visible: bool) -> ComputerUseResult<()> {
    let command = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
    let _ = unsafe { ShowWindow(hwnd, command) };
    pump_overlay_messages(hwnd);
    if unsafe { IsWindowVisible(hwnd) }.as_bool() != visible {
        return Err(overlay_backend_error(
            if visible { "show" } else { "hide" },
            "Windows did not apply the requested visibility",
        ));
    }
    Ok(())
}

fn overlay_backend_error(operation: &str, detail: impl std::fmt::Display) -> ComputerUseError {
    ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        format!("failed to {operation} the Computer Use visual overlay: {detail}"),
    )
}

fn pump_overlay_messages(hwnd: HWND) {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn session_event_blocked(event: u32) -> Option<bool> {
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

fn run_banner(
    window_handle: u64,
    process_id: u32,
    app_name: &str,
    signals: &BannerRuntimeSignals,
    ready: &std::sync::mpsc::SyncSender<ComputerUseResult<()>>,
) -> ComputerUseResult<()> {
    ensure_interactive_desktop()?;
    let target = HWND(window_handle as *mut core::ffi::c_void);
    let caption = format!("●  DCC MCP controls {app_name}   ·   {STOP_HOTKEY_LABEL} to stop");
    let mut rect = available_target_rect_for_process(target, process_id)?;
    let overlay = ControlOverlay::new(target, &rect, &caption)?;

    let hotkey_result = unsafe {
        RegisterHotKey(
            Some(overlay.banner.hwnd),
            HOTKEY_ID,
            STOP_HOTKEY_MODIFIERS,
            VK_ESCAPE.0 as u32,
        )
    };
    if let Err(error) = hotkey_result {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            format!("failed to reserve {STOP_HOTKEY_LABEL} for Computer Use: {error}"),
        ));
    }
    let _hotkey = RegisteredHotKey {
        hwnd: overlay.banner.hwnd,
    };
    let _session_notifications = RegisteredSessionNotifications::new(overlay.banner.hwnd)?;
    let _desktop_barrier =
        RegisteredDesktopBarrier::new(Arc::clone(&signals.desktop_barrier), overlay.banner.hwnd);
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
            format!("failed to reset the cross-process Computer Use stop latch: {error}"),
        )
    })?;
    USER_INTERRUPTED.store(false, Ordering::Release);
    USER_INTERRUPT_EVENT_FAILED.store(false, Ordering::Release);
    Ok(())
}

mod geometry;
mod input;

use geometry::*;
pub(crate) use input::{flush_pending_input_releases, perform_action, window_dpi};
