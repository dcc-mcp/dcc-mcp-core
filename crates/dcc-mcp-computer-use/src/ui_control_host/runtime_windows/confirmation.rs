use std::cell::RefCell;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use dcc_mcp_ui_control::host_protocol::{
    UiControlAction, UiControlHostErrorCode, UiControlPolicyTier, UiControlSystemOperation,
    UiControlTarget,
};
use uuid::Uuid;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DestroyWindow, EndDialog, GetClassNameW, GetWindowTextW,
    GetWindowThreadProcessId, HCBT_ACTIVATE, HHOOK, IDNO, IDYES, IsWindow, MB_DEFBUTTON2,
    MB_ICONWARNING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MessageBoxW, PostMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_CBT, WM_CLOSE,
};
use windows::core::PCWSTR;

use super::super::{ConfirmationKind, ConfirmationSurface, HostFailure};
use super::wide;

const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(30);

pub(in crate::ui_control_host) struct WindowsConfirmationSurface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConfirmationDialogWindow {
    window_handle: u64,
    thread_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConfirmationOutcome {
    approved: bool,
    dialog: ConfirmationDialogWindow,
}

#[derive(Debug, Clone, Default)]
struct ConfirmationWatchdogState {
    dialog: Option<ConfirmationDialogWindow>,
    expired: bool,
    cancelled: bool,
    capture_error: Option<String>,
    close_error: Option<String>,
}

struct ConfirmationWatchdogShared {
    expected_caption: String,
    state: Mutex<ConfirmationWatchdogState>,
    changed: Condvar,
}

struct ConfirmationHookState {
    shared: Arc<ConfirmationWatchdogShared>,
}

thread_local! {
    static CONFIRMATION_HOOK_STATE: RefCell<Option<ConfirmationHookState>> = const { RefCell::new(None) };
}

struct ConfirmationHook(HHOOK);

impl Drop for ConfirmationHook {
    fn drop(&mut self) {
        let _ = unsafe { UnhookWindowsHookEx(self.0) };
    }
}

struct ConfirmationHookStateGuard {
    active: bool,
}

impl ConfirmationHookStateGuard {
    fn take(mut self) -> Option<ConfirmationHookState> {
        self.active = false;
        CONFIRMATION_HOOK_STATE.with(|state| state.borrow_mut().take())
    }
}

impl Drop for ConfirmationHookStateGuard {
    fn drop(&mut self) {
        if self.active {
            CONFIRMATION_HOOK_STATE.with(|state| {
                state.borrow_mut().take();
            });
        }
    }
}

unsafe extern "system" fn confirmation_cbt_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    if code == HCBT_ACTIVATE as i32 {
        let dialog = ConfirmationDialogWindow {
            window_handle: wparam.0 as u64,
            thread_id: unsafe { GetCurrentThreadId() },
        };
        CONFIRMATION_HOOK_STATE.with(|state| {
            let Ok(state) = state.try_borrow() else {
                return;
            };
            let Some(state) = state.as_ref() else {
                return;
            };
            capture_confirmation_dialog(&state.shared, dialog);
        });
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn close_confirmation_dialog_on_owner_thread(hwnd: HWND) -> windows::core::Result<()> {
    if unsafe { EndDialog(hwnd, IDNO.0 as isize) }.is_ok() {
        return Ok(());
    }
    unsafe { DestroyWindow(hwnd) }
}

fn watchdog_state(
    shared: &ConfirmationWatchdogShared,
) -> std::sync::MutexGuard<'_, ConfirmationWatchdogState> {
    shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn capture_confirmation_dialog(
    shared: &Arc<ConfirmationWatchdogShared>,
    dialog: ConfirmationDialogWindow,
) {
    let Ok(hwnd) = validate_confirmation_dialog_owner(dialog) else {
        return;
    };
    // This CBT hook is installed only on the current host request thread and
    // only for the duration of this MessageBoxW call. Capture its first owned
    // activation even if class/caption inspection fails so an identity error
    // can close that exact HWND immediately instead of leaving an unbounded
    // modal surface behind.
    let identity_error = (confirmation_window_class(hwnd) != "#32770"
        || confirmation_window_text(hwnd) != shared.expected_caption)
        .then(|| "the trusted UI Control confirmation HWND identity changed".to_owned());
    let should_close = {
        let mut state = watchdog_state(shared);
        if state.dialog.is_some() {
            return;
        }
        state.dialog = Some(dialog);
        if let Some(message) = identity_error {
            state.capture_error = Some(message);
            state.expired = true;
        }
        let should_close = state.expired;
        shared.changed.notify_all();
        should_close
    };
    if should_close && let Err(error) = close_confirmation_dialog_on_owner_thread(hwnd) {
        let mut state = watchdog_state(shared);
        state.close_error = Some(format!(
            "close the invalid or expired UI Control confirmation surface: {error}"
        ));
        shared.changed.notify_all();
    }
}

fn confirmation_window_text(hwnd: HWND) -> String {
    let mut text = vec![0_u16; 512];
    let copied = unsafe { GetWindowTextW(hwnd, &mut text) }.max(0) as usize;
    text.truncate(copied);
    String::from_utf16_lossy(&text)
}

fn confirmation_window_class(hwnd: HWND) -> String {
    let mut class_name = [0_u16; 64];
    let copied = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
    String::from_utf16_lossy(&class_name[..copied])
}

fn validate_confirmation_dialog(
    dialog: ConfirmationDialogWindow,
    expected_caption: &str,
) -> Result<HWND, HostFailure> {
    let hwnd = validate_confirmation_dialog_owner(dialog)?;
    if confirmation_window_class(hwnd) != "#32770"
        || confirmation_window_text(hwnd) != expected_caption
    {
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "the trusted UI Control confirmation HWND identity changed",
        ));
    }
    Ok(hwnd)
}

fn validate_confirmation_dialog_owner(
    dialog: ConfirmationDialogWindow,
) -> Result<HWND, HostFailure> {
    let hwnd = HWND(dialog.window_handle as usize as *mut core::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "the trusted UI Control confirmation HWND no longer exists",
        ));
    }
    let mut process_id = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if thread_id != dialog.thread_id || process_id != unsafe { GetCurrentProcessId() } {
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "the trusted UI Control confirmation HWND changed ownership",
        ));
    }
    Ok(hwnd)
}

fn confirmation_dialog_still_owned(dialog: ConfirmationDialogWindow) -> bool {
    let hwnd = HWND(dialog.window_handle as usize as *mut core::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return false;
    }
    let mut process_id = 0;
    (unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) }) == dialog.thread_id
        && process_id == unsafe { GetCurrentProcessId() }
}

struct ConfirmationWatchdog {
    shared: Arc<ConfirmationWatchdogShared>,
    deadline: Instant,
    thread: Option<JoinHandle<()>>,
}

impl ConfirmationWatchdog {
    fn start(expected_caption: String, timeout: Duration) -> Result<Self, HostFailure> {
        let shared = Arc::new(ConfirmationWatchdogShared {
            expected_caption,
            state: Mutex::new(ConfirmationWatchdogState::default()),
            changed: Condvar::new(),
        });
        let deadline = Instant::now() + timeout;
        let worker_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("dcc-mcp-ui-control-confirmation-watchdog".to_owned())
            .spawn(move || run_confirmation_watchdog(&worker_shared, deadline))
            .map_err(|error| {
                HostFailure::new(
                    UiControlHostErrorCode::BackendUnavailable,
                    format!("start the trusted confirmation expiry watchdog: {error}"),
                )
            })?;
        Ok(Self {
            shared,
            deadline,
            thread: Some(thread),
        })
    }

    fn finish(mut self) -> Result<ConfirmationWatchdogState, HostFailure> {
        {
            let mut state = watchdog_state(&self.shared);
            state.cancelled = true;
            self.shared.changed.notify_all();
        }
        let thread = self.thread.take().ok_or_else(|| {
            HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                "the trusted UI Control confirmation watchdog state disappeared",
            )
        })?;
        thread.join().map_err(|_| {
            HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                "the trusted UI Control confirmation watchdog panicked",
            )
        })?;
        Ok(watchdog_state(&self.shared).clone())
    }
}

impl Drop for ConfirmationWatchdog {
    fn drop(&mut self) {
        {
            let mut state = watchdog_state(&self.shared);
            state.cancelled = true;
            self.shared.changed.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_confirmation_watchdog(shared: &Arc<ConfirmationWatchdogShared>, deadline: Instant) {
    let dialog = {
        let mut state = watchdog_state(shared);
        loop {
            if state.cancelled {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.expired = true;
                let dialog = state.dialog;
                shared.changed.notify_all();
                break dialog;
            }
            let (next, _) = shared
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }
    };
    if let Some(dialog) = dialog {
        expire_confirmation_dialog(shared, dialog);
    }
}

fn expire_confirmation_dialog(
    shared: &Arc<ConfirmationWatchdogShared>,
    dialog: ConfirmationDialogWindow,
) {
    let hwnd = match validate_confirmation_dialog(dialog, &shared.expected_caption) {
        Ok(hwnd) => hwnd,
        Err(_) if !confirmation_dialog_still_owned(dialog) => return,
        Err(failure) => {
            let mut state = watchdog_state(shared);
            state.close_error = Some(failure.message);
            shared.changed.notify_all();
            return;
        }
    };
    if unsafe { EndDialog(hwnd, IDNO.0 as isize) }.is_ok()
        || !confirmation_dialog_still_owned(dialog)
    {
        return;
    }
    if let Err(error) = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
        let mut state = watchdog_state(shared);
        state.close_error = Some(format!(
            "close the expired UI Control confirmation surface: {error}"
        ));
        shared.changed.notify_all();
    }
}

fn run_bounded_confirmation_dialog(
    caption: String,
    message: Vec<u16>,
    operator_timeout: Duration,
) -> Result<ConfirmationOutcome, HostFailure> {
    run_bounded_confirmation_dialog_with_identity(
        caption.clone(),
        caption,
        message,
        operator_timeout,
    )
}

fn run_bounded_confirmation_dialog_with_identity(
    caption: String,
    expected_caption: String,
    message: Vec<u16>,
    operator_timeout: Duration,
) -> Result<ConfirmationOutcome, HostFailure> {
    let thread_id = unsafe { GetCurrentThreadId() };
    let watchdog = ConfirmationWatchdog::start(expected_caption, operator_timeout)?;
    let deadline = watchdog.deadline;
    let state_was_installed = CONFIRMATION_HOOK_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.is_some() {
            return false;
        }
        *state = Some(ConfirmationHookState {
            shared: Arc::clone(&watchdog.shared),
        });
        true
    });
    if !state_was_installed {
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "a trusted UI Control confirmation is already active on this thread",
        ));
    }
    let state_guard = ConfirmationHookStateGuard { active: true };
    let hook = unsafe { SetWindowsHookExW(WH_CBT, Some(confirmation_cbt_hook), None, thread_id) }
        .map_err(|error| {
        HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            format!("install the trusted confirmation HWND hook: {error}"),
        )
    })?;
    let hook = ConfirmationHook(hook);
    let wide_caption = wide(&caption);
    let result = unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(wide_caption.as_ptr()),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
    let message_box_error =
        (result.0 == 0).then(|| windows::core::Error::from_thread().to_string());
    let completed_at = Instant::now();
    drop(hook);
    state_guard.take().ok_or_else(|| {
        HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "the trusted UI Control confirmation state disappeared",
        )
    })?;
    let state = watchdog.finish()?;
    if let Some(dialog) = state.dialog
        && confirmation_dialog_still_owned(dialog)
    {
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "the trusted UI Control confirmation call returned without destroying its HWND",
        ));
    }
    if result.0 == 0 {
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            format!(
                "show the trusted UI Control confirmation surface: {}",
                message_box_error
                    .as_deref()
                    .unwrap_or("unknown Win32 error")
            ),
        ));
    }
    let dialog = state.dialog.ok_or_else(|| {
        HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "the trusted UI Control confirmation HWND was not captured",
        )
    })?;
    if let Some(message) = state.capture_error.as_deref() {
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            message,
        ));
    }
    if let Some(message) = state.close_error.as_deref() {
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            message,
        ));
    }
    Ok(ConfirmationOutcome {
        approved: confirmation_result_is_approved(
            result == IDYES,
            completed_at,
            deadline,
            state.expired,
        ),
        dialog,
    })
}

fn confirmation_result_is_approved(
    selected_yes: bool,
    completed_at: Instant,
    deadline: Instant,
    expired: bool,
) -> bool {
    selected_yes && completed_at < deadline && !expired
}

impl ConfirmationSurface for WindowsConfirmationSurface {
    fn confirm(
        &self,
        kind: ConfirmationKind<'_>,
        target: Option<&UiControlTarget>,
        action: Option<&UiControlAction>,
    ) -> Result<bool, HostFailure> {
        let (heading, detail) = match kind {
            ConfirmationKind::ConsequentialAction(UiControlPolicyTier::PreApproval) => (
                "Approve this DCC UI Control action?",
                "The action may sign in, upload, move, rename, or transmit identified data.",
            ),
            ConfirmationKind::ConsequentialAction(_) => (
                "Confirm this consequential DCC action?",
                "The action may delete, overwrite, install, purchase, change access, or submit content.",
            ),
            ConfirmationKind::SystemOperation(_) => (
                "Confirm this Windows configuration change?",
                "The operation will ensure one exact operator-approved HKCU value or symbolic link.",
            ),
            ConfirmationKind::ResumeAfterStop => (
                "Resume DCC UI Control?",
                "The global Esc stop latch will be cleared for this Windows session.",
            ),
        };
        let action_name = match kind {
            ConfirmationKind::SystemOperation(operation) => operation.audit_name(),
            _ => action
                .map(|value| value.action.as_str())
                .unwrap_or("open_session"),
        };
        let (scope, privacy) = match kind {
            ConfirmationKind::SystemOperation(operation) => (
                system_operation_confirmation_scope(operation),
                "Registry value data is hidden. The shown key or paths are not written to audit logs.",
            ),
            _ => (
                target.map_or_else(
                    || "Scope: current Windows user (no DCC window required)".to_owned(),
                    |target| {
                        format!(
                            "Application window: {}\nProcess: {}",
                            target.window_title, target.process_id
                        )
                    },
                ),
                "Sensitive text and coordinates are not shown or logged.",
            ),
        };
        let message = format!(
            "{detail}\n\n{scope}\nAction: {action_name}\n\n{privacy}\n\nThis confirmation expires after 30 seconds."
        );
        let message = wide(&message);
        let request_marker = Uuid::new_v4().simple().to_string();
        let caption = format!("{heading} [{}]", &request_marker[..8]);
        run_bounded_confirmation_dialog(caption, message, CONFIRMATION_TIMEOUT)
            .map(|outcome| outcome.approved)
    }
}

fn system_operation_confirmation_scope(operation: &UiControlSystemOperation) -> String {
    match operation {
        UiControlSystemOperation::EnsureRegistryString {
            key, value_name, ..
        }
        | UiControlSystemOperation::EnsureRegistryDword {
            key, value_name, ..
        } => format!(
            "Scope: current Windows user\nRegistry key: HKEY_CURRENT_USER\\{key}\nValue name: {}",
            if value_name.is_empty() {
                "(Default)"
            } else {
                value_name
            }
        ),
        UiControlSystemOperation::EnsureFileSymlink { link, target }
        | UiControlSystemOperation::EnsureDirectorySymlink { link, target } => {
            format!("Scope: current Windows user\nLink path: {link}\nTarget path: {target}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_confirmation_timeout_closes_exact_hwnd_on_the_calling_thread() {
        if !crate::platform::desktop_interactive() {
            eprintln!("confirmation lifecycle test skipped: Windows desktop is not interactive");
            return;
        }
        let caption = format!(
            "DCC MCP UI Control lifecycle test [{}]",
            &Uuid::new_v4().simple().to_string()[..8]
        );
        let started = Instant::now();

        let outcome = run_bounded_confirmation_dialog(
            caption,
            wide("This test confirmation must close automatically."),
            Duration::from_millis(100),
        )
        .unwrap();

        assert!(!outcome.approved);
        assert_ne!(outcome.dialog.window_handle, 0);
        assert_eq!(outcome.dialog.thread_id, unsafe { GetCurrentThreadId() });
        assert!(!confirmation_dialog_still_owned(outcome.dialog));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn confirmation_identity_failure_closes_the_exact_owned_hwnd_immediately() {
        if !crate::platform::desktop_interactive() {
            eprintln!("confirmation lifecycle test skipped: Windows desktop is not interactive");
            return;
        }
        let caption = format!(
            "DCC MCP UI Control identity failure test [{}]",
            &Uuid::new_v4().simple().to_string()[..8]
        );
        let started = Instant::now();

        let failure = run_bounded_confirmation_dialog_with_identity(
            caption,
            "intentionally wrong confirmation caption".to_owned(),
            wide("This mismatched test confirmation must fail closed immediately."),
            Duration::from_secs(3),
        )
        .unwrap_err();

        assert_eq!(failure.code, UiControlHostErrorCode::BackendUnavailable);
        assert!(failure.message.contains("HWND identity changed"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn confirmation_watchdog_expires_and_joins_without_an_hwnd() {
        let watchdog = ConfirmationWatchdog::start(
            "DCC MCP UI Control missing HWND test".to_owned(),
            Duration::from_millis(25),
        )
        .unwrap();
        {
            let state = watchdog_state(&watchdog.shared);
            let (state, wait_result) = watchdog
                .shared
                .changed
                .wait_timeout_while(state, Duration::from_secs(5), |state| !state.expired)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(!wait_result.timed_out(), "watchdog did not expire");
            assert!(state.expired);
        }

        let state = watchdog.finish().unwrap();

        assert!(state.expired);
        assert!(state.dialog.is_none());
    }

    #[test]
    fn confirmation_yes_at_or_after_deadline_is_never_approved() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(10);

        assert!(confirmation_result_is_approved(true, now, deadline, false));
        assert!(!confirmation_result_is_approved(
            true, deadline, deadline, false
        ));
        assert!(!confirmation_result_is_approved(
            true,
            deadline + Duration::from_millis(1),
            deadline,
            false
        ));
        assert!(!confirmation_result_is_approved(true, now, deadline, true));
    }
}
