//! Constants for the DCC-MCP ecosystem.

use std::sync::LazyLock;

/// Application name used for platform-specific directory resolution.
pub const APP_NAME: &str = "dcc-mcp";
/// Application author identifier (exposed to Python consumers).
pub const APP_AUTHOR: &str = "dcc-mcp";
/// Default log level when `ENV_LOG_LEVEL` is not set.
pub const DEFAULT_LOG_LEVEL: &str = "DEBUG";
/// Environment variable name for overriding the log level.
pub const ENV_LOG_LEVEL: &str = "MCP_LOG_LEVEL";

/// Filename expected at the root of every skill package.
pub const SKILL_METADATA_FILE: &str = "SKILL.md";
/// Environment variable containing additional skill search paths.
pub const ENV_SKILL_PATHS: &str = "DCC_MCP_SKILL_PATHS";
/// Subdirectory inside a skill package that holds executable scripts.
pub const SKILL_SCRIPTS_DIR: &str = "scripts";
/// Subdirectory inside a skill package that holds auxiliary metadata files.
pub const SKILL_METADATA_DIR: &str = "metadata";
/// Filename for the dependency listing inside the metadata/ directory.
pub const DEPENDS_FILE: &str = "depends.md";
/// Default DCC name when none is specified.
pub const DEFAULT_DCC: &str = "python";
/// Default version string for skills and actions.
pub const DEFAULT_VERSION: &str = "1.0.0";
/// Default MIME type for MCP resources.
pub const DEFAULT_MIME_TYPE: &str = "text/plain";

// ── ActionResult-related constants ──

/// Default error type when the error message doesn't follow the `Type: details` pattern.
pub const DEFAULT_ERROR_TYPE: &str = "Exception";
/// Default user-facing prompt for exception-based results.
pub const DEFAULT_ERROR_PROMPT: &str = "Please check error details and retry";
/// Default success message for wrapped non-dict results.
pub const DEFAULT_SUCCESS_MESSAGE: &str = "Successfully processed result";
/// Context key for the error type string.
pub const CTX_KEY_ERROR_TYPE: &str = "error_type";
/// Context key for the traceback string.
pub const CTX_KEY_TRACEBACK: &str = "traceback";
/// Context key for the wrapped value.
pub const CTX_KEY_VALUE: &str = "value";
/// Context key for possible solutions list.
pub const CTX_KEY_POSSIBLE_SOLUTIONS: &str = "possible_solutions";

// ── ActionResult known keys (for dict validation) ──

/// Standard keys extracted from a dict during `validate_action_result`.
pub const ACTION_RESULT_KNOWN_KEYS: &[&str] = &["success", "message", "prompt", "error"];

// ── Schema defaults ──

/// Default JSON schema for action input/output when none is provided.
///
/// Backed by a `LazyLock` to avoid heap-allocating a new `serde_json::Value`
/// on every call.
static DEFAULT_SCHEMA: LazyLock<serde_json::Value> =
    LazyLock::new(|| serde_json::json!({"type": "object", "properties": {}}));

/// Return a reference to the default JSON schema.
///
/// Callers that need ownership should `.clone()` the returned reference.
#[must_use]
pub fn default_schema() -> &'static serde_json::Value {
    &DEFAULT_SCHEMA
}

/// Tolerance in seconds for file modification time comparison in cache checks.
pub const MTIME_EPSILON_SECS: f64 = 0.001;

/// Supported script extensions → script type name (compile-time constant).
pub const SUPPORTED_SCRIPT_EXTENSIONS: &[(&str, &str)] = &[
    (".py", "python"),
    (".mel", "mel"),
    (".ms", "maxscript"),
    (".bat", "batch"),
    (".cmd", "batch"),
    (".sh", "shell"),
    (".bash", "shell"),
    (".ps1", "powershell"),
    (".vbs", "vbscript"),
    (".jsx", "javascript"),
    (".js", "javascript"),
];

/// Normalize an extension to bare form (strip optional leading dot).
fn normalize_ext(ext: &str) -> &str {
    ext.strip_prefix('.').unwrap_or(ext)
}

/// Check if a file extension is a supported script extension.
///
/// Accepts both dotted (`.py`) and bare (`py`) forms; comparison is case-insensitive.
#[must_use]
pub fn is_supported_extension(ext: &str) -> bool {
    let bare = normalize_ext(ext);
    SUPPORTED_SCRIPT_EXTENSIONS.iter().any(|(e, _)| {
        e.strip_prefix('.')
            .is_some_and(|b| b.eq_ignore_ascii_case(bare))
    })
}

/// Look up the script type name for a file extension.
///
/// Accepts both dotted (`.py`) and bare (`py`) forms; comparison is case-insensitive.
/// Returns `None` if the extension is not recognized.
#[must_use]
pub fn get_script_type(ext: &str) -> Option<&'static str> {
    let bare = normalize_ext(ext);
    SUPPORTED_SCRIPT_EXTENSIONS.iter().find_map(|(e, t)| {
        e.strip_prefix('.')
            .filter(|b| b.eq_ignore_ascii_case(bare))
            .map(|_| *t)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_supported_extension ──────────────────────────────────────────────────

    #[test]
    fn test_is_supported_extension_valid() {
        // Both dotted and bare forms work
        assert!(is_supported_extension(".py"));
        assert!(is_supported_extension("py"));
        assert!(is_supported_extension(".sh"));
        assert!(is_supported_extension("sh"));
        assert!(is_supported_extension(".js"));
        assert!(is_supported_extension("PY")); // case-insensitive
    }

    #[test]
    fn test_is_supported_extension_all_supported() {
        for (ext, _) in SUPPORTED_SCRIPT_EXTENSIONS {
            assert!(
                is_supported_extension(ext),
                "expected {ext} to be supported"
            );
            // bare form (strip leading dot)
            let bare = ext.strip_prefix('.').unwrap_or(ext);
            assert!(is_supported_extension(bare), "bare form {bare} should work");
        }
    }

    #[test]
    fn test_is_supported_extension_invalid() {
        assert!(!is_supported_extension(".txt"));
        assert!(!is_supported_extension(".rs"));
        assert!(!is_supported_extension("txt"));
        assert!(!is_supported_extension(""));
        assert!(!is_supported_extension("toml"));
        assert!(!is_supported_extension(".json"));
    }

    #[test]
    fn test_is_supported_extension_case_insensitive() {
        assert!(is_supported_extension("PY"));
        assert!(is_supported_extension(".MEL"));
        assert!(is_supported_extension("MS"));
        assert!(is_supported_extension(".PS1"));
    }

    // ── get_script_type ─────────────────────────────────────────────────────────

    #[test]
    fn test_get_script_type() {
        // Both dotted and bare forms work
        assert_eq!(get_script_type(".py"), Some("python"));
        assert_eq!(get_script_type("py"), Some("python"));
        assert_eq!(get_script_type(".sh"), Some("shell"));
        assert_eq!(get_script_type(".bat"), Some("batch"));
        assert_eq!(get_script_type(".js"), Some("javascript"));
        assert_eq!(get_script_type(".mel"), Some("mel"));
        assert_eq!(get_script_type(".txt"), None);
        assert_eq!(get_script_type("PY"), Some("python")); // case-insensitive
    }

    #[test]
    fn test_get_script_type_all_supported() {
        for (ext, expected_type) in SUPPORTED_SCRIPT_EXTENSIONS {
            let result = get_script_type(ext);
            assert_eq!(
                result,
                Some(*expected_type),
                "ext={ext} should map to {expected_type}"
            );
        }
    }

    #[test]
    fn test_get_script_type_unknown_returns_none() {
        assert!(get_script_type(".rs").is_none());
        assert!(get_script_type(".md").is_none());
        assert!(get_script_type("").is_none());
    }

    #[test]
    fn test_get_script_type_batch_variants() {
        // Both .bat and .cmd map to "batch"
        assert_eq!(get_script_type(".bat"), Some("batch"));
        assert_eq!(get_script_type(".cmd"), Some("batch"));
    }

    #[test]
    fn test_get_script_type_shell_variants() {
        // Both .sh and .bash map to "shell"
        assert_eq!(get_script_type(".sh"), Some("shell"));
        assert_eq!(get_script_type(".bash"), Some("shell"));
    }

    #[test]
    fn test_get_script_type_jsx_is_javascript() {
        assert_eq!(get_script_type(".jsx"), Some("javascript"));
        assert_eq!(get_script_type(".js"), Some("javascript"));
    }

    // ── default_schema ───────────────────────────────────────────────────────────

    #[test]
    fn test_default_schema() {
        let schema = default_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
    }

    #[test]
    fn test_default_schema_is_same_instance() {
        // LazyLock should return the same pointer
        let a = default_schema() as *const _;
        let b = default_schema() as *const _;
        assert_eq!(a, b);
    }

    // ── ACTION_RESULT_KNOWN_KEYS ─────────────────────────────────────────────────

    #[test]
    fn test_action_result_known_keys() {
        assert!(ACTION_RESULT_KNOWN_KEYS.contains(&"success"));
        assert!(ACTION_RESULT_KNOWN_KEYS.contains(&"message"));
        assert!(ACTION_RESULT_KNOWN_KEYS.contains(&"prompt"));
        assert!(ACTION_RESULT_KNOWN_KEYS.contains(&"error"));
        assert!(!ACTION_RESULT_KNOWN_KEYS.contains(&"context"));
        assert!(!ACTION_RESULT_KNOWN_KEYS.contains(&""));
    }

    // ── String constants ─────────────────────────────────────────────────────────

    #[test]
    fn test_app_name_not_empty() {
        assert_eq!(APP_NAME, "dcc-mcp");
    }

    #[test]
    fn test_default_dcc_not_empty() {
        assert_eq!(DEFAULT_DCC, "python");
    }

    #[test]
    fn test_default_version_semver_like() {
        let parts: Vec<&str> = DEFAULT_VERSION.split('.').collect();
        assert!(
            parts.len() >= 2,
            "DEFAULT_VERSION should be semver-like, got: {DEFAULT_VERSION}"
        );
    }

    #[test]
    fn test_mtime_epsilon_positive() {
        assert_eq!(MTIME_EPSILON_SECS, 0.001);
    }

    #[test]
    fn test_skill_metadata_file_constant() {
        assert_eq!(SKILL_METADATA_FILE, "SKILL.md");
        assert!(SKILL_METADATA_FILE.ends_with(".md"));
    }

    #[test]
    fn test_env_var_constants_not_empty() {
        assert_eq!(ENV_SKILL_PATHS, "DCC_MCP_SKILL_PATHS");
        assert_eq!(ENV_LOG_LEVEL, "MCP_LOG_LEVEL");
    }

    #[test]
    fn test_ctx_key_constants_not_empty() {
        assert_eq!(CTX_KEY_ERROR_TYPE, "error_type");
        assert_eq!(CTX_KEY_TRACEBACK, "traceback");
        assert_eq!(CTX_KEY_VALUE, "value");
        assert_eq!(CTX_KEY_POSSIBLE_SOLUTIONS, "possible_solutions");
    }
}
