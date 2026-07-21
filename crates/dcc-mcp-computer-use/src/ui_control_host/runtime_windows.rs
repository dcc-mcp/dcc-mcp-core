use std::io::ErrorKind;
use std::io::{Read, Write};
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use dcc_mcp_capture::{CaptureTarget, WindowFinder};
use dcc_mcp_shm::SharedBuffer;
use dcc_mcp_ui_control::host_protocol::{
    UiControlAction, UiControlEnsureOutcome, UiControlHostErrorCode, UiControlInputKind,
    UiControlPolicyTier, UiControlSharedImage, UiControlSystemOperation, UiControlTarget,
    UiControlTaskGrant, UiControlWindowOperation, UiControlWindowState,
};
use serde_json::{Value, json};
use uuid::Uuid;
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ELEVATION_REQUIRED, ERROR_FILE_NOT_FOUND, ERROR_PRIVILEGE_NOT_HELD,
    ERROR_SUCCESS, HANDLE, HWND, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_CREATE_KEY_DISPOSITION,
    REG_CREATED_NEW_KEY, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
    RegCloseKey, RegCreateKeyExW, RegQueryValueExW, RegSetValueExW,
};
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
    ActionControlFence, ActionFenceExpectation, ConfirmationKind, ConfirmationSurface, HostFailure,
    HostRuntime, HostRuntimeSession, RuntimeAccessibilityState, RuntimeActionResult,
    RuntimeSnapshot, stale_accessibility_state, verify_expected_action_fence,
};

const UIA_TIMEOUT: Duration = Duration::from_secs(30);
const UIA_SCRIPT: &str = include_str!(
    "../../../../python/dcc_mcp_core/skills/ui-control/scripts/_windows_uia_backend.ps1"
);
const UIA_HELPERS: &str = include_str!(
    "../../../../python/dcc_mcp_core/skills/ui-control/scripts/_windows_uia_helpers.ps1"
);

pub(super) struct WindowsHostRuntime;

pub(super) fn execute_system_operation(
    operation: &UiControlSystemOperation,
) -> Result<UiControlEnsureOutcome, HostFailure> {
    match operation {
        UiControlSystemOperation::EnsureRegistryString {
            key,
            value_name,
            value,
        } => ensure_registry_value(key, value_name, REG_SZ, registry_string_bytes(value)),
        UiControlSystemOperation::EnsureRegistryDword {
            key,
            value_name,
            value,
        } => ensure_registry_value(key, value_name, REG_DWORD, value.to_le_bytes().to_vec()),
        UiControlSystemOperation::EnsureFileSymlink { link, target } => {
            ensure_symlink(link, target, false)
        }
        UiControlSystemOperation::EnsureDirectorySymlink { link, target } => {
            ensure_symlink(link, target, true)
        }
    }
}

struct OwnedRegistryKey(HKEY);

impl Drop for OwnedRegistryKey {
    fn drop(&mut self) {
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn ensure_registry_value(
    key: &str,
    value_name: &str,
    desired_type: REG_VALUE_TYPE,
    desired_data: Vec<u8>,
) -> Result<UiControlEnsureOutcome, HostFailure> {
    let key = wide(key);
    let value_name = wide(value_name);
    let mut handle = HKEY::default();
    let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            REG_SAM_FLAGS(KEY_QUERY_VALUE.0 | KEY_SET_VALUE.0),
            None,
            &mut handle,
            Some(&mut disposition),
        )
    };
    ensure_registry_status(status, "open the approved HKCU registry key")?;
    let handle = OwnedRegistryKey(handle);
    let existing = query_registry_value(handle.0, PCWSTR(value_name.as_ptr()))?;
    if existing
        .as_ref()
        .is_some_and(|(value_type, data)| *value_type == desired_type && data == &desired_data)
    {
        return Ok(UiControlEnsureOutcome::Unchanged);
    }
    let status = unsafe {
        RegSetValueExW(
            handle.0,
            PCWSTR(value_name.as_ptr()),
            None,
            desired_type,
            Some(&desired_data),
        )
    };
    ensure_registry_status(status, "write the approved HKCU registry value")?;
    Ok(
        if disposition == REG_CREATED_NEW_KEY || existing.is_none() {
            UiControlEnsureOutcome::Created
        } else {
            UiControlEnsureOutcome::Updated
        },
    )
}

fn query_registry_value(
    key: HKEY,
    value_name: PCWSTR,
) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>, HostFailure> {
    let mut value_type = REG_VALUE_TYPE::default();
    let mut length = 0_u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            value_name,
            None,
            Some(&mut value_type),
            None,
            Some(&mut length),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    ensure_registry_status(status, "read the approved HKCU registry value")?;
    let mut data = vec![0_u8; length as usize];
    let status = unsafe {
        RegQueryValueExW(
            key,
            value_name,
            None,
            Some(&mut value_type),
            Some(data.as_mut_ptr()),
            Some(&mut length),
        )
    };
    ensure_registry_status(status, "read the approved HKCU registry value")?;
    data.truncate(length as usize);
    Ok(Some((value_type, data)))
}

fn registry_string_bytes(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn ensure_registry_status(status: WIN32_ERROR, context: &str) -> Result<(), HostFailure> {
    if status == ERROR_SUCCESS {
        return Ok(());
    }
    Err(HostFailure::new(
        if matches!(
            status,
            ERROR_ACCESS_DENIED | ERROR_ELEVATION_REQUIRED | ERROR_PRIVILEGE_NOT_HELD
        ) {
            UiControlHostErrorCode::ElevationRequired
        } else {
            UiControlHostErrorCode::BackendUnavailable
        },
        if matches!(
            status,
            ERROR_ACCESS_DENIED | ERROR_ELEVATION_REQUIRED | ERROR_PRIVILEGE_NOT_HELD
        ) {
            "the approved HKCU registry target is not writable without elevated privileges"
                .to_owned()
        } else {
            format!("{context}: Windows error {}", status.0)
        },
    ))
}

fn ensure_symlink(
    link: &str,
    target: &str,
    directory: bool,
) -> Result<UiControlEnsureOutcome, HostFailure> {
    let link = Path::new(link);
    let target = Path::new(target);
    let target_metadata = std::fs::metadata(target)
        .map_err(|error| map_symlink_error(error, "inspect the approved symbolic-link target"))?;
    if target_metadata.is_dir() != directory || (!directory && !target_metadata.is_file()) {
        return Err(HostFailure::new(
            UiControlHostErrorCode::InvalidRequest,
            "the symbolic-link target kind does not match the typed operation",
        ));
    }
    let target = std::fs::canonicalize(target)
        .map_err(|error| map_symlink_error(error, "resolve the approved symbolic-link target"))?;
    let parent = link.parent().ok_or_else(|| {
        HostFailure::new(
            UiControlHostErrorCode::InvalidRequest,
            "the symbolic-link path has no parent directory",
        )
    })?;
    let parent = std::fs::canonicalize(parent)
        .map_err(|error| map_symlink_error(error, "resolve the symbolic-link parent"))?;
    let link = parent.join(link.file_name().ok_or_else(|| {
        HostFailure::new(
            UiControlHostErrorCode::InvalidRequest,
            "the symbolic-link path has no file name",
        )
    })?);
    if is_protected_link_path(&link) {
        return Err(HostFailure::new(
            UiControlHostErrorCode::ElevationRequired,
            "protected Windows locations are unsupported; select an operator-approved user-writable link path",
        ));
    }

    match std::fs::symlink_metadata(&link) {
        Ok(metadata) if !metadata.file_type().is_symlink() => {
            return Err(symlink_conflict());
        }
        Ok(_) => {
            let current = std::fs::read_link(&link)
                .map_err(|error| map_symlink_error(error, "inspect the existing symbolic link"))?;
            let current = if current.is_absolute() {
                current
            } else {
                parent.join(current)
            };
            let current = std::fs::canonicalize(current).map_err(|_| symlink_conflict())?;
            return if paths_equal(&current, &target) {
                Ok(UiControlEnsureOutcome::Unchanged)
            } else {
                Err(symlink_conflict())
            };
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(map_symlink_error(
                error,
                "inspect the approved symbolic-link path",
            ));
        }
    }

    let result = if directory {
        symlink_dir(&target, &link)
    } else {
        symlink_file(&target, &link)
    };
    result
        .map(|()| UiControlEnsureOutcome::Created)
        .map_err(|error| map_symlink_error(error, "create the approved symbolic link"))
}

fn is_protected_link_path(path: &Path) -> bool {
    [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ]
    .into_iter()
    .filter_map(std::env::var_os)
    .map(PathBuf::from)
    .any(|root| path_is_within(path, &root))
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = normalized_windows_path(path);
    let root = normalized_windows_path(root)
        .trim_end_matches('\\')
        .to_owned();
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|rest| rest.starts_with('\\'))
}

fn normalized_windows_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    value
        .strip_prefix(r"\\?\")
        .unwrap_or(&value)
        .to_ascii_lowercase()
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn symlink_conflict() -> HostFailure {
    HostFailure::new(
        UiControlHostErrorCode::Conflict,
        "the symbolic-link path already exists with different state; UI Control never overwrites it",
    )
}

fn map_symlink_error(error: std::io::Error, context: &str) -> HostFailure {
    let code = if error.kind() == ErrorKind::PermissionDenied
        || matches!(error.raw_os_error(), Some(740 | 1314))
    {
        UiControlHostErrorCode::ElevationRequired
    } else if error.kind() == ErrorKind::AlreadyExists {
        UiControlHostErrorCode::Conflict
    } else {
        UiControlHostErrorCode::BackendUnavailable
    };
    HostFailure::new(
        code,
        match code {
            UiControlHostErrorCode::ElevationRequired => {
                "the symbolic-link location is not writable without elevated privileges".to_owned()
            }
            UiControlHostErrorCode::Conflict => symlink_conflict().message,
            _ => format!("{context}: {error}"),
        },
    )
}

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

    fn start_visible_notice(&mut self) -> Result<(), HostFailure> {
        self.window_generation.verify()?;
        if self.started {
            return Ok(());
        }
        let status = self.session.start().map_err(map_computer_use_error)?;
        self.target = target_from_status(&status)?;
        self.window_generation.verify()?;
        self.started = true;
        Ok(())
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
        self.start_visible_notice()?;
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
        let accessibility = query_accessibility_state(&self.target, max_depth, max_nodes)?;

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
            root: accessibility.root,
            focus_runtime_id: accessibility.focus_runtime_id,
            node_count: accessibility.node_count,
            image,
        })
    }

    fn accessibility_state(
        &mut self,
        max_depth: u32,
        max_nodes: u32,
    ) -> Result<RuntimeAccessibilityState, HostFailure> {
        self.window_generation.verify()?;
        query_accessibility_state(&self.target, max_depth, max_nodes)
    }

    fn execute(
        &mut self,
        observation_id: &str,
        action: &UiControlAction,
        fence: &ActionFenceExpectation,
    ) -> Result<RuntimeActionResult, HostFailure> {
        self.window_generation.verify()?;
        self.image_buffer = None;
        let result = match action.input_kind {
            UiControlInputKind::RawInput => {
                let request = native_action(action, observation_id);
                let mut pre_input_fence = || {
                    let live =
                        query_accessibility_state(&self.target, fence.max_depth, fence.max_nodes)
                            .map_err(map_host_failure_to_computer_use_error)?;
                    verify_expected_action_fence(action, fence, &live)
                        .map_err(map_host_failure_to_computer_use_error)?;
                    Ok(())
                };
                self.session
                    .perform_with_pre_input_fence(&request, &mut pre_input_fence)
                    .map_err(map_computer_use_error)?;
                Ok(RuntimeActionResult {
                    message: format!("completed scoped native action {:?}", action.action),
                    before_focus_runtime_id: None,
                    after_focus_runtime_id: None,
                })
            }
            UiControlInputKind::Semantic => {
                let [expected_control] = fence.controls.as_slice() else {
                    return Err(stale_accessibility_state());
                };
                self.session
                    .prepare_semantic_action(observation_id)
                    .map_err(map_computer_use_error)?;
                let raw = run_uia(json!({
                    "mode": "act",
                    "scope": exact_scope(&self.target),
                    "max_depth": fence.max_depth,
                    "max_nodes": fence.max_nodes,
                    "expected_fence": control_fence_json(expected_control),
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
        let message = format!("{detail}\n\n{scope}\nAction: {action_name}\n\n{privacy}");
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

fn exact_scope(target: &UiControlTarget) -> Value {
    json!({
        "window_titles": [],
        "process_ids": [target.process_id],
        "process_names": [],
        "window_handles": [target.window_handle],
        "native_scope_trusted": true,
    })
}

fn query_accessibility_state(
    target: &UiControlTarget,
    max_depth: u32,
    max_nodes: u32,
) -> Result<RuntimeAccessibilityState, HostFailure> {
    let raw = run_uia(json!({
        "mode": "snapshot",
        "scope": exact_scope(target),
        "max_depth": max_depth,
        "max_nodes": max_nodes,
    }))?;
    ensure_uia_ok(&raw, "Windows UI Automation snapshot failed")?;
    Ok(RuntimeAccessibilityState {
        root: raw.get("root").cloned().ok_or_else(|| {
            HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                "Windows UI Automation returned no scoped root",
            )
        })?,
        focus_runtime_id: optional_string(&raw, "focus_runtime_id"),
        node_count: raw
            .get("node_count")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(1),
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

fn map_host_failure_to_computer_use_error(failure: HostFailure) -> ComputerUseError {
    let code = match failure.code {
        UiControlHostErrorCode::StaleObservation => ComputerUseErrorCode::StaleObservation,
        UiControlHostErrorCode::InvalidTarget => ComputerUseErrorCode::InvalidTarget,
        UiControlHostErrorCode::DesktopUnavailable => ComputerUseErrorCode::DesktopUnavailable,
        UiControlHostErrorCode::CaptureFailed => ComputerUseErrorCode::CaptureFailed,
        UiControlHostErrorCode::UserInterrupted => ComputerUseErrorCode::UserInterrupted,
        UiControlHostErrorCode::InvalidRequest => ComputerUseErrorCode::InvalidAction,
        _ => ComputerUseErrorCode::BackendUnavailable,
    };
    ComputerUseError::new(code, failure.message)
}

fn control_fence_json(fence: &ActionControlFence) -> Value {
    json!({
        "identity": fence.identity,
        "is_password": fence.is_password,
        "name": fence.name,
        "automation_id": fence.automation_id,
        "class_name": fence.class_name,
        "policy_tier": fence.policy_tier,
    })
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
    use windows::Win32::System::Registry::RegDeleteTreeW;
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

    struct TestRegistryKey(String);

    impl Drop for TestRegistryKey {
        fn drop(&mut self) {
            let key = wide(&self.0);
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
        }
    }

    struct TestSymlinkTree(PathBuf);

    impl Drop for TestSymlinkTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("file-link"));
            let _ = std::fs::remove_dir(self.0.join("dir-link"));
            let _ = std::fs::remove_dir_all(&self.0);
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

    #[test]
    fn semantic_uia_script_rechecks_the_confirmed_fence_immediately_before_mutation() {
        let fence_check = UIA_SCRIPT
            .find("Matches-Expected-Fence $target $payload.expected_fence")
            .unwrap();
        let ancestry_check = UIA_SCRIPT[fence_check..]
            .find("Denied-Action-Target-Reason $root $target")
            .map(|offset| fence_check + offset)
            .unwrap();
        let invoke = UIA_SCRIPT
            .find("$actionResult = Invoke-Action $target")
            .unwrap();

        assert!(fence_check < invoke);
        assert!(fence_check < ancestry_check);
        assert!(ancestry_check < invoke);
        assert!(UIA_SCRIPT[fence_check..invoke].contains("stale_observation"));
        assert!(UIA_SCRIPT.contains("Has-Authentication-Secret-Marker $current"));
    }

    #[test]
    fn typed_system_helpers_preserve_registry_and_symlink_contracts() {
        assert_eq!(registry_string_bytes("ok"), b"o\0k\0\0\0");
        assert!(path_is_within(
            Path::new(r"\\?\C:\Program Files\Vendor\plugin"),
            Path::new(r"C:\Program Files")
        ));

        let root = std::env::temp_dir().join(format!(
            "dcc-mcp-system-operation-test-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let target = root.join("target.txt");
        let link = root.join("existing.txt");
        std::fs::write(&target, b"target").unwrap();
        std::fs::write(&link, b"keep").unwrap();
        let failure =
            ensure_symlink(&link.to_string_lossy(), &target.to_string_lossy(), false).unwrap_err();
        assert_eq!(failure.code, UiControlHostErrorCode::Conflict);
        assert_eq!(std::fs::read(&link).unwrap(), b"keep");
        std::fs::remove_file(link).unwrap();
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn registry_ensure_reports_created_updated_and_unchanged() {
        let key = format!(r"Software\DccMcp\Tests\{}", Uuid::new_v4().simple());
        let _cleanup = TestRegistryKey(key.clone());

        assert_eq!(
            ensure_registry_value(&key, "Enabled", REG_SZ, registry_string_bytes("one")).unwrap(),
            UiControlEnsureOutcome::Created
        );
        assert_eq!(
            ensure_registry_value(&key, "Enabled", REG_SZ, registry_string_bytes("one")).unwrap(),
            UiControlEnsureOutcome::Unchanged
        );
        assert_eq!(
            ensure_registry_value(&key, "Enabled", REG_SZ, registry_string_bytes("two")).unwrap(),
            UiControlEnsureOutcome::Updated
        );
        assert_eq!(
            ensure_registry_value(&key, "Enabled", REG_SZ, registry_string_bytes("two")).unwrap(),
            UiControlEnsureOutcome::Unchanged
        );
    }

    #[test]
    fn file_and_directory_symlinks_report_created_and_unchanged_when_permitted() {
        let root = std::env::temp_dir().join(format!(
            "dcc-mcp-symlink-operation-test-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let _cleanup = TestSymlinkTree(root.clone());
        let file_target = root.join("target.txt");
        let directory_target = root.join("target-dir");
        std::fs::write(&file_target, b"target").unwrap();
        std::fs::create_dir(&directory_target).unwrap();

        let file_link = root.join("file-link");
        let created = ensure_symlink(
            &file_link.to_string_lossy(),
            &file_target.to_string_lossy(),
            false,
        );
        if matches!(
            created,
            Err(HostFailure {
                code: UiControlHostErrorCode::ElevationRequired,
                ..
            })
        ) {
            eprintln!("symlink integration skipped: Windows symlink privilege is unavailable");
            return;
        }
        assert_eq!(created.unwrap(), UiControlEnsureOutcome::Created);
        assert_eq!(
            ensure_symlink(
                &file_link.to_string_lossy(),
                &file_target.to_string_lossy(),
                false,
            )
            .unwrap(),
            UiControlEnsureOutcome::Unchanged
        );

        let directory_link = root.join("dir-link");
        assert_eq!(
            ensure_symlink(
                &directory_link.to_string_lossy(),
                &directory_target.to_string_lossy(),
                true,
            )
            .unwrap(),
            UiControlEnsureOutcome::Created
        );
        assert_eq!(
            ensure_symlink(
                &directory_link.to_string_lossy(),
                &directory_target.to_string_lossy(),
                true,
            )
            .unwrap(),
            UiControlEnsureOutcome::Unchanged
        );
    }
}
