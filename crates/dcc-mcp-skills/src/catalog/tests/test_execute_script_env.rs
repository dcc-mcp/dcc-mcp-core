//! Regression tests for issue #231 (silent ambient-python fallback) and
//! GUI-executable guard.
//!
//! These tests manipulate process env vars. Each test wraps its env-var
//! mutations in a shared [`EnvVarsGuard`] so that dropped values are
//! restored atomically, even on panic.
use super::*;
use dcc_mcp_test_utils::EnvVarsGuard;

/// Clear the three Python-execution env vars and return a guard that restores
/// them on drop. Individual tests may then `unsafe { set_var }` one of them
/// under the guard's serialised scope.
fn clear_exec_env() -> EnvVarsGuard {
    EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
    ])
}

// ── Ambient-python checks (issue #231) ──────────────────────────────────────

#[test]
fn test_execute_script_rejects_ambient_python_for_host_dcc() {
    let _g = clear_exec_env();

    let result = execute_script(
        "any_skill.py",
        serde_json::json!({"key": "value"}),
        Some("maya"),
    );
    let err = result.expect_err("must fail loudly when DCC_MCP_PYTHON_EXECUTABLE is unset");
    assert!(
        err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
        "error message must mention the env var: got {err}"
    );
    assert!(
        err.to_lowercase().contains("maya"),
        "error message must mention the offending DCC: got {err}"
    );
}

#[test]
fn test_execute_script_rejects_ambient_python_case_insensitive() {
    let _g = clear_exec_env();
    let err = execute_script("any.py", serde_json::json!({}), Some("Houdini"))
        .expect_err("Houdini must also require a host python");
    assert!(err.contains("DCC_MCP_PYTHON_EXECUTABLE"));
}

#[test]
fn test_execute_script_allows_opt_out_via_env_var() {
    let _g = clear_exec_env();
    unsafe { std::env::set_var("DCC_MCP_ALLOW_AMBIENT_PYTHON", "1") };

    let result = execute_script("/does/not/exist.py", serde_json::json!({}), Some("maya"));
    if let Err(err) = result {
        assert!(
            !err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
            "opt-out must suppress the #231 check: got {err}"
        );
    }
}

#[test]
fn test_execute_script_skips_check_when_executable_set() {
    let _g = clear_exec_env();
    unsafe { std::env::set_var("DCC_MCP_PYTHON_EXECUTABLE", "python") };

    let result = execute_script("/does/not/exist.py", serde_json::json!({}), Some("maya"));
    if let Err(err) = result {
        assert!(
            !err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
            "explicit executable must disable the loud-fail: got {err}"
        );
    }
}

#[test]
fn test_execute_script_allows_generic_python_dcc() {
    let _g = clear_exec_env();
    let result = execute_script("/does/not/exist.py", serde_json::json!({}), Some("python"));
    if let Err(err) = result {
        assert!(
            !err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
            "generic 'python' dcc must not trigger the #231 check: got {err}"
        );
    }
}

#[test]
fn test_execute_script_no_dcc_hint_does_not_trigger_check() {
    let _g = clear_exec_env();
    let result = execute_script("/does/not/exist.py", serde_json::json!({}), None);
    if let Err(err) = result {
        assert!(
            !err.contains("DCC_MCP_PYTHON_EXECUTABLE"),
            "no dcc hint must not trigger the #231 check: got {err}"
        );
    }
}

// ── GUI-executable guard ─────────────────────────────────────────────────────

#[test]
fn test_execute_script_rejects_gui_executable_maya_exe() {
    let _g = clear_exec_env();
    unsafe { std::env::set_var("DCC_MCP_PYTHON_EXECUTABLE", "maya.exe") };

    let result = execute_script("any_skill.py", serde_json::json!({}), None);
    let err =
        result.expect_err("must fail when DCC_MCP_PYTHON_EXECUTABLE points to a GUI executable");
    assert!(
        err.contains("GUI executable"),
        "error must mention GUI executable: got {err}"
    );
    assert!(
        err.contains("maya.exe"),
        "error must mention the offending executable: got {err}"
    );
}

#[test]
fn test_execute_script_rejects_gui_executable_case_insensitive() {
    let _g = clear_exec_env();
    unsafe {
        std::env::set_var(
            "DCC_MCP_PYTHON_EXECUTABLE",
            "/usr/autodesk/maya2024/bin/Maya",
        )
    };

    let err = execute_script("any_skill.py", serde_json::json!({}), None)
        .expect_err("must fail for GUI executable regardless of case");
    assert!(
        err.contains("GUI executable"),
        "error must mention GUI executable: got {err}"
    );
}

#[test]
fn test_execute_script_rejects_gui_executable_blender() {
    let _g = clear_exec_env();
    unsafe { std::env::set_var("DCC_MCP_PYTHON_EXECUTABLE", "blender") };

    let err = execute_script("any_skill.py", serde_json::json!({}), None)
        .expect_err("must fail for blender GUI executable");
    assert!(
        err.contains("GUI executable"),
        "error must mention GUI executable: got {err}"
    );
}

#[test]
fn test_execute_script_allows_headless_interpreter() {
    let _g = clear_exec_env();
    // mayapy is the headless Maya interpreter — must NOT trigger the GUI guard
    unsafe { std::env::set_var("DCC_MCP_PYTHON_EXECUTABLE", "mayapy") };

    let result = execute_script("/does/not/exist.py", serde_json::json!({}), None);
    if let Err(err) = result {
        assert!(
            !err.contains("GUI executable"),
            "headless interpreter must not trigger GUI guard: got {err}"
        );
    }
}

#[test]
fn test_execute_script_allows_hython() {
    let _g = clear_exec_env();
    // hython is Houdini's headless interpreter
    unsafe { std::env::set_var("DCC_MCP_PYTHON_EXECUTABLE", "hython") };

    let result = execute_script("/does/not/exist.py", serde_json::json!({}), None);
    if let Err(err) = result {
        assert!(
            !err.contains("GUI executable"),
            "hython must not trigger GUI guard: got {err}"
        );
    }
}

// ── execute_script_in_process diagnostics (requires python-bindings) ─────────

#[cfg(feature = "python-bindings")]
#[test]
fn test_execute_script_in_process_not_initialized() {
    let result =
        super::execute::execute_script_in_process("/fake/script.py", serde_json::json!({}));
    let err = result.expect_err("must fail when Python is not initialized");
    assert!(
        err.contains("not initialized"),
        "error must mention 'not initialized': got {err}"
    );
    assert!(
        err.contains("SkillCatalog::with_in_process_executor"),
        "error must hint at in-process executor registration: got {err}"
    );
}

#[cfg(feature = "python-bindings")]
#[test]
fn test_execute_script_in_process_error_includes_script_path() {
    let result =
        super::execute::execute_script_in_process("/path/to/my_skill.py", serde_json::json!({}));
    let err = result.expect_err("must fail");
    assert!(
        err.contains("my_skill.py"),
        "error must include the script path: got {err}"
    );
}
