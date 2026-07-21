use super::*;
use std::sync::mpsc;
use std::thread::JoinHandle;

use dcc_mcp_ui_control::host_protocol::UiControlPolicyTier;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::RegDeleteTreeW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    HMENU, MSG, PostMessageW, PostQuitMessage, RegisterClassW, TranslateMessage, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW,
    WS_VISIBLE,
};
use windows::core::w;

struct TestWindow(HWND);

impl Drop for TestWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

const SEMANTIC_CLOSE_BUTTON_ID: usize = 1_001;

unsafe extern "system" fn semantic_close_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_COMMAND && (wparam.0 & 0xffff) == SEMANTIC_CLOSE_BUTTON_ID {
        let _ = unsafe { DestroyWindow(hwnd) };
        return LRESULT(0);
    }
    if message == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

struct SemanticCloseWindow {
    hwnd: HWND,
    thread: Option<JoinHandle<()>>,
}

impl SemanticCloseWindow {
    fn spawn() -> Self {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("dcc-mcp-semantic-close-test-window".to_owned())
            .spawn(move || {
                let instance = unsafe { GetModuleHandleW(None) }.unwrap();
                let class_name = wide(&format!(
                    "DccMcpSemanticCloseTestWindow{}",
                    Uuid::new_v4().simple()
                ));
                let window_class = WNDCLASSW {
                    lpfnWndProc: Some(semantic_close_window_proc),
                    hInstance: instance.into(),
                    lpszClassName: PCWSTR(class_name.as_ptr()),
                    ..Default::default()
                };
                assert_ne!(unsafe { RegisterClassW(&window_class) }, 0);
                let title = wide("DCC MCP semantic Invoke close test");
                let hwnd = unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        PCWSTR(class_name.as_ptr()),
                        PCWSTR(title.as_ptr()),
                        WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0),
                        240,
                        240,
                        520,
                        260,
                        None,
                        None,
                        Some(instance.into()),
                        None,
                    )
                }
                .unwrap();
                let button_text = wide("Close test window");
                unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        w!("BUTTON"),
                        PCWSTR(button_text.as_ptr()),
                        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | BS_PUSHBUTTON as u32),
                        40,
                        60,
                        220,
                        60,
                        Some(hwnd),
                        Some(HMENU(SEMANTIC_CLOSE_BUTTON_ID as *mut core::ffi::c_void)),
                        Some(instance.into()),
                        None,
                    )
                }
                .unwrap();
                ready_tx.send(hwnd.0 as usize as u64).unwrap();
                let mut message = MSG::default();
                while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            })
            .unwrap();
        let handle = ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        Self {
            hwnd: HWND(handle as usize as *mut core::ffi::c_void),
            thread: Some(thread),
        }
    }

    fn join(mut self) {
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for SemanticCloseWindow {
    fn drop(&mut self) {
        if unsafe { IsWindow(Some(self.hwnd)) }.as_bool() {
            let _ = unsafe { PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn find_control_by_name<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
    if node.get("name").and_then(Value::as_str) == Some(name) {
        return Some(node);
    }
    node.get("children")
        .and_then(Value::as_array)
        .and_then(|children| {
            children
                .iter()
                .find_map(|child| find_control_by_name(child, name))
        })
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
    let failure = guard.target_closed_after_completed_action().unwrap_err();
    assert_eq!(failure.code, UiControlHostErrorCode::InvalidTarget);
}

#[test]
fn completed_action_can_report_a_closed_exact_target_without_following_replacement() {
    let class = wide("STATIC");
    let title = wide("DCC MCP UI Control close transition test");
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
        window_title: "DCC close transition test".to_owned(),
    };
    let guard = WindowGenerationGuard::bind(&target).unwrap();

    unsafe { DestroyWindow(hwnd) }.unwrap();

    assert!(guard.target_closed_after_completed_action().unwrap());
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
    let failed_mutation = UIA_SCRIPT[invoke..]
        .find("if (-not [bool]$actionResult.ok)")
        .map(|offset| invoke + offset)
        .unwrap();
    let optional_post_state = UIA_SCRIPT[failed_mutation..]
        .find("$afterFocus = $null")
        .map(|offset| failed_mutation + offset)
        .unwrap();

    assert!(fence_check < invoke);
    assert!(fence_check < ancestry_check);
    assert!(ancestry_check < invoke);
    assert!(invoke < failed_mutation);
    assert!(failed_mutation < optional_post_state);
    assert!(UIA_SCRIPT[optional_post_state..].contains("$control = Element-Raw $target"));
    assert!(UIA_SCRIPT[optional_post_state..].contains("catch {}"));
    assert!(UIA_SCRIPT[fence_check..invoke].contains("stale_observation"));
    assert!(UIA_SCRIPT.contains("Has-Authentication-Secret-Marker $current"));
}

#[test]
fn semantic_invoke_that_destroys_the_target_reports_mutation_success() {
    let window = SemanticCloseWindow::spawn();
    let target = UiControlTarget {
        process_id: unsafe { GetCurrentProcessId() },
        window_handle: window.hwnd.0 as usize as u64,
        window_title: "DCC MCP semantic Invoke close test".to_owned(),
    };
    let accessibility = query_accessibility_state(&target, 5, 100).unwrap();
    let button = find_control_by_name(&accessibility.root, "Close test window")
        .expect("standard button must be present in the scoped UIA tree");
    let runtime_id = button
        .get("runtime_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let fallback_path = button.get("fallback_path").and_then(Value::as_str).unwrap();
    let identity = runtime_id.unwrap_or(fallback_path).to_owned();
    let control_id = runtime_id.map_or_else(
        || format!("uia:path:{fallback_path}"),
        |value| format!("uia:{value}"),
    );
    let text = |key| {
        button
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let expected = ActionControlFence {
        identity,
        is_password: button
            .get("is_password")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        name: text("name"),
        automation_id: text("automation_id"),
        class_name: text("class_name"),
        policy_tier: UiControlPolicyTier::TaskGrant,
    };

    let raw = run_uia(json!({
        "mode": "act",
        "scope": exact_scope(&target),
        "max_depth": 5,
        "max_nodes": 100,
        "expected_fence": control_fence_json(&expected),
        "action": {
            "control_id": control_id,
            "action": "click",
            "text": "",
            "checked": false,
        },
    }))
    .unwrap();

    assert_eq!(raw.get("ok").and_then(Value::as_bool), Some(true), "{raw}");
    assert_eq!(
        raw.get("message").and_then(Value::as_str),
        Some("invoked native button"),
        "{raw}"
    );
    assert!(!unsafe { IsWindow(Some(window.hwnd)) }.as_bool());
    window.join();
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
