//! Constants for the DCC-MCP ecosystem.
//!
//! Logging-related constants live in `dcc-mcp-logging::constants` (issue #496).
//! Skill-domain constants (`SKILL_*`, `ENV_*_SKILL_*`, `*_env_key`,
//! `MTIME_EPSILON_SECS`, `SUPPORTED_SCRIPT_EXTENSIONS`, `is_supported_extension`,
//! `get_script_type`) live in `dcc-mcp-skills::constants` (issue #498).

use std::sync::LazyLock;

/// Application name used for platform-specific directory resolution.
pub const APP_NAME: &str = "dcc-mcp";
/// Application author identifier (exposed to Python consumers).
pub const APP_AUTHOR: &str = "dcc-mcp";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_schema() {
        let schema = default_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
    }

    #[test]
    fn test_default_schema_is_same_instance() {
        let a = default_schema() as *const _;
        let b = default_schema() as *const _;
        assert_eq!(a, b);
    }

    #[test]
    fn test_action_result_known_keys() {
        assert!(ACTION_RESULT_KNOWN_KEYS.contains(&"success"));
        assert!(ACTION_RESULT_KNOWN_KEYS.contains(&"message"));
        assert!(ACTION_RESULT_KNOWN_KEYS.contains(&"prompt"));
        assert!(ACTION_RESULT_KNOWN_KEYS.contains(&"error"));
        assert!(!ACTION_RESULT_KNOWN_KEYS.contains(&"context"));
        assert!(!ACTION_RESULT_KNOWN_KEYS.contains(&""));
    }

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
    fn test_ctx_key_constants_not_empty() {
        assert_eq!(CTX_KEY_ERROR_TYPE, "error_type");
        assert_eq!(CTX_KEY_TRACEBACK, "traceback");
        assert_eq!(CTX_KEY_VALUE, "value");
        assert_eq!(CTX_KEY_POSSIBLE_SOLUTIONS, "possible_solutions");
    }
}
