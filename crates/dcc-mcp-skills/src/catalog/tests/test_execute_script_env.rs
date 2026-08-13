//! Regression tests for issue #231 (silent ambient-python fallback) and
//! GUI-executable guard.
//!
//! These tests manipulate process env vars. Each test wraps its env-var
//! mutations in a shared [`EnvVarsGuard`] so that dropped values are
//! restored atomically, even on panic.
use super::*;
use dcc_mcp_test_utils::EnvVarsGuard;

#[cfg(feature = "python-bindings")]
use std::sync::Once;

#[cfg(feature = "python-bindings")]
static PYTHON_INIT: Once = Once::new();

/// Clear the three Python-execution env vars and return a guard that restores
/// them on drop. Tests that need a specific value should create their own
/// `EnvVarsGuard::set(&[...])` that includes these three clears followed by
/// the desired override.
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
    let _g = EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", Some("1")),
    ]);

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
    let _g = EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
        ("DCC_MCP_PYTHON_EXECUTABLE", Some("python")),
    ]);

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

#[cfg(feature = "python-bindings")]
#[test]
fn test_execute_script_uses_attached_python_when_path_lacks_python() {
    use pyo3::types::PyAnyMethods;

    const CHILD_ENV: &str = "DCC_MCP_TEST_ATTACHED_PYTHON_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("current test exe"))
            .arg(
                "catalog::tests::test_execute_script_env::test_execute_script_uses_attached_python_when_path_lacks_python",
            )
            .arg("--exact")
            .arg("--nocapture")
            .env(CHILD_ENV, "1")
            .output()
            .expect("run isolated attached-Python test");
        assert!(
            output.status.success(),
            "isolated test failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }

    let python_output = std::process::Command::new("python")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .expect("resolve a Python interpreter before clearing PATH");
    assert!(python_output.status.success());
    let python_executable = String::from_utf8(python_output.stdout)
        .expect("Python executable path is UTF-8")
        .trim()
        .to_string();

    PYTHON_INIT.call_once(pyo3::Python::initialize);
    pyo3::Python::attach(|py| {
        py.import("sys")
            .expect("import sys")
            .setattr("executable", python_executable.as_str())
            .expect("model an extension loaded by the active Python interpreter");
    });

    let empty_path = tempfile::tempdir().expect("create empty PATH root");
    let script_dir = tempfile::tempdir().expect("create skill script root");
    let script = script_dir.path().join("status.py");
    std::fs::write(
        &script,
        "import json\nprint(json.dumps({'success': True, 'runtime': 'attached'}))\n",
    )
    .expect("write skill script");

    let empty_path_value = empty_path.path().to_string_lossy().into_owned();
    let _g = EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
        ("PATH", Some(empty_path_value.as_str())),
    ]);

    let result = execute_script(
        script.to_str().expect("UTF-8 script path"),
        serde_json::json!({}),
        Some("freecad"),
    )
    .expect("the attached Python interpreter must execute the skill");

    assert_eq!(result["success"], true);
    assert_eq!(result["runtime"], "attached");
}

#[test]
fn test_attached_interpreter_filter_rejects_dcc_gui_binaries() {
    for executable in [
        "python.exe",
        "python3.14",
        "pythonw.exe",
        "pypy3",
        "mayapy.exe",
        "hython",
    ] {
        assert!(
            super::execute::is_python_cli_executable(std::path::Path::new(executable)),
            "{executable} must be accepted as a Python CLI",
        );
    }

    for executable in ["FreeCAD.exe", "openscad.exe", "blender.exe", "maya.exe"] {
        assert!(
            !super::execute::is_python_cli_executable(std::path::Path::new(executable)),
            "{executable} must never be auto-selected as a Python interpreter",
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
    let _g = EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
        ("DCC_MCP_PYTHON_EXECUTABLE", Some("maya.exe")),
    ]);

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
    let _g = EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
        (
            "DCC_MCP_PYTHON_EXECUTABLE",
            Some("/usr/autodesk/maya2024/bin/Maya"),
        ),
    ]);

    let err = execute_script("any_skill.py", serde_json::json!({}), None)
        .expect_err("must fail for GUI executable regardless of case");
    assert!(
        err.contains("GUI executable"),
        "error must mention GUI executable: got {err}"
    );
}

#[test]
fn test_execute_script_rejects_gui_executable_blender() {
    let _g = EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
        ("DCC_MCP_PYTHON_EXECUTABLE", Some("blender")),
    ]);

    let err = execute_script("any_skill.py", serde_json::json!({}), None)
        .expect_err("must fail for blender GUI executable");
    assert!(
        err.contains("GUI executable"),
        "error must mention GUI executable: got {err}"
    );
}

#[test]
fn test_execute_script_allows_headless_interpreter() {
    let _g = EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
        ("DCC_MCP_PYTHON_EXECUTABLE", Some("mayapy")),
    ]);

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
    let _g = EnvVarsGuard::set(&[
        ("DCC_MCP_PYTHON_EXECUTABLE", None),
        ("DCC_MCP_PYTHON_INIT_SNIPPET", None),
        ("DCC_MCP_ALLOW_AMBIENT_PYTHON", None),
        ("DCC_MCP_PYTHON_EXECUTABLE", Some("hython")),
    ]);

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
    const CHILD_ENV: &str = "DCC_MCP_TEST_PYTHON_NOT_INITIALIZED_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("current test exe"))
            .arg("catalog::tests::test_execute_script_env::test_execute_script_in_process_not_initialized")
            .arg("--exact")
            .arg("--nocapture")
            .env(CHILD_ENV, "1")
            .output()
            .expect("run isolated uninitialized-Python test");
        assert!(
            output.status.success(),
            "isolated test failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }

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
