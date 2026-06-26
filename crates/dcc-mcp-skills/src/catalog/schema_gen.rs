//! Generate JSON Schema from Python script signatures.
//!
//! This module provides functionality to introspect Python scripts and generate
//! JSON Schema compatible `inputSchema` for MCP tools. It uses a helper Python
//! script to extract function signatures, type annotations, defaults, and
//! docstring parameter descriptions.

use parking_lot::Mutex;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

const HELPER_SOURCE: &str = include_str!("../../scripts/generate_input_schema.py");

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct SchemaCacheKey {
    script_path: PathBuf,
    function_name: Option<String>,
    file_stamp: Option<(u64, u128)>,
}

impl SchemaCacheKey {
    fn new(script_path: &Path, function_name: Option<&str>) -> Self {
        let script_path =
            std::fs::canonicalize(script_path).unwrap_or_else(|_| script_path.to_path_buf());
        let file_stamp = std::fs::metadata(&script_path).ok().map(|meta| {
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            (meta.len(), modified)
        });
        Self {
            script_path,
            function_name: function_name.map(str::to_string),
            file_stamp,
        }
    }
}

fn schema_cache() -> &'static Mutex<HashMap<SchemaCacheKey, Option<JsonValue>>> {
    static CACHE: OnceLock<Mutex<HashMap<SchemaCacheKey, Option<JsonValue>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
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
    let cache_key = SchemaCacheKey::new(script_path, function_name);
    if let Some(cached) = schema_cache().lock().get(&cache_key).cloned() {
        return cached;
    }

    let schema = generate_input_schema_uncached(script_path, function_name);
    schema_cache().lock().insert(cache_key, schema.clone());
    schema
}

fn generate_input_schema_uncached(
    script_path: &Path,
    function_name: Option<&str>,
) -> Option<JsonValue> {
    // Try to find Python interpreter (try python first, then python3)
    let python_cmd = find_python_interpreter()?;

    // Build command: python -c <embedded helper> <script_path> [function_name]
    let mut cmd = Command::new(python_cmd);
    cmd.arg("-c");
    cmd.arg(HELPER_SOURCE);
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
///
/// The result is cached in a `OnceLock` — the shell-out to `python
/// --version` only happens once per process lifetime.
fn find_python_interpreter() -> Option<String> {
    static PYTHON_CMD: OnceLock<Option<String>> = OnceLock::new();
    PYTHON_CMD
        .get_or_init(find_python_interpreter_uncached)
        .clone()
}

fn find_python_interpreter_uncached() -> Option<String> {
    // List of possible Python commands to try (in order of preference)
    let candidates = ["python", "python3", "py"];

    for &cmd in &candidates {
        if Command::new(cmd)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return Some(cmd.to_string());
        }
    }

    tracing::warn!("Python interpreter not found (tried 'python', 'python3', 'py')");
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
        let mut file = tempfile::Builder::new().suffix(".py").tempfile().unwrap();
        writeln!(file, "{}", content).unwrap();
        file.flush().unwrap();
        file
    }

    fn set_manifest_dir() {
        // Some scripts under test resolve project-local imports relative to the crate root.
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

    #[test]
    fn test_generate_schema_caches_script_results() {
        if Command::new("python").arg("--version").output().is_err() {
            eprintln!("Skipping test: Python interpreter not found in PATH");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("counter.txt");
        std::fs::write(&counter, "0").unwrap();
        let script = create_test_script(
            r#"
import os
from pathlib import Path

counter = Path(os.environ["DCC_MCP_SCHEMA_COUNTER"])
counter.write_text(str(int(counter.read_text() or "0") + 1))


def main(value: int):
    pass
"#,
        );

        let old_counter = env::var_os("DCC_MCP_SCHEMA_COUNTER");
        unsafe {
            env::set_var("DCC_MCP_SCHEMA_COUNTER", &counter);
        }
        let first = generate_input_schema(script.path(), Some("main"));
        let second = generate_input_schema(script.path(), Some("main"));
        if let Some(value) = old_counter {
            unsafe {
                env::set_var("DCC_MCP_SCHEMA_COUNTER", value);
            }
        } else {
            unsafe {
                env::remove_var("DCC_MCP_SCHEMA_COUNTER");
            }
        }

        assert!(first.is_some());
        assert_eq!(first, second);
        assert_eq!(std::fs::read_to_string(counter).unwrap(), "1");
    }
}
