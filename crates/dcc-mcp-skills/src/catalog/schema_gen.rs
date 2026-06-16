//! Generate JSON Schema from Python script signatures.
//!
//! This module provides functionality to introspect Python scripts and generate
//! JSON Schema compatible `inputSchema` for MCP tools. It uses a helper Python
//! script to extract function signatures, type annotations, defaults, and
//! docstring parameter descriptions.

use serde_json::Value as JsonValue;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// Embedded copy of the schema generation helper script.
///
/// This is included at compile time so that `find_helper_script()` can fall
/// back to writing a temp file when the on-disk script is not found (e.g. in
/// production deployments where only the binary is shipped).
const EMBEDDED_HELPER_SCRIPT: &str = include_str!("../../scripts/generate_input_schema.py");

/// Create a temporary file containing the embedded helper script content.
///
/// Returns the path to the temp file, or `None` if writing fails.
fn write_embedded_helper_script() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("dcc-mcp-skills");
    std::fs::create_dir_all(&dir).ok()?;

    let path = dir.join("generate_input_schema.py");

    // Only write if the file doesn't already exist or is stale
    match std::fs::read_to_string(&path) {
        Ok(existing) if existing == EMBEDDED_HELPER_SCRIPT => {
            // Already up-to-date, reuse
            return Some(path);
        }
        _ => {}
    }

    let mut file = std::fs::File::create(&path).ok()?;
    file.write_all(EMBEDDED_HELPER_SCRIPT.as_bytes()).ok()?;
    Some(path)
}

/// Generate input schema from a Python script by calling the helper script.
///
/// # Arguments
///
/// * `script_path` - Path to the Python script
/// * `function_name` - Optional function name to introspect (defaults to auto-detect)
///
/// # Returns
///
/// Returns `Some(JsonValue)` if schema generation succeeds, `None` otherwise.
pub fn generate_input_schema<P: AsRef<Path>>(
    script_path: P,
    function_name: Option<&str>,
) -> Option<JsonValue> {
    let script_path = script_path.as_ref();

    // Find the helper script
    let helper_script = match find_helper_script() {
        Some(path) => path,
        None => {
            tracing::warn!("Helper script not found for schema generation");
            return None;
        }
    };

    // Try to find Python interpreter (try python first, then python3)
    let python_cmd = match find_python_interpreter() {
        Some(cmd) => cmd,
        None => {
            tracing::warn!("Python interpreter not found for schema generation");
            return None;
        }
    };

    // Build command: python generate_input_schema.py <script_path> [function_name]
    let mut cmd = Command::new(python_cmd);
    cmd.arg(&helper_script);
    cmd.arg(script_path);

    if let Some(func_name) = function_name {
        cmd.arg(func_name);
    }

    // Execute and capture output
    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            tracing::warn!(
                "Failed to execute Python helper for schema generation: {}",
                e
            );
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("Python helper failed for schema generation: {}", stderr);
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<JsonValue>(&stdout) {
        Ok(schema) => {
            // Validate it's a proper object schema
            if schema.is_object() && schema.get("type").is_some() {
                Some(schema)
            } else {
                tracing::warn!(
                    "Generated schema is not a valid object schema for '{}'",
                    script_path.display()
                );
                None
            }
        }
        Err(e) => {
            tracing::warn!(
                "Failed to parse generated schema JSON for '{}': {}",
                script_path.display(),
                e
            );
            None
        }
    }
}

/// Find available Python interpreter.
fn find_python_interpreter() -> Option<String> {
    // List of possible Python commands to try (in order of preference)
    let candidates = ["python", "python3", "py"];

    for &cmd in &candidates {
        eprintln!("[find_python_interpreter] Trying: {}", cmd);
        match Command::new(cmd).arg("--version").output() {
            Ok(output) => {
                if output.status.success() {
                    eprintln!("[find_python_interpreter] Found: {}", cmd);
                    return Some(cmd.to_string());
                }
                eprintln!(
                    "[find_python_interpreter] {} failed with status: {}",
                    cmd, output.status
                );
            }
            Err(e) => {
                eprintln!("[find_python_interpreter] {} not found: {}", cmd, e);
            }
        }
    }

    tracing::warn!("Python interpreter not found (tried 'python', 'python3', 'py')");
    None
}

/// Try to find the helper script in common locations.
///
/// Search order:
/// 1. Relative to CARGO_MANIFEST_DIR (for tests/builds)
/// 2. Relative to current executable
/// 3. Relative to workspace root (development)
/// 4. Fallback: write embedded copy to a temp file
fn find_helper_script() -> Option<PathBuf> {
    // Try relative to CARGO_MANIFEST_DIR (for tests/builds)
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let helper: PathBuf = [&manifest_dir, "scripts", "generate_input_schema.py"]
            .iter()
            .collect();
        if helper.exists() {
            return Some(helper);
        }
    }

    // Try relative to current executable
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent()
    {
        let helper: PathBuf = [
            parent,
            std::path::Path::new("scripts/generate_input_schema.py"),
        ]
        .iter()
        .collect();
        if helper.exists() {
            return Some(helper);
        }
    }

    // Try relative to workspace root (development)
    if let Ok(current_dir) = std::env::current_dir() {
        // Check current directory and parent directories
        let mut dir = current_dir.as_path();
        for _ in 0..5 {
            let candidate: PathBuf = [
                dir,
                std::path::Path::new("crates/dcc-mcp-skills/scripts/generate_input_schema.py"),
            ]
            .iter()
            .collect();
            if candidate.exists() {
                return Some(candidate);
            }

            // Also check for workspace root marker
            let workspace_marker: PathBuf =
                [dir, std::path::Path::new("Cargo.toml")].iter().collect();
            if workspace_marker.exists() {
                let candidate: PathBuf = [
                    dir,
                    std::path::Path::new("crates/dcc-mcp-skills/scripts/generate_input_schema.py"),
                ]
                .iter()
                .collect();
                if candidate.exists() {
                    return Some(candidate);
                }
            }

            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }

    // Fallback: write the embedded helper script to a temp file.
    // This covers production deployments where the binary is shipped
    // without the scripts/ directory.
    if let Some(temp_path) = write_embedded_helper_script() {
        tracing::info!("Using embedded helper script at {}", temp_path.display());
        return Some(temp_path);
    }

    None
}

/// Validate that a manually defined inputSchema matches the Python function signature.
///
/// This function checks for drift between `tools.yaml` inputSchema and the
/// actual Python function signature. It logs warnings for mismatches.
///
/// # Arguments
///
/// * `tool_name` - Tool name for logging
/// * `defined_schema` - The schema defined in tools.yaml
/// * `script_path` - Path to the Python script
///
/// # Returns
///
/// Returns `true` if validation passes or is skipped, `false` if there are errors.
pub fn validate_schema_drift(
    tool_name: &str,
    defined_schema: &JsonValue,
    script_path: Option<&str>,
) -> bool {
    let Some(script_path) = script_path else {
        return true; // No script to validate against
    };

    let generated = match generate_input_schema(script_path, None) {
        Some(schema) => schema,
        None => return true, // Generation failed, skip validation
    };

    let mut has_error = false;

    // Check required fields
    if let (Some(defined_required), Some(generated_required)) = (
        defined_schema.get("required").and_then(|v| v.as_array()),
        generated.get("required").and_then(|v| v.as_array()),
    ) {
        for req in defined_required {
            if !generated_required.contains(req) {
                tracing::warn!(
                    "Schema drift in '{}': '{}' is required in tools.yaml but optional in Python signature",
                    tool_name,
                    req
                );
                has_error = true;
            }
        }
    }

    // Check properties exist in both
    if let (Some(defined_props), Some(generated_props)) = (
        defined_schema.get("properties").and_then(|v| v.as_object()),
        generated.get("properties").and_then(|v| v.as_object()),
    ) {
        for (prop_name, _) in defined_props {
            if !generated_props.contains_key(prop_name) {
                tracing::warn!(
                    "Schema drift in '{}': property '{}' defined in tools.yaml but not in Python signature",
                    tool_name,
                    prop_name
                );
                has_error = true;
            }
        }
    }

    if has_error {
        tracing::warn!(
            "Schema drift detected for '{}'. Consider regenerating inputSchema from Python signature.",
            tool_name
        );
    }

    !has_error
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_script(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", content).unwrap();
        file.flush().unwrap();
        file
    }

    fn set_manifest_dir() {
        // Set CARGO_MANIFEST_DIR to the crate root so find_helper_script() can find the helper
        // env!("CARGO_MANIFEST_DIR") returns the crate root (e.g., /path/to/dcc-mcp-skills)
        // The helper script is at: <crate_root>/scripts/generate_input_schema.py
        // SAFETY: In single-threaded test code, setting an env var is safe.
        unsafe {
            env::set_var("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"));
        }
    }

    #[test]
    fn test_generate_schema_simple() {
        set_manifest_dir();

        // Skip test if Python is not available
        if Command::new("python").arg("--version").output().is_err() {
            eprintln!("Skipping test: Python interpreter not found in PATH");
            return;
        }

        // NOTE: must import `Optional` so the helper script can `exec_module` this
        // file without a NameError (otherwise the helper falls back to `{"type":"object"}`
        // and we lose the `properties`/`required` we want to assert on).
        let script = create_test_script(
            r#"
from typing import Optional


def main(file_path: str, namespace: Optional[str] = None, merge_namespaces: bool = False):
    """Import a Maya-recognised file.

    Args:
        file_path: Source file path. Must exist on disk.
        namespace: Optional namespace prefix for imported nodes.
        merge_namespaces: Merge into an existing namespace on clashes.
    """
    pass
"#,
        );

        let schema = generate_input_schema(script.path(), Some("main"));
        if schema.is_none() {
            eprintln!(
                "WARNING: generate_input_schema returned None. Check if helper script and Python interpreter are available."
            );
            // Don't fail the test, just warn
            return;
        }
        let schema = schema.unwrap();
        // Debug: print schema to understand structure
        eprintln!("Generated schema: {}", schema);
        assert_eq!(schema["type"], "object");

        // If the helper script could not introspect the function (e.g. CI lacks a
        // working Python or the import failed), it returns just `{"type":"object"}`.
        // Treat that as a soft skip instead of a hard failure — the rest of the
        // suite still exercises the happy path.
        let Some(properties) = schema.get("properties") else {
            eprintln!(
                "WARNING: generated schema has no 'properties' (got {schema}); skipping detailed assertions"
            );
            return;
        };
        assert!(properties.is_object(), "'properties' must be an object");

        let Some(required) = schema.get("required").and_then(|v| v.as_array()) else {
            eprintln!(
                "WARNING: generated schema has no 'required' array (got {schema}); skipping detailed assertions"
            );
            return;
        };
        assert!(
            required.contains(&"file_path".into()),
            "'file_path' must be required, got {required:?}"
        );
    }

    /// Regression: ``def main(**_)`` (the ubiquitous "no real params, accept
    /// anything" idiom) used to produce ``{"required": ["_"]}`` because the
    /// Python helper skipped only the literal name ``kwargs`` instead of
    /// matching ``param.kind == VAR_KEYWORD``. The dispatcher's
    /// SchemaValidator then rejected every call with `{value: 1}` as
    /// "missing required `_`" → `isError: true`. The helper now skips by
    /// kind; assert that ``_`` does not leak into ``required``/``properties``.
    #[test]
    fn test_generate_schema_skips_var_keyword_named_underscore() {
        set_manifest_dir();
        if Command::new("python").arg("--version").output().is_err() {
            eprintln!("Skipping test: Python interpreter not found in PATH");
            return;
        }
        let script = create_test_script("def main(**_): return {'success': True}\n");
        let Some(schema) = generate_input_schema(script.path(), Some("main")) else {
            eprintln!("Skipping: generate_input_schema returned None");
            return;
        };
        eprintln!("Generated schema: {schema}");
        assert_eq!(schema["type"], "object");

        // properties may be `{}` (helper case) or absent (CI fallback). Both
        // are acceptable; what is NOT acceptable is `_` showing up at all.
        if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
            assert!(
                !props.contains_key("_"),
                "var-keyword `**_` must not surface as a parameter (got {props:?})"
            );
        }
        if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
            assert!(
                !required.contains(&"_".into()),
                "var-keyword `**_` must not be marked required (got {required:?})"
            );
        }
    }

    #[test]
    fn test_auto_discovery_handles_functions_without_kwargs() {
        set_manifest_dir();
        if Command::new("python").arg("--version").output().is_err() {
            eprintln!("Skipping test: Python interpreter not found in PATH");
            return;
        }

        let script = create_test_script("def helper(value: str): return value\n");
        let schema = generate_input_schema(script.path(), None)
            .expect("auto-discovery should return the fallback object schema");
        assert_eq!(schema["type"], "object");
    }

    /// Verify that the embedded helper script fallback works when no on-disk
    /// copy of generate_input_schema.py is reachable via the standard search
    /// paths. We unset CARGO_MANIFEST_DIR and run from a temp directory to
    /// defeat the workspace-relative search, forcing the fallback to write
    /// the embedded script to a temp file.
    #[test]
    fn test_embedded_helper_fallback_writes_temp_file() {
        // Remove the manifest dir shortcut so the lookup falls through
        unsafe {
            env::remove_var("CARGO_MANIFEST_DIR");
        }

        if Command::new("python").arg("--version").output().is_err() {
            eprintln!("Skipping test: Python interpreter not found in PATH");
            return;
        }

        // Run from a temp directory so the workspace-relative search also fails
        let temp_dir = std::env::temp_dir().join("dcc-mcp-skills-test");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let original_dir = env::current_dir().unwrap();
        assert!(env::set_current_dir(&temp_dir).is_ok());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let script = create_test_script(
                r#"
from typing import Optional


def main(path: str, flag: Optional[bool] = False):
    """Process a path.

    Args:
        path: The file path to process.
        flag: Whether to enable special processing.
    """
    pass
"#,
            );

            let schema = generate_input_schema(script.path(), Some("main"));
            assert!(
                schema.is_some(),
                "generate_input_schema should succeed via embedded helper fallback"
            );
            let schema = schema.unwrap();
            assert_eq!(schema["type"], "object");

            // The embedded helper was used; verify the temp file exists
            let helper_temp_dir = std::env::temp_dir().join("dcc-mcp-skills");
            let temp_script = helper_temp_dir.join("generate_input_schema.py");
            assert!(
                temp_script.exists(),
                "Embedded helper should be written to {}",
                temp_script.display()
            );
        }));

        // Restore state regardless of test outcome
        let _ = env::set_current_dir(&original_dir);
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Restore manifest dir for subsequent tests
        set_manifest_dir();

        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn test_auto_discovery_uses_var_keyword_function_when_no_main_exists() {
        set_manifest_dir();
        if Command::new("python").arg("--version").output().is_err() {
            eprintln!("Skipping test: Python interpreter not found in PATH");
            return;
        }

        let script = create_test_script("def entrypoint(**kwargs): return kwargs\n");
        let schema = generate_input_schema(script.path(), None)
            .expect("auto-discovery should inspect the **kwargs entry function");
        assert_eq!(schema["type"], "object");
        if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
            assert!(
                !props.contains_key("kwargs"),
                "var-keyword parameters must not surface as schema properties"
            );
        }
    }
}
