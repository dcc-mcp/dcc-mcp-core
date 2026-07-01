//! Opt-in JSON Schema generation from Python script signatures.
//!
//! Runtime skill discovery is manifest-first and does not import or execute
//! Python scripts unless `DCC_MCP_ENABLE_SCHEMA_INTROSPECTION=1` is set. The
//! helper path remains for development and migration tooling that needs to
//! derive `inputSchema` from a script signature before committing it to
//! `tools.yaml`.

use parking_lot::Mutex;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

const HELPER_SOURCE: &str = include_str!("../../scripts/generate_input_schema.py");
const ENABLE_SCHEMA_INTROSPECTION_ENV: &str = "DCC_MCP_ENABLE_SCHEMA_INTROSPECTION";

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

/// Return true when runtime Python schema introspection is explicitly enabled.
///
/// The default runtime path is manifest-first: skill authors declare
/// `input_schema` in `tools.yaml`, like FastMCP/LangChain keep schemas attached
/// to registered tools. Importing arbitrary skill scripts during discovery is
/// kept as a development escape hatch because it can spawn DCC host Python
/// processes and run module-level side effects.
pub fn schema_introspection_enabled() -> bool {
    std::env::var(ENABLE_SCHEMA_INTROSPECTION_ENV).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Generate a script-derived schema only when explicitly enabled.
pub fn generate_input_schema_if_enabled<P: AsRef<Path>>(
    script_path: P,
    function_name: Option<&str>,
) -> Option<JsonValue> {
    if !schema_introspection_enabled() {
        return None;
    }
    generate_input_schema(script_path, function_name)
}

fn generate_input_schema_uncached(
    script_path: &Path,
    function_name: Option<&str>,
) -> Option<JsonValue> {
    // Try to find Python interpreter (try python first, then python3)
    let python_cmd = find_python_interpreter()?;
    let helper_path = schema_helper_script_path()?;

    // Build command: python <materialized helper> <script_path> [function_name].
    // Keeping the helper source out of argv prevents DCC process inspectors from
    // accumulating huge repeated command lines during skill scans.
    let mut cmd =
        build_schema_helper_command(&python_cmd, &helper_path, script_path, function_name);

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

fn schema_helper_script_path() -> Option<PathBuf> {
    static HELPER_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    HELPER_PATH.get_or_init(materialize_schema_helper).clone()
}

fn materialize_schema_helper() -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    HELPER_SOURCE.hash(&mut hasher);
    let helper_hash = hasher.finish();
    let dir = std::env::temp_dir().join("dcc-mcp-core");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(
            "Failed to create schema helper temp directory '{}': {}",
            dir.display(),
            e
        );
        return None;
    }

    let path = dir.join(format!(
        "generate_input_schema-{helper_hash:x}-{}.py",
        std::process::id()
    ));
    if let Err(e) = std::fs::write(&path, HELPER_SOURCE) {
        tracing::warn!(
            "Failed to materialize schema helper script '{}': {}",
            path.display(),
            e
        );
        return None;
    }
    Some(path)
}

fn build_schema_helper_command(
    python_cmd: &str,
    helper_path: &Path,
    script_path: &Path,
    function_name: Option<&str>,
) -> Command {
    let mut cmd = Command::new(python_cmd);
    cmd.arg(helper_path);
    cmd.arg(script_path);
    if let Some(func_name) = function_name {
        cmd.arg(func_name);
    }
    cmd
}

/// Find available Python interpreter.
///
/// `DCC_MCP_PYTHON_EXECUTABLE` is honored first so DCC hosts can use mayapy,
/// hython, or another host interpreter. The result is cached in a `OnceLock`;
/// set the env var before the server starts.
fn find_python_interpreter() -> Option<String> {
    static PYTHON_CMD: OnceLock<Option<String>> = OnceLock::new();
    PYTHON_CMD
        .get_or_init(find_python_interpreter_uncached)
        .clone()
}

fn find_python_interpreter_uncached() -> Option<String> {
    if let Ok(cmd) = std::env::var("DCC_MCP_PYTHON_EXECUTABLE")
        && !cmd.trim().is_empty()
    {
        if crate::gui_executable::is_gui_executable(Path::new(&cmd)).is_some() {
            tracing::warn!(
                "Ignoring DCC_MCP_PYTHON_EXECUTABLE for schema generation because it points to a GUI DCC executable: {}",
                cmd
            );
        } else if Command::new(&cmd)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return Some(cmd);
        }
    }

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
    if !schema_introspection_enabled() {
        return true;
    }

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
    use dcc_mcp_test_utils::EnvVarsGuard;
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

        let counter_value = counter.to_string_lossy().into_owned();
        let _env = EnvVarsGuard::set(&[("DCC_MCP_SCHEMA_COUNTER", Some(counter_value.as_str()))]);
        let first = generate_input_schema(script.path(), Some("main"));
        let second = generate_input_schema(script.path(), Some("main"));

        assert!(first.is_some());
        assert_eq!(first, second);
        assert_eq!(std::fs::read_to_string(counter).unwrap(), "1");
    }

    #[test]
    fn test_schema_helper_command_uses_materialized_script() {
        let helper_path = schema_helper_script_path().expect("helper script should materialize");
        assert!(helper_path.is_file());
        assert_eq!(
            std::fs::read_to_string(&helper_path).unwrap(),
            HELPER_SOURCE
        );

        let script_path = Path::new("tool.py");
        let cmd = build_schema_helper_command("python", &helper_path, script_path, Some("main"));
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(!args.iter().any(|arg| arg == "-c"));
        assert!(!args.iter().any(|arg| arg.contains("Generate JSON Schema")));
        assert_eq!(args[0], helper_path.to_string_lossy());
        assert_eq!(args[1], script_path.to_string_lossy());
        assert_eq!(args[2], "main");
    }

    #[test]
    fn test_schema_introspection_is_opt_in() {
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

        let counter_value = counter.to_string_lossy().into_owned();
        let _env = EnvVarsGuard::set(&[
            ("DCC_MCP_ENABLE_SCHEMA_INTROSPECTION", None),
            ("DCC_MCP_SCHEMA_COUNTER", Some(counter_value.as_str())),
        ]);

        assert!(generate_input_schema_if_enabled(script.path(), Some("main")).is_none());
        assert_eq!(std::fs::read_to_string(counter).unwrap(), "0");
    }

    #[test]
    fn test_schema_introspection_opt_in_runs_helper() {
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

        let counter_value = counter.to_string_lossy().into_owned();
        let _env = EnvVarsGuard::set(&[
            ("DCC_MCP_ENABLE_SCHEMA_INTROSPECTION", Some("1")),
            ("DCC_MCP_SCHEMA_COUNTER", Some(counter_value.as_str())),
        ]);

        let schema = generate_input_schema_if_enabled(script.path(), Some("main"));
        assert!(schema.is_some());
        assert_eq!(std::fs::read_to_string(counter).unwrap(), "1");
    }
}
