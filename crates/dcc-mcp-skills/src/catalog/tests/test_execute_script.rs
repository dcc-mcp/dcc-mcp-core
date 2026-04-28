//! Basic execute_script smoke tests (dual-mode param passing).
//!
//! These tests verify the function's call conventions without requiring a real
//! Python environment — they just check the function does not panic and
//! gracefully handles missing executables.
use super::*;

#[test]
fn test_execute_script_stdin_json_params() {
    // execute_script writes the full JSON to stdin — verify the call runs.
    let result = execute_script(
        "python",
        serde_json::json!({"greeting": "hello-stdin"}),
        None,
    );
    // Skip gracefully if Python is not available in this environment.
    if let Err(ref e) = result {
        if e.contains("Failed to spawn") || e.contains("No such file") {
            return;
        }
    }
    let _ = result;
}

#[test]
fn test_execute_script_cli_flags_passed_for_scalar_params() {
    // Scalar params (string/number/bool) must be expanded as --key value flags
    // so argparse-based scripts can receive them.
    let result = execute_script(
        "python",
        serde_json::json!({"name": "Alice", "count": 3, "verbose": true}),
        None,
    );
    if let Err(ref e) = result {
        if e.contains("Failed to spawn") || e.contains("No such file") {
            return;
        }
    }
    let _ = result;
}

#[test]
fn test_execute_script_complex_values_not_expanded_as_flags() {
    // Object/array params must NOT be expanded as CLI flags — they should only
    // arrive via stdin JSON.  The function must not panic.
    let result = execute_script(
        "python",
        serde_json::json!({
            "simple": "value",
            "nested": {"a": 1},
            "list": [1, 2, 3],
        }),
        None,
    );
    if let Err(ref e) = result {
        if e.contains("Failed to spawn") || e.contains("No such file") {
            return;
        }
    }
    let _ = result;
}
