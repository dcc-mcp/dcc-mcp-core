use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use dcc_mcp_app_ui::host_protocol::{
    UiControlAction, UiControlHostErrorCode, UiControlInputKind, UiControlPolicyTier,
    UiControlSharedImage, UiControlTarget, UiControlTaskGrant, UiControlWindowOperation,
    UiControlWindowState,
};
use dcc_mcp_capture::{CaptureTarget, WindowFinder};
use dcc_mcp_shm::SharedBuffer;
use serde_json::{Value, json};
use uuid::Uuid;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::UI::WindowsAndMessaging::{
    GetPropW, GetWindowThreadProcessId, IDYES, IsWindow, MB_DEFBUTTON2, MB_ICONWARNING,
    MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MessageBoxW, RemovePropW, SetPropW,
};
use windows::core::PCWSTR;

use crate::{
    ComputerUseAction, ComputerUseError, ComputerUseErrorCode, ComputerUsePoint,
    ComputerUseSession, ComputerUseTargetScope,
};

use super::{
    ConfirmationKind, ConfirmationSurface, HostFailure, HostRuntime, HostRuntimeSession,
    RuntimeActionResult, RuntimeSnapshot,
};

const UIA_TIMEOUT: Duration = Duration::from_secs(30);
const UIA_SCRIPT: &str =
    include_str!("../../../../python/dcc_mcp_core/skills/app-ui/scripts/_windows_uia_backend.ps1");
const UIA_HELPERS: &str =
    include_str!("../../../../python/dcc_mcp_core/skills/app-ui/scripts/_windows_uia_helpers.ps1");

pub(super) struct WindowsHostRuntime;

impl HostRuntime for WindowsHostRuntime {
    fn open(&self, grant: &UiControlTaskGrant) -> Result<Box<dyn HostRuntimeSession>, HostFailure> {
        let target = resolve_exact_target(grant)?;
        let trusted_scope =
            ComputerUseTargetScope::new(Some(target.process_id), Some(target.window_handle))
                .map_err(map_computer_use_error)?;
        let session = ComputerUseSession::new(
            trusted_scope,
            Some(target.process_id),
            Some(target.window_handle),
            None,
            Some(grant.dcc_type.clone()),
        )
        .map_err(map_computer_use_error)?;
        let window_generation = WindowGenerationGuard::bind(&target)?;
        Ok(Box::new(WindowsRuntimeSession {
            session,
            target,
            window_generation,
            started: false,
            image_buffer: None,
        }))
    }
}

struct WindowsRuntimeSession {
    session: ComputerUseSession,
    target: UiControlTarget,
    window_generation: WindowGenerationGuard,
    started: bool,
    image_buffer: Option<SharedBuffer>,
}

impl HostRuntimeSession for WindowsRuntimeSession {
    fn target(&self) -> &UiControlTarget {
        &self.target
    }

    fn window_state(&mut self) -> Result<UiControlWindowState, HostFailure> {
        self.window_generation.verify()?;
        let state =
            crate::platform::scoped_window_state(self.target.window_handle, self.target.process_id)
                .map_err(map_computer_use_error)?;
        Ok(protocol_window_state(state))
    }

    fn change_window_state(
        &mut self,
        operation: UiControlWindowOperation,
    ) -> Result<UiControlWindowState, HostFailure> {
        self.window_generation.verify()?;
        let operation = match operation {
            UiControlWindowOperation::Restore => crate::platform::ScopedWindowOperation::Restore,
            UiControlWindowOperation::Show => crate::platform::ScopedWindowOperation::Show,
            UiControlWindowOperation::Activate => crate::platform::ScopedWindowOperation::Activate,
        };
        let state = crate::platform::transition_scoped_window(
            self.target.window_handle,
            self.target.process_id,
            operation,
        )
        .map_err(map_computer_use_error)?;
        self.window_generation.verify()?;
        Ok(protocol_window_state(state))
    }

    fn snapshot(&mut self, max_depth: u32, max_nodes: u32) -> Result<RuntimeSnapshot, HostFailure> {
        self.window_generation.verify()?;
        if !self.started {
            let status = self.session.start().map_err(map_computer_use_error)?;
            self.target = target_from_status(&status)?;
            self.started = true;
        }
        let screenshot = self.session.screenshot().map_err(map_computer_use_error)?;
        self.window_generation.verify()?;
        let observation = serde_json::to_value(&screenshot.observation).map_err(|error| {
            HostFailure::new(
                UiControlHostErrorCode::CaptureFailed,
                format!("serialize the native screenshot observation: {error}"),
            )
        })?;
        self.target = UiControlTarget {
            process_id: screenshot.observation.process_id,
            window_handle: screenshot.observation.window_handle,
            window_title: screenshot.observation.window_title.clone(),
        };
        let raw = run_uia(json!({
            "mode": "snapshot",
            "scope": exact_scope(&self.target),
            "max_depth": max_depth,
            "max_nodes": max_nodes,
        }))?;
        ensure_uia_ok(&raw, "Windows UI Automation snapshot failed")?;
        let root = raw.get("root").cloned().ok_or_else(|| {
            HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                "Windows UI Automation returned no scoped root",
            )
        })?;

        let buffer_id = Uuid::new_v4().simple().to_string()[..16].to_owned();
        let buffer =
            SharedBuffer::create_with_id(buffer_id, screenshot.data.len()).map_err(|error| {
                HostFailure::new(
                    UiControlHostErrorCode::CaptureFailed,
                    format!("create the screenshot shared-memory buffer: {error}"),
                )
            })?;
        buffer.write(&screenshot.data).map_err(|error| {
            HostFailure::new(
                UiControlHostErrorCode::CaptureFailed,
                format!("write the screenshot shared-memory buffer: {error}"),
            )
        })?;
        let image = UiControlSharedImage {
            name: buffer.name(),
            id: buffer.id.clone(),
            length: screenshot.data.len(),
            mime_type: "image/png".to_owned(),
        };
        self.image_buffer = Some(buffer);

        Ok(RuntimeSnapshot {
            observation_id: screenshot.observation.observation_id,
            target: self.target.clone(),
            observation,
            root,
            focus_runtime_id: raw
                .get("focus_runtime_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            node_count: raw
                .get("node_count")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(1),
            image,
        })
    }

    fn execute(
        &mut self,
        observation_id: &str,
        action: &UiControlAction,
    ) -> Result<RuntimeActionResult, HostFailure> {
        self.window_generation.verify()?;
        self.image_buffer = None;
        let result = match action.input_kind {
            UiControlInputKind::RawInput => {
                let request = native_action(action, observation_id);
                self.session
                    .perform(&request)
                    .map_err(map_computer_use_error)?;
                Ok(RuntimeActionResult {
                    message: format!("completed scoped native action {:?}", action.action),
                    before_focus_runtime_id: None,
                    after_focus_runtime_id: None,
                })
            }
            UiControlInputKind::Semantic => {
                self.session
                    .prepare_semantic_action(observation_id)
                    .map_err(map_computer_use_error)?;
                let raw = run_uia(json!({
                    "mode": "act",
                    "scope": exact_scope(&self.target),
                    "max_depth": 12,
                    "max_nodes": 2000,
                    "action": {
                        "control_id": action.control_id,
                        "action": action.action,
                        "text": action.text.as_deref().unwrap_or(""),
                        "checked": action.checked.unwrap_or(false),
                    },
                }))?;
                ensure_uia_ok(&raw, "Windows UI Automation action failed")?;
                Ok(RuntimeActionResult {
                    message: raw
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("completed scoped Windows UI Automation action")
                        .to_owned(),
                    before_focus_runtime_id: optional_string(&raw, "before_focus_runtime_id"),
                    after_focus_runtime_id: optional_string(&raw, "after_focus_runtime_id"),
                })
            }
        };
        self.window_generation.verify()?;
        result
    }

    fn resume_after_approval(&mut self) -> Result<(), HostFailure> {
        if self.started {
            let stopped = self.session.stop();
            if stopped
                .get("cleanup_pending")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Err(HostFailure::new(
                    UiControlHostErrorCode::BackendUnavailable,
                    "the previous input owner or overlay is still cleaning up; retry shortly",
                ));
            }
            self.started = false;
        }
        let resumed = self.session.resume_after_user_approval();
        if !resumed
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(failure_from_value(&resumed, "resume UI Control"));
        }
        Ok(())
    }

    fn stop(&mut self) -> bool {
        self.image_buffer = None;
        if !self.started {
            return false;
        }
        self.started = false;
        self.session
            .stop()
            .get("cleanup_pending")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
}

struct WindowGenerationGuard {
    window_handle: u64,
    process_id: u32,
    property_name: Vec<u16>,
    marker: usize,
}

impl WindowGenerationGuard {
    fn bind(target: &UiControlTarget) -> Result<Self, HostFailure> {
        let hwnd = HWND(target.window_handle as usize as *mut core::ffi::c_void);
        let property_name = wide(&format!(
            "DccMcpUiControlHostWindowGeneration-{}",
            Uuid::new_v4().simple()
        ));
        let marker_pointer = std::ptr::dangling_mut::<u8>().cast::<core::ffi::c_void>();
        let marker = marker_pointer as usize;
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool()
            || unsafe {
                SetPropW(
                    hwnd,
                    PCWSTR(property_name.as_ptr()),
                    Some(HANDLE(marker_pointer)),
                )
            }
            .is_err()
        {
            return Err(HostFailure::new(
                UiControlHostErrorCode::InvalidTarget,
                "the host could not bind a generation marker to the exact target window",
            ));
        }
        let guard = Self {
            window_handle: target.window_handle,
            process_id: target.process_id,
            property_name,
            marker,
        };
        guard.verify()?;
        Ok(guard)
    }

    fn verify(&self) -> Result<(), HostFailure> {
        let hwnd = HWND(self.window_handle as usize as *mut core::ffi::c_void);
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err(HostFailure::new(
                UiControlHostErrorCode::InvalidTarget,
                "the exact target window no longer exists",
            ));
        }
        let mut process_id = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
        let marker = unsafe { GetPropW(hwnd, PCWSTR(self.property_name.as_ptr())) };
        if process_id != self.process_id || marker.0 as usize != self.marker {
            return Err(HostFailure::new(
                UiControlHostErrorCode::InvalidTarget,
                "the exact target HWND generation changed",
            ));
        }
        Ok(())
    }
}

impl Drop for WindowGenerationGuard {
    fn drop(&mut self) {
        let hwnd = HWND(self.window_handle as usize as *mut core::ffi::c_void);
        if unsafe { GetPropW(hwnd, PCWSTR(self.property_name.as_ptr())) }.0 as usize == self.marker
        {
            let _ = unsafe { RemovePropW(hwnd, PCWSTR(self.property_name.as_ptr())) };
        }
    }
}

fn resolve_exact_target(grant: &UiControlTaskGrant) -> Result<UiControlTarget, HostFailure> {
    let finder = WindowFinder::new();
    let info = if let Some(window_handle) = grant.window_handle {
        finder.info_for_handle(window_handle)
    } else if let Some(process_id) = grant.process_id {
        finder.find(&CaptureTarget::ProcessId(process_id))
    } else {
        return Err(HostFailure::new(
            UiControlHostErrorCode::InvalidTarget,
            "an exact PID or HWND scope is required",
        ));
    }
    .map_err(|error| HostFailure::new(UiControlHostErrorCode::InvalidTarget, error.to_string()))?;
    if grant
        .process_id
        .is_some_and(|process_id| process_id != info.pid)
    {
        return Err(HostFailure::new(
            UiControlHostErrorCode::InvalidTarget,
            "the granted HWND is not owned by the granted process",
        ));
    }
    crate::platform::validate_target_policy(info.handle, info.pid, &info.title)
        .map_err(map_computer_use_error)?;
    Ok(UiControlTarget {
        process_id: info.pid,
        window_handle: info.handle,
        window_title: if info.title.is_empty() {
            "DCC application".to_owned()
        } else {
            info.title
        },
    })
}

fn protocol_window_state(state: crate::platform::ScopedWindowState) -> UiControlWindowState {
    UiControlWindowState {
        process_id: state.process_id,
        window_handle: state.window_handle,
        exists: state.exists,
        visible: state.visible,
        minimized: state.minimized,
        foreground: state.foreground,
    }
}

pub(super) struct WindowsConfirmationSurface;

impl ConfirmationSurface for WindowsConfirmationSurface {
    fn confirm(
        &self,
        kind: ConfirmationKind,
        target: &UiControlTarget,
        action: Option<&UiControlAction>,
    ) -> Result<bool, HostFailure> {
        let (heading, detail) = match kind {
            ConfirmationKind::TaskGrant => (
                "Allow DCC UI Control?",
                "This grants visible, single-window control until the session is stopped.",
            ),
            ConfirmationKind::ConsequentialAction(UiControlPolicyTier::PreApproval) => (
                "Approve this DCC UI Control action?",
                "The action may sign in, upload, move, rename, or transmit identified data.",
            ),
            ConfirmationKind::ConsequentialAction(_) => (
                "Confirm this consequential DCC action?",
                "The action may delete, overwrite, install, purchase, change access, or submit content.",
            ),
            ConfirmationKind::ResumeAfterStop => (
                "Resume DCC UI Control?",
                "The global Esc stop latch will be cleared for this Windows session.",
            ),
        };
        let action_name = action
            .map(|value| value.action.as_str())
            .unwrap_or("open_session");
        let message = format!(
            "{detail}\n\nApplication window: {}\nProcess: {}\nAction: {}\n\nSensitive text and coordinates are not shown or logged.",
            target.window_title, target.process_id, action_name
        );
        let message = wide(&message);
        let heading = wide(heading);
        let result = unsafe {
            MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                PCWSTR(heading.as_ptr()),
                MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_SETFOREGROUND | MB_TOPMOST,
            )
        };
        Ok(result == IDYES)
    }
}

fn exact_scope(target: &UiControlTarget) -> Value {
    json!({
        "window_titles": [],
        "process_ids": [target.process_id],
        "process_names": [],
        "window_handles": [target.window_handle],
        "native_scope_trusted": true,
    })
}

fn target_from_status(status: &Value) -> Result<UiControlTarget, HostFailure> {
    let process_id = status
        .get("process_id")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            HostFailure::new(
                UiControlHostErrorCode::InvalidTarget,
                "the native session did not resolve a target process id",
            )
        })?;
    let window_handle = status
        .get("window_handle")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            HostFailure::new(
                UiControlHostErrorCode::InvalidTarget,
                "the native session did not resolve a target window handle",
            )
        })?;
    Ok(UiControlTarget {
        process_id,
        window_handle,
        window_title: status
            .get("window_title")
            .and_then(Value::as_str)
            .unwrap_or("DCC application")
            .to_owned(),
    })
}

fn native_action(action: &UiControlAction, observation_id: &str) -> ComputerUseAction {
    ComputerUseAction {
        action: match action.action.as_str() {
            "raw_coordinate_click" => "click".to_owned(),
            "keyboard_shortcut" => "keypress".to_owned(),
            value => value.to_owned(),
        },
        observation_id: Some(observation_id.to_owned()),
        x: action.x,
        y: action.y,
        button: action.button.clone(),
        scroll_x: action.scroll_x,
        scroll_y: action.scroll_y,
        path: action
            .path
            .iter()
            .map(|point| ComputerUsePoint {
                x: point.x,
                y: point.y,
            })
            .collect(),
        text: action.text.clone(),
        keys: action.keys.clone(),
        duration_ms: action.duration_ms,
    }
}

fn run_uia(payload: Value) -> Result<Value, HostFailure> {
    let script = UIA_SCRIPT.replace("# DCC_MCP_UIA_HELPERS", UIA_HELPERS);
    let script_path = std::env::temp_dir().join(format!(
        "dcc-mcp-ui-control-host-{}.ps1",
        Uuid::new_v4().simple()
    ));
    std::fs::write(&script_path, script).map_err(|error| {
        HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            format!("materialize the Windows UI Automation helper: {error}"),
        )
    })?;

    let result = run_uia_child(&script_path, &payload);
    let _ = std::fs::remove_file(script_path);
    result
}

fn run_uia_child(script_path: &std::path::Path, payload: &Value) -> Result<Value, HostFailure> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut child = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| {
            HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                format!("start the Windows UI Automation helper: {error}"),
            )
        })?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            "the Windows UI Automation helper has no stdin",
        )
    })?;
    stdin
        .write_all(payload.to_string().as_bytes())
        .map_err(|error| {
            HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                format!("send the Windows UI Automation request: {error}"),
            )
        })?;
    drop(stdin);

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdout_reader = thread::spawn(move || read_all(stdout));
    let stderr_reader = thread::spawn(move || read_all(stderr));
    let deadline = Instant::now() + UIA_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                format!("poll the Windows UI Automation helper: {error}"),
            )
        })? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                "Windows UI Automation timed out after 30 seconds",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        let message = String::from_utf8_lossy(if stderr.is_empty() { &stdout } else { &stderr });
        return Err(HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            format!("Windows UI Automation helper failed: {}", message.trim()),
        ));
    }
    serde_json::from_slice(&stdout).map_err(|error| {
        HostFailure::new(
            UiControlHostErrorCode::BackendUnavailable,
            format!("decode the Windows UI Automation response: {error}"),
        )
    })
}

fn read_all(mut reader: impl Read) -> Vec<u8> {
    let mut output = Vec::new();
    let _ = reader.read_to_end(&mut output);
    output
}

fn ensure_uia_ok(value: &Value, fallback: &str) -> Result<(), HostFailure> {
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    Err(failure_from_value(value, fallback))
}

fn failure_from_value(value: &Value, fallback: &str) -> HostFailure {
    let error = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("backend_unavailable");
    let code = match error {
        "stale_observation" => UiControlHostErrorCode::StaleObservation,
        "permission_denied" | "invalid_target" | "missing_window" => {
            UiControlHostErrorCode::InvalidTarget
        }
        "desktop_unavailable" => UiControlHostErrorCode::DesktopUnavailable,
        "capture_failed" => UiControlHostErrorCode::CaptureFailed,
        "user_interrupted" => UiControlHostErrorCode::UserInterrupted,
        _ => UiControlHostErrorCode::BackendUnavailable,
    };
    HostFailure::new(
        code,
        value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(fallback),
    )
}

fn map_computer_use_error(error: ComputerUseError) -> HostFailure {
    let code = match error.code {
        ComputerUseErrorCode::InvalidAction => UiControlHostErrorCode::InvalidRequest,
        ComputerUseErrorCode::BackendUnavailable
        | ComputerUseErrorCode::FocusLost
        | ComputerUseErrorCode::InputFailed => UiControlHostErrorCode::BackendUnavailable,
        ComputerUseErrorCode::MissingWindow
        | ComputerUseErrorCode::InvalidTarget
        | ComputerUseErrorCode::PermissionDenied => UiControlHostErrorCode::InvalidTarget,
        ComputerUseErrorCode::StaleObservation => UiControlHostErrorCode::StaleObservation,
        ComputerUseErrorCode::UserInterrupted => UiControlHostErrorCode::UserInterrupted,
        ComputerUseErrorCode::DesktopUnavailable => UiControlHostErrorCode::DesktopUnavailable,
        ComputerUseErrorCode::CaptureFailed => UiControlHostErrorCode::CaptureFailed,
    };
    HostFailure::new(code, error.message)
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WS_OVERLAPPEDWINDOW,
    };

    struct TestWindow(HWND);

    impl Drop for TestWindow {
        fn drop(&mut self) {
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    #[test]
    fn window_generation_marker_detects_capability_replacement() {
        let class = wide("STATIC");
        let title = wide("DCC MCP UI Control generation test");
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
                0,
                0,
                320,
                200,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let _window = TestWindow(hwnd);
        let target = UiControlTarget {
            process_id: unsafe { GetCurrentProcessId() },
            window_handle: hwnd.0 as usize as u64,
            window_title: "DCC generation test".to_owned(),
        };
        let guard = WindowGenerationGuard::bind(&target).unwrap();
        guard.verify().unwrap();

        let _ = unsafe { RemovePropW(hwnd, PCWSTR(guard.property_name.as_ptr())) };
        let failure = guard.verify().unwrap_err();
        assert_eq!(failure.code, UiControlHostErrorCode::InvalidTarget);
    }
}
