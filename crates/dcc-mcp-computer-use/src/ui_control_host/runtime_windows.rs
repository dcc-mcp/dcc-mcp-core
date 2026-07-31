use std::cell::RefCell;
use std::io::ErrorKind;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use dcc_mcp_capture::{
    CaptureError, CaptureTarget, WindowFinder, WindowRecordingConfig, record_window_jpeg_sequence,
};
use dcc_mcp_shm::SharedBuffer;
use dcc_mcp_ui_control::host_protocol::{
    UiControlAction, UiControlClipArtifact, UiControlClipFormat, UiControlEnsureOutcome,
    UiControlHostErrorCode, UiControlInputKind, UiControlSharedImage, UiControlSystemOperation,
    UiControlTarget, UiControlTaskGrant, UiControlWindowOperation, UiControlWindowState,
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
    GetPropW, GetWindowThreadProcessId, IsWindow, RemovePropW, SetPropW,
};
use windows::core::PCWSTR;

use crate::{
    ComputerUseAction, ComputerUseError, ComputerUseErrorCode, ComputerUsePoint,
    ComputerUseRecordingFence, ComputerUseSession, ComputerUseTargetScope,
};

use super::{
    ActionControlFence, ActionFenceExpectation, HostFailure, HostRuntime, HostRuntimeSession,
    RuntimeAccessibilityState, RuntimeActionResult, RuntimeClipRequest, RuntimeSnapshot,
    allows_owned_standard_menu_popup,
    recording_artifact::{RecordingArtifactError, RecordingArtifactWriter},
    stale_accessibility_state, verify_expected_action_fence,
};

mod confirmation;

pub(super) use confirmation::WindowsConfirmationSurface;

const UIA_TIMEOUT: Duration = Duration::from_secs(30);
const UIA_STDERR_LIMIT: u64 = 64 * 1024;
const UIA_SCRIPT: &str = include_str!(
    "../../../../python/dcc_mcp_core/skills/ui-control/scripts/_windows_uia_backend.ps1"
);
const UIA_HELPERS: &str = include_str!(
    "../../../../python/dcc_mcp_core/skills/ui-control/scripts/_windows_uia_helpers.ps1"
);

struct UiaWorker {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    responses: Receiver<Vec<u8>>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<Vec<u8>>>,
    stderr: Vec<u8>,
    script_path: PathBuf,
}

impl UiaWorker {
    fn start() -> Result<Self, HostFailure> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
        let child = Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_file(&script_path);
                return Err(HostFailure::new(
                    UiControlHostErrorCode::BackendUnavailable,
                    format!("start the Windows UI Automation helper: {error}"),
                ));
            }
        };
        let stdin = child.stdin.take().expect("piped UIA worker stdin");
        let stdout = child.stdout.take().expect("piped UIA worker stdout");
        let stderr = child.stderr.take().expect("piped UIA worker stderr");
        let (response_tx, responses) = mpsc::channel();
        let stdout_reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut response = Vec::new();
                match reader.read_until(b'\n', &mut response) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        while response
                            .last()
                            .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
                        {
                            response.pop();
                        }
                        if response_tx.send(response).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let stderr_reader = thread::spawn(move || read_bounded(stderr, UIA_STDERR_LIMIT));
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            responses,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            stderr: Vec::new(),
            script_path,
        })
    }

    fn request(&mut self, payload: &Value) -> Result<Value, HostFailure> {
        if self.child.is_none() {
            return Err(HostFailure::new(
                UiControlHostErrorCode::BackendUnavailable,
                "the Windows UI Automation worker is unavailable; reopen the UI Control session",
            ));
        }
        let exited = match self
            .child
            .as_mut()
            .expect("active UIA worker child")
            .try_wait()
        {
            Ok(exited) => exited,
            Err(error) => {
                return Err(self.fail(format!("poll the Windows UI Automation worker: {error}")));
            }
        };
        if let Some(status) = exited {
            return Err(self.fail(format!(
                "Windows UI Automation worker exited before the request with status {status}"
            )));
        }
        let stdin = self.stdin.as_mut().expect("active UIA worker stdin");
        if let Err(error) = writeln!(stdin, "{payload}").and_then(|()| stdin.flush()) {
            return Err(self.fail(format!(
                "send the Windows UI Automation worker request: {error}"
            )));
        }
        let response = match self.responses.recv_timeout(UIA_TIMEOUT) {
            Ok(response) => response,
            Err(RecvTimeoutError::Timeout) => {
                return Err(self.fail("Windows UI Automation timed out after 30 seconds"));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(self.fail("Windows UI Automation worker closed without a response"));
            }
        };
        match serde_json::from_slice(&response) {
            Ok(value) => Ok(value),
            Err(error) => Err(self.fail(format!(
                "decode the Windows UI Automation response: {error}"
            ))),
        }
    }

    fn fail(&mut self, message: impl Into<String>) -> HostFailure {
        self.shutdown();
        let mut message = message.into();
        let stderr = String::from_utf8_lossy(&self.stderr);
        if !stderr.trim().is_empty() {
            message.push_str(": ");
            message.push_str(stderr.trim());
        }
        HostFailure::new(UiControlHostErrorCode::BackendUnavailable, message)
    }

    fn shutdown(&mut self) {
        self.stdin = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            self.stderr = reader.join().unwrap_or_default();
        }
        let _ = std::fs::remove_file(&self.script_path);
    }

    #[cfg(test)]
    fn process_id(&self) -> u32 {
        self.child.as_ref().map(Child::id).unwrap_or_default()
    }
}

impl Drop for UiaWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

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
    fn open(
        &self,
        grant: &UiControlTaskGrant,
        session_id: &str,
    ) -> Result<Box<dyn HostRuntimeSession>, HostFailure> {
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
            if session_id.is_empty() {
                None
            } else {
                Some(session_id.to_string())
            },
        )
        .map_err(map_computer_use_error)?;
        let window_generation = WindowGenerationGuard::bind(&target)?;
        Ok(Box::new(WindowsRuntimeSession {
            session,
            target,
            window_generation,
            started: false,
            image_buffer: None,
            uia_worker: None,
        }))
    }
}

struct WindowsRuntimeSession {
    session: ComputerUseSession,
    target: UiControlTarget,
    window_generation: WindowGenerationGuard,
    started: bool,
    image_buffer: Option<SharedBuffer>,
    uia_worker: Option<UiaWorker>,
}

impl WindowsRuntimeSession {
    fn uia_worker(&mut self) -> Result<&mut UiaWorker, HostFailure> {
        if self.uia_worker.is_none() {
            self.uia_worker = Some(UiaWorker::start()?);
        }
        Ok(self
            .uia_worker
            .as_mut()
            .expect("UIA worker was initialized"))
    }

    fn query_accessibility_state(
        &mut self,
        max_depth: u32,
        max_nodes: u32,
        allow_owned_standard_menu_popup: bool,
    ) -> Result<RuntimeAccessibilityState, HostFailure> {
        let target = self.target.clone();
        query_accessibility_state(
            self.uia_worker()?,
            &target,
            max_depth,
            max_nodes,
            allow_owned_standard_menu_popup,
        )
    }

    fn run_uia(&mut self, payload: Value) -> Result<Value, HostFailure> {
        self.uia_worker()?.request(&payload)
    }
}

impl HostRuntimeSession for WindowsRuntimeSession {
    fn target(&self) -> &UiControlTarget {
        &self.target
    }

    fn user_interrupted(&self) -> bool {
        self.session
            .status()
            .get("user_interrupted")
            .and_then(Value::as_bool)
            .unwrap_or(true)
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
        let accessibility = self.query_accessibility_state(max_depth, max_nodes, true)?;

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

    fn record_clip(
        &mut self,
        request: RuntimeClipRequest,
    ) -> Result<UiControlClipArtifact, HostFailure> {
        self.start_visible_notice()?;
        self.window_generation.verify()?;
        match request.format {
            UiControlClipFormat::JpegSequence => {}
        }
        let recording_fence = self
            .session
            .recording_fence()
            .map_err(map_computer_use_error)?;
        let config = WindowRecordingConfig::new(
            self.target.window_handle,
            request.duration_ms,
            request.frames_per_second,
            request.jpeg_quality,
        )
        .map_err(map_recording_capture_error)?;
        let recording_id = format!("clip-{}", Uuid::new_v4().simple());
        let recording_root =
            PathBuf::from(dcc_mcp_paths::get_platform_dir("cache").map_err(|error| {
                HostFailure::new(
                    UiControlHostErrorCode::CaptureFailed,
                    format!("resolve the host-owned recording directory: {error}"),
                )
            })?)
            .join("ui-control")
            .join("recordings");
        let mut writer = RecordingArtifactWriter::create_in(
            &recording_root,
            &recording_id,
            self.target.clone(),
            request.frames_per_second,
            request.jpeg_quality,
        )
        .map_err(map_recording_artifact_error)?;

        let generation = &self.window_generation;
        let fence_failure = RefCell::new(None);
        let summary = record_window_jpeg_sequence(
            &config,
            |frame| {
                if let Some(failure) = recording_fence_failure(&recording_fence, generation) {
                    *fence_failure.borrow_mut() = Some(failure);
                    return Err(CaptureError::Cancelled);
                }
                writer
                    .write_frame(
                        frame.index,
                        frame.timestamp_ms,
                        frame.width,
                        frame.height,
                        &frame.data,
                    )
                    .map_err(|error| CaptureError::Internal(error.to_string()))
            },
            || {
                if let Some(failure) = recording_fence_failure(&recording_fence, generation) {
                    *fence_failure.borrow_mut() = Some(failure);
                    true
                } else {
                    false
                }
            },
        );
        if let Some(failure) = fence_failure.into_inner() {
            return Err(failure);
        }
        let summary = summary.map_err(map_recording_capture_error)?;
        self.window_generation.verify()?;
        if summary.frame_count != config.schedule().frame_count() {
            return Err(HostFailure::new(
                UiControlHostErrorCode::CaptureFailed,
                format!(
                    "recording produced {} frames; expected {}",
                    summary.frame_count,
                    config.schedule().frame_count()
                ),
            ));
        }
        writer
            .finish(summary.ended_at_ms)
            .map_err(map_recording_artifact_error)
    }

    fn accessibility_state(
        &mut self,
        max_depth: u32,
        max_nodes: u32,
        allow_owned_standard_menu_popup: bool,
    ) -> Result<RuntimeAccessibilityState, HostFailure> {
        self.window_generation.verify()?;
        self.query_accessibility_state(max_depth, max_nodes, allow_owned_standard_menu_popup)
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
                if self.uia_worker.is_none() {
                    self.uia_worker = Some(UiaWorker::start()?);
                }
                let target = self.target.clone();
                let worker = self
                    .uia_worker
                    .as_mut()
                    .expect("UIA worker was initialized");
                let mut pre_input_fence = || {
                    let live = query_accessibility_state(
                        worker,
                        &target,
                        fence.max_depth,
                        fence.max_nodes,
                        allows_owned_standard_menu_popup(action),
                    )
                    .map_err(map_host_failure_to_computer_use_error)?;
                    verify_expected_action_fence(action, fence, &live)
                        .map_err(map_host_failure_to_computer_use_error)?;
                    Ok(())
                };
                let session = &mut self.session;
                session
                    .perform_with_pre_input_fence(&request, &mut pre_input_fence)
                    .map_err(map_computer_use_error)?;
                Ok(RuntimeActionResult {
                    message: format!("completed scoped native action {:?}", action.action),
                    target_closed: false,
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
                let raw = self.run_uia(json!({
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
                    target_closed: false,
                    before_focus_runtime_id: optional_string(&raw, "before_focus_runtime_id"),
                    after_focus_runtime_id: optional_string(&raw, "after_focus_runtime_id"),
                })
            }
        };
        let mut result = result?;
        if self
            .window_generation
            .target_closed_after_completed_action()?
        {
            result.target_closed = true;
            result.message.push_str(
                "; the exact target window closed, so this UI Control session was revoked",
            );
        }
        Ok(result)
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
        if let Some(mut worker) = self.uia_worker.take() {
            worker.shutdown();
        }
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

    fn target_closed_after_completed_action(&self) -> Result<bool, HostFailure> {
        let hwnd = HWND(self.window_handle as usize as *mut core::ffi::c_void);
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Ok(true);
        }
        let mut process_id = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
        let marker = unsafe { GetPropW(hwnd, PCWSTR(self.property_name.as_ptr())) };
        if process_id != self.process_id || marker.0 as usize != self.marker {
            if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                return Ok(true);
            }
            return Err(HostFailure::new(
                UiControlHostErrorCode::InvalidTarget,
                "the exact target HWND generation changed",
            ));
        }
        Ok(false)
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

fn exact_scope(target: &UiControlTarget) -> Value {
    json!({
        "window_titles": [],
        "process_ids": [target.process_id],
        "process_names": [],
        "window_handles": [target.window_handle],
        "native_scope_trusted": true,
    })
}

fn accessibility_scope(target: &UiControlTarget, allow_owned_standard_menu_popup: bool) -> Value {
    let mut scope = exact_scope(target);
    scope["allow_owned_standard_menu_popup"] = Value::Bool(allow_owned_standard_menu_popup);
    scope
}

fn query_accessibility_state(
    worker: &mut UiaWorker,
    target: &UiControlTarget,
    max_depth: u32,
    max_nodes: u32,
    allow_owned_standard_menu_popup: bool,
) -> Result<RuntimeAccessibilityState, HostFailure> {
    let raw = worker.request(&json!({
        "mode": "snapshot",
        "scope": accessibility_scope(target, allow_owned_standard_menu_popup),
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

fn read_bounded(reader: impl Read, limit: u64) -> Vec<u8> {
    let mut reader = reader.take(limit);
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

fn map_recording_capture_error(error: CaptureError) -> HostFailure {
    let code = match error {
        CaptureError::Cancelled => UiControlHostErrorCode::UserInterrupted,
        CaptureError::TargetNotFound(_) => UiControlHostErrorCode::InvalidTarget,
        CaptureError::BackendNotSupported(_) => UiControlHostErrorCode::BackendUnavailable,
        CaptureError::DesktopUnavailable { .. } => UiControlHostErrorCode::DesktopUnavailable,
        _ => UiControlHostErrorCode::CaptureFailed,
    };
    HostFailure::new(code, error.to_string())
}

fn map_recording_artifact_error(error: RecordingArtifactError) -> HostFailure {
    HostFailure::new(UiControlHostErrorCode::CaptureFailed, error.to_string())
}

fn recording_fence_failure(
    fence: &ComputerUseRecordingFence,
    generation: &WindowGenerationGuard,
) -> Option<HostFailure> {
    if let Err(error) = fence.check() {
        return Some(map_computer_use_error(error));
    }
    generation.verify().err()
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
mod tests;
