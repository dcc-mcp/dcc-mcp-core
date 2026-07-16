use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    COLORREF, CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT, WAIT_ABANDONED, WAIT_OBJECT_0,
    WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONULL, MONITORINFO,
    MonitorFromPoint, MonitorFromRect,
};
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
    BringWindowToTop, CreateWindowExW, DestroyWindow, DispatchMessageW, GA_ROOT, GetAncestor,
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect,
    GetWindowThreadProcessId, HWND_TOPMOST, IsIconic, IsWindow, IsWindowVisible, LWA_ALPHA, MSG,
    PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostMessageW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SW_RESTORE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_HOTKEY, WM_WTSSESSION_CHANGE, WS_BORDER, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE, WTS_CONSOLE_CONNECT,
    WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT, WTS_SESSION_LOCK,
    WTS_SESSION_UNLOCK, WindowFromPoint,
};
use windows::core::{BOOL, PCWSTR, PWSTR};

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
static USER_INTERRUPT_EVENT: OnceLock<Option<OwnedKernelHandle>> = OnceLock::new();
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
const STATIC_CENTER: u32 = 0x0000_0001;
const STATIC_CENTER_IMAGE: u32 = 0x0000_0200;
const STATIC_WHITE_RECT: u32 = 0x0000_0006;
const BORDER_THICKNESS: i32 = 5;
const POINTER_EFFECT_SIZE: i32 = 34;
const DEFAULT_POINTER_EFFECT_DWELL_MS: u64 = 350;
const DRAG_UPDATE_INTERVAL_MS: u64 = 16;
const TARGET_RESTORE_TIMEOUT: Duration = Duration::from_millis(500);
const DESKTOP_BARRIER_MESSAGE: u32 = WM_APP + 0x443;
const DESKTOP_BARRIER_TIMEOUT: Duration = Duration::from_millis(500);
const PROCESS_PATH_CAPACITY: usize = 32_768;

type OverlayGeometry = (i32, i32, i32, i32);
type BorderGeometries = [OverlayGeometry; 4];

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
        loop {
            let input_guard = INPUT_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if flush_pending_input_releases_locked().is_ok() {
                drop(self.owner.take());
                drop(input_guard);
                return;
            }
            drop(input_guard);
            // A named Windows mutex is thread-affine. Keep this banner thread
            // alive, and therefore keep every adapter process fenced out,
            // until reconnect or policy changes let us confirm all releases.
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn create_manual_reset_event(name: &str) -> windows::core::Result<OwnedKernelHandle> {
    let name = wide(name);
    let handle = unsafe { CreateEventW(None, true, false, PCWSTR(name.as_ptr())) }?;
    Ok(OwnedKernelHandle::new(handle))
}

fn event_signaled(event: &OwnedKernelHandle) -> Option<bool> {
    match unsafe { WaitForSingleObject(event.get(), 0) } {
        WAIT_OBJECT_0 => Some(true),
        WAIT_TIMEOUT => Some(false),
        _ => None,
    }
}

fn user_interrupt_event() -> Option<&'static OwnedKernelHandle> {
    USER_INTERRUPT_EVENT
        .get_or_init(|| create_manual_reset_event(USER_INTERRUPT_EVENT_NAME).ok())
        .as_ref()
}

fn require_user_interrupt_event() -> ComputerUseResult<&'static OwnedKernelHandle> {
    user_interrupt_event().ok_or_else(|| {
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
        user_interrupt_event().is_some_and(|event| unsafe { SetEvent(event.get()) }.is_ok());
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
    let _ = require_user_interrupt_event().inspect_err(|_| {
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

struct ControlOverlay {
    banner: HWND,
    borders: Vec<HWND>,
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
        let (x, y, width, height) = banner_geometry;
        let banner = create_static_overlay(
            caption,
            WS_BORDER.0 | STATIC_CENTER | STATIC_CENTER_IMAGE,
            (x, y, width, height),
            245,
        )?;
        let mut borders = Vec::with_capacity(4);
        for geometry in border_geometries {
            match create_static_overlay("", STATIC_WHITE_RECT, geometry, 225) {
                Ok(hwnd) => borders.push(hwnd),
                Err(error) => {
                    for hwnd in borders {
                        let _ = unsafe { DestroyWindow(hwnd) };
                    }
                    let _ = unsafe { DestroyWindow(banner) };
                    return Err(error);
                }
            }
        }
        Ok(Self { banner, borders })
    }

    fn reposition(
        &self,
        target: HWND,
        target_rect: &windows::Win32::Foundation::RECT,
    ) -> ComputerUseResult<()> {
        let (banner_geometry, border_geometries) = overlay_geometries(target, target_rect)?;
        position_overlay(self.banner, banner_geometry, false)?;
        for (hwnd, geometry) in self.borders.iter().zip(border_geometries) {
            position_overlay(*hwnd, geometry, false)?;
        }
        Ok(())
    }

    fn set_visible(&self, visible: bool) -> ComputerUseResult<()> {
        set_overlay_visible(self.banner, visible)?;
        for hwnd in &self.borders {
            set_overlay_visible(*hwnd, visible)?;
        }
        Ok(())
    }
}

impl Drop for ControlOverlay {
    fn drop(&mut self) {
        for hwnd in self.borders.drain(..) {
            let _ = unsafe { DestroyWindow(hwnd) };
        }
        let _ = unsafe { DestroyWindow(self.banner) };
    }
}

fn create_static_overlay(
    caption: &str,
    static_style: u32,
    (x, y, width, height): OverlayGeometry,
    alpha: u8,
) -> ComputerUseResult<HWND> {
    let class = wide("STATIC");
    let caption = wide(caption);
    let style = WINDOW_STYLE(WS_POPUP.0 | WS_VISIBLE.0 | static_style);
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
            PCWSTR(class.as_ptr()),
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
    .map_err(|error| {
        ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            format!("failed to create Computer Use visual overlay: {error}"),
        )
    })?;
    if let Err(error) = unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) } {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(overlay_backend_error(
            "configure transparency for",
            error.to_string(),
        ));
    }
    if let Err(error) = position_overlay(hwnd, (x, y, width, height), true) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    pump_overlay_messages(hwnd);
    Ok(hwnd)
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
    let caption = format!(
        "DCC MCP Computer Use is controlling {app_name} - press {STOP_HOTKEY_LABEL} to stop"
    );
    let mut rect = available_target_rect_for_process(target, process_id)?;
    let overlay = ControlOverlay::new(target, &rect, &caption)?;

    let hotkey_result = unsafe {
        RegisterHotKey(
            Some(overlay.banner),
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
        hwnd: overlay.banner,
    };
    let _session_notifications = RegisteredSessionNotifications::new(overlay.banner)?;
    let _desktop_barrier =
        RegisteredDesktopBarrier::new(Arc::clone(&signals.desktop_barrier), overlay.banner);
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
                    overlay.set_visible(false)?;
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
                overlay.set_visible(false)?;
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
                        overlay.set_visible(false)?;
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
                overlay.set_visible(false)?;
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
    let Ok(event) = require_user_interrupt_event() else {
        // Cross-process interruption can no longer be proven. Fail closed so
        // no process silently resumes native input with a broken latch.
        return true;
    };
    match event_signaled(event) {
        Some(signaled) => signaled,
        None => {
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
    let event = require_user_interrupt_event()?;
    unsafe { ResetEvent(event.get()) }.map_err(|error| {
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

fn available_target_rect_for_process(
    target: HWND,
    expected_process_id: u32,
) -> ComputerUseResult<windows::Win32::Foundation::RECT> {
    validate_target_identity(target, expected_process_id)?;
    available_target_rect(target)
}

fn restore_target_for_input(target: HWND, expected_process_id: u32) -> ComputerUseResult<()> {
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

fn validate_target_identity(target: HWND, expected_process_id: u32) -> ComputerUseResult<()> {
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

fn available_target_rect(target: HWND) -> ComputerUseResult<windows::Win32::Foundation::RECT> {
    if !unsafe { IsWindow(Some(target)) }.as_bool()
        || !unsafe { IsWindowVisible(target) }.as_bool()
        || unsafe { IsIconic(target) }.as_bool()
    {
        return Err(target_unavailable());
    }
    let mut rect = windows::Win32::Foundation::RECT::default();
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

fn rect_has_positive_area(rect: &windows::Win32::Foundation::RECT) -> bool {
    rect.right > rect.left && rect.bottom > rect.top
}

fn monitor_for_rect(rect: &windows::Win32::Foundation::RECT) -> Option<HMONITOR> {
    let monitor = unsafe { MonitorFromRect(rect, MONITOR_DEFAULTTONULL) };
    (!monitor.is_invalid()).then_some(monitor)
}

fn rect_intersects_monitor(rect: &windows::Win32::Foundation::RECT) -> bool {
    monitor_for_rect(rect).is_some()
}

fn monitor_work_area(
    rect: &windows::Win32::Foundation::RECT,
) -> Option<(HMONITOR, windows::Win32::Foundation::RECT)> {
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

fn monitor_dpi(monitor: HMONITOR, target: Option<HWND>) -> u32 {
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
struct MonitorStamp {
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

fn display_environment_stamp() -> ComputerUseResult<Vec<MonitorStamp>> {
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

fn scaled_pixels(pixels: i32, dpi: u32) -> i32 {
    let scaled = (i64::from(pixels) * i64::from(dpi.max(96)) + 48) / 96;
    scaled.clamp(1, i64::from(i32::MAX)) as i32
}

fn target_unavailable() -> ComputerUseError {
    ComputerUseError::new(
        ComputerUseErrorCode::MissingWindow,
        "the scoped DCC window is minimized, closed, or unavailable",
    )
}

fn overlay_geometries(
    target: HWND,
    target_rect: &windows::Win32::Foundation::RECT,
) -> ComputerUseResult<(OverlayGeometry, BorderGeometries)> {
    let (monitor, display_rect) = monitor_work_area(target_rect).ok_or_else(target_unavailable)?;
    let dpi = monitor_dpi(monitor, Some(target));
    Ok((
        banner_geometry(target_rect, &display_rect, dpi),
        border_geometries(target_rect, dpi),
    ))
}

fn banner_geometry(
    rect: &windows::Win32::Foundation::RECT,
    display_rect: &windows::Win32::Foundation::RECT,
    dpi: u32,
) -> OverlayGeometry {
    let target_width = rect.right.saturating_sub(rect.left).max(1);
    let display_width = display_rect.right.saturating_sub(display_rect.left).max(1);
    let display_height = display_rect.bottom.saturating_sub(display_rect.top).max(1);
    let width = target_width
        .max(scaled_pixels(320, dpi))
        .min(scaled_pixels(720, dpi))
        .min(display_width);
    let height = scaled_pixels(36, dpi).min(display_height);
    let centered_x = rect
        .left
        .saturating_add(target_width.saturating_sub(width) / 2);
    let x = centered_x.clamp(display_rect.left, display_rect.right.saturating_sub(width));
    let y = rect
        .top
        .saturating_add(scaled_pixels(8, dpi))
        .clamp(display_rect.top, display_rect.bottom.saturating_sub(height));
    (x, y, width, height)
}

fn border_geometries(rect: &windows::Win32::Foundation::RECT, dpi: u32) -> BorderGeometries {
    let thickness = scaled_pixels(BORDER_THICKNESS, dpi);
    let width = rect
        .right
        .saturating_sub(rect.left)
        .max(thickness.saturating_mul(2));
    let height = rect
        .bottom
        .saturating_sub(rect.top)
        .max(thickness.saturating_mul(2));
    [
        (rect.left, rect.top, width, thickness),
        (
            rect.left,
            rect.bottom.saturating_sub(thickness),
            width,
            thickness,
        ),
        (rect.left, rect.top, thickness, height),
        (
            rect.right.saturating_sub(thickness),
            rect.top,
            thickness,
            height,
        ),
    ]
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

struct PointerEffect {
    hwnd: HWND,
}

impl PointerEffect {
    fn new(screen_x: i32, screen_y: i32, glyph: &str) -> ComputerUseResult<Self> {
        let (x, y, size) = pointer_effect_geometry(screen_x, screen_y);
        let hwnd = create_static_overlay(
            glyph,
            WS_BORDER.0 | STATIC_CENTER | STATIC_CENTER_IMAGE,
            (x, y, size, size),
            235,
        )?;
        Ok(Self { hwnd })
    }

    fn reposition(&self, screen_x: i32, screen_y: i32) -> ComputerUseResult<()> {
        let (x, y, size) = pointer_effect_geometry(screen_x, screen_y);
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

fn pointer_effect_geometry(screen_x: i32, screen_y: i32) -> (i32, i32, i32) {
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
    )
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
            let effect = PointerEffect::new(screen_x, screen_y, "●")?;
            click(
                window_handle,
                observation,
                screen_x,
                screen_y,
                request.button.as_deref().unwrap_or("left"),
                &guard,
            )?;
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "double_click" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(window_handle, observation, point, &guard, true)?;
            let effect = PointerEffect::new(screen_x, screen_y, "◎")?;
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
            effect.dwell(&guard, pointer_effect_dwell(request))?;
        }
        "scroll" => {
            let point = required_point(request)?;
            let (screen_x, screen_y) = move_to(window_handle, observation, point, &guard, true)?;
            let effect = PointerEffect::new(screen_x, screen_y, "↕")?;
            scroll(
                window_handle,
                observation,
                screen_x,
                screen_y,
                request.scroll_x.unwrap_or(0),
                request.scroll_y.unwrap_or(0),
                &guard,
            )?;
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
    ensure_target_foreground(window_handle, observation.process_id)?;
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
    ensure_target_foreground(window_handle, process_id)?;
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

fn flush_pending_input_releases_locked() -> ComputerUseResult<()> {
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
mod tests {
    use super::*;

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

        flush_pending_input_releases_with(&mut pending, |batch| batch.len() as u32, || true)
            .unwrap();
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
        };

        assert!(ensure_observation_target_state(&observation, [10, 20, 800, 600], 144).is_ok());
        let error =
            ensure_observation_target_state(&observation, [10, 20, 800, 600], 192).unwrap_err();
        assert_eq!(error.code, ComputerUseErrorCode::StaleObservation);
        assert!(error.message.contains("DPI"));
    }

    #[test]
    fn control_border_tracks_all_target_edges() {
        let rect = windows::Win32::Foundation::RECT {
            left: -100,
            top: 20,
            right: 900,
            bottom: 620,
        };

        assert_eq!(
            border_geometries(&rect, 96),
            [
                (-100, 20, 1000, BORDER_THICKNESS),
                (-100, 615, 1000, BORDER_THICKNESS),
                (-100, 20, BORDER_THICKNESS, 600),
                (895, 20, BORDER_THICKNESS, 600),
            ]
        );
    }

    #[test]
    fn overlay_pixels_scale_at_common_monitor_dpis() {
        assert_eq!(scaled_pixels(36, 96), 36);
        assert_eq!(scaled_pixels(36, 144), 54);
        assert_eq!(scaled_pixels(36, 192), 72);
        assert_eq!(scaled_pixels(BORDER_THICKNESS, 96), 5);
        assert_eq!(scaled_pixels(BORDER_THICKNESS, 144), 8);
        assert_eq!(scaled_pixels(BORDER_THICKNESS, 192), 10);
        assert_eq!(scaled_pixels(POINTER_EFFECT_SIZE, 96), 34);
        assert_eq!(scaled_pixels(POINTER_EFFECT_SIZE, 144), 51);
        assert_eq!(scaled_pixels(POINTER_EFFECT_SIZE, 192), 68);
    }

    #[test]
    fn banner_stays_on_a_negative_origin_monitor() {
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

        assert_eq!(banner_geometry(&target, &display, 96), (-1760, 28, 720, 36));
    }

    #[test]
    fn banner_clamps_partial_offscreen_targets_to_monitor_work_area() {
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

        assert_eq!(banner_geometry(&target, &display, 144), (-1920, 0, 480, 54));
    }

    #[test]
    fn banner_clamps_cross_gap_targets_to_the_selected_real_monitor() {
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

        assert_eq!(banner_geometry(&target, &display, 96), (2560, 200, 720, 36));
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
            GWL_EXSTYLE, GetWindowLongPtrW, GetWindowTextW, IsWindowVisible,
        };

        let _dpi_awareness = ThreadDpiAwareness::enter().unwrap();
        let effect = PointerEffect::new(200, 240, "●").unwrap();

        assert!(unsafe { IsWindowVisible(effect.hwnd) }.as_bool());

        let mut rect = windows::Win32::Foundation::RECT::default();
        unsafe { GetWindowRect(effect.hwnd, &mut rect) }.unwrap();
        let (_, _, expected_size) = pointer_effect_geometry(200, 240);
        assert_eq!(rect.right - rect.left, expected_size);
        assert_eq!(rect.bottom - rect.top, expected_size);

        let ex_style = unsafe { GetWindowLongPtrW(effect.hwnd, GWL_EXSTYLE) } as u32;
        for required in [WS_EX_NOACTIVATE.0, WS_EX_TRANSPARENT.0, WS_EX_TOPMOST.0] {
            assert_eq!(ex_style & required, required);
        }

        let mut caption = [0_u16; 8];
        let length = unsafe { GetWindowTextW(effect.hwnd, &mut caption) } as usize;
        assert_eq!(String::from_utf16_lossy(&caption[..length]), "●");
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

        let wrong_process =
            ensure_target_foreground(handle, process_id.saturating_add(1)).unwrap_err();
        assert_eq!(wrong_process.code, ComputerUseErrorCode::InvalidTarget);

        let occluded =
            ensure_point_targets_window(200, 240, effect.hwnd, process_id.saturating_add(1))
                .unwrap_err();
        assert_eq!(occluded.code, ComputerUseErrorCode::InvalidTarget);

        let same_process_wrong_window =
            ensure_point_targets_window(200, 240, other_window.hwnd, process_id).unwrap_err();
        assert_eq!(
            same_process_wrong_window.code,
            ComputerUseErrorCode::InvalidTarget
        );

        let _ = unsafe { ShowWindow(effect.hwnd, SW_HIDE) };
        let hidden = available_target_rect(effect.hwnd).unwrap_err();
        assert_eq!(hidden.code, ComputerUseErrorCode::MissingWindow);
        assert!(validate_target_identity(effect.hwnd, process_id).is_ok());
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
}
