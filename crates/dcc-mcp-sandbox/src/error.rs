//! Sandbox error types.

use thiserror::Error;

/// Errors that can occur within the sandbox.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum SandboxError {
    /// The script exceeded the configured execution time limit.
    #[error("execution timed out after {timeout_ms}ms")]
    Timeout {
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    /// The requested action is not permitted by the API whitelist.
    #[error("action '{action}' is not allowed by the API whitelist")]
    ActionNotAllowed {
        /// The denied action name.
        action: String,
    },

    /// The requested file path is outside all allowed directories.
    #[error("path '{path}' is outside allowed directories")]
    PathNotAllowed {
        /// The denied path.
        path: String,
    },

    /// Input validation failed for a parameter.
    #[error("input validation failed for '{field}': {reason}")]
    ValidationFailed {
        /// The field that failed validation.
        field: String,
        /// The reason for the failure.
        reason: String,
    },

    /// Sandbox is in read-only mode; write operations are forbidden.
    #[error("sandbox is in read-only mode; write operation '{operation}' is not permitted")]
    ReadOnlyViolation {
        /// The attempted write operation.
        operation: String,
    },

    /// Maximum number of allowed actions per execution was exceeded.
    #[error("exceeded maximum action count of {limit} (attempted {attempted})")]
    ActionLimitExceeded {
        /// The configured limit.
        limit: u32,
        /// The number attempted.
        attempted: u32,
    },

    /// Serialization / deserialization error wrapping a message.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Internal error, should not normally occur.
    #[error("internal sandbox error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_display() {
        let err = SandboxError::Timeout { timeout_ms: 5000 };
        assert!(err.to_string().contains("5000ms"));
    }

    #[test]
    fn test_action_not_allowed_display() {
        let err = SandboxError::ActionNotAllowed {
            action: "delete_scene".to_string(),
        };
        assert!(err.to_string().contains("delete_scene"));
        assert!(err.to_string().contains("whitelist"));
    }

    #[test]
    fn test_path_not_allowed_display() {
        let err = SandboxError::PathNotAllowed {
            path: "/etc/passwd".to_string(),
        };
        assert!(err.to_string().contains("/etc/passwd"));
    }

    #[test]
    fn test_validation_failed_display() {
        let err = SandboxError::ValidationFailed {
            field: "script".to_string(),
            reason: "empty input".to_string(),
        };
        assert!(err.to_string().contains("script"));
        assert!(err.to_string().contains("empty input"));
    }

    #[test]
    fn test_read_only_violation_display() {
        let err = SandboxError::ReadOnlyViolation {
            operation: "create_mesh".to_string(),
        };
        assert!(err.to_string().contains("read-only"));
        assert!(err.to_string().contains("create_mesh"));
    }

    #[test]
    fn test_action_limit_exceeded_display() {
        let err = SandboxError::ActionLimitExceeded {
            limit: 100,
            attempted: 101,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("101"));
    }

    #[test]
    fn test_clone_and_partial_eq() {
        let err = SandboxError::Timeout { timeout_ms: 1000 };
        assert_eq!(err.clone(), err);
        let other = SandboxError::Timeout { timeout_ms: 2000 };
        assert_ne!(err, other);
    }
}
