//! Sandbox execution context — the runtime state for one sandboxed session.
//!
//! A [`SandboxContext`] bundles a [`SandboxPolicy`], an [`AuditLog`], and a
//! mutable action counter.  It is the single entry point that higher-level
//! code (MCP server, test harness, Python bindings) should interact with.

use std::time::{Duration, Instant};

use serde_json::Value;
use tracing::{debug, warn};

use crate::audit::{AuditEntry, AuditLog, AuditOutcome};
use crate::error::SandboxError;
use crate::policy::SandboxPolicy;
use crate::validator::InputValidator;

// ── ExecutionResult ───────────────────────────────────────────────────────────

/// The result of executing an action within the sandbox.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// The raw JSON value returned by the action handler, if any.
    pub value: Option<Value>,
    /// Wall-clock duration of the execution.
    pub duration: Duration,
}

// ── ActionHandler ─────────────────────────────────────────────────────────────

/// A synchronous action handler callable by the sandbox.
///
/// The handler receives the validated parameter map and should return
/// either a JSON `Value` or a [`SandboxError`].
pub type ActionHandler =
    Box<dyn Fn(&serde_json::Map<String, Value>) -> Result<Value, SandboxError> + Send + Sync>;

// ── SandboxContext ────────────────────────────────────────────────────────────

/// Runtime sandbox context for a single session.
///
/// # Example
/// ```rust,no_run
/// use dcc_mcp_sandbox::{SandboxContext, SandboxPolicy};
/// use serde_json::json;
///
/// let policy = SandboxPolicy::builder()
///     .allow_actions(["get_scene_info"])
///     .build();
/// let mut ctx = SandboxContext::new(policy);
///
/// let result = ctx.execute("get_scene_info", &json!({}), None, None);
/// ```
pub struct SandboxContext {
    policy: SandboxPolicy,
    audit_log: AuditLog,
    action_count: u32,
    actor: Option<String>,
}

impl SandboxContext {
    /// Create a new context with the given policy.
    pub fn new(policy: SandboxPolicy) -> Self {
        Self {
            policy,
            audit_log: AuditLog::new(),
            action_count: 0,
            actor: None,
        }
    }

    /// Attach a caller identity to all subsequent audit entries.
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    /// Return a reference to the audit log.
    pub fn audit_log(&self) -> &AuditLog {
        &self.audit_log
    }

    /// Return a reference to the current policy.
    pub fn policy(&self) -> &SandboxPolicy {
        &self.policy
    }

    /// Return the number of actions executed in this session.
    pub fn action_count(&self) -> u32 {
        self.action_count
    }

    // ── Core execution ────────────────────────────────────────────────────────

    /// Execute `action` with `params` through the full sandbox pipeline:
    ///
    /// 1. Policy check (whitelist, deny list, read-only, action limit)
    /// 2. Optional input validation via `validator`
    /// 3. Optional timeout enforcement via `timeout_override`
    /// 4. Invoke `handler` if provided, or return a default empty result
    /// 5. Emit an [`AuditEntry`] regardless of outcome
    ///
    /// When `handler` is `None` the sandbox only performs policy+validation
    /// checks and returns an empty success result (useful for pre-flight
    /// checks in tests).
    pub fn execute(
        &mut self,
        action: &str,
        params: &Value,
        validator: Option<&InputValidator>,
        handler: Option<&ActionHandler>,
    ) -> Result<ExecutionResult, SandboxError> {
        let start = Instant::now();

        // ── 1. Action limit check ─────────────────────────────────────────────
        if let Some(max) = self.policy.max_actions {
            let attempted = self.action_count + 1;
            if attempted > max {
                let err = SandboxError::ActionLimitExceeded {
                    limit: max,
                    attempted,
                };
                self.emit_audit(
                    action,
                    params,
                    start.elapsed(),
                    AuditOutcome::Denied {
                        reason: err.to_string(),
                    },
                );
                return Err(err);
            }
        }

        // ── 2. Policy: action whitelist / deny list ───────────────────────────
        if let Err(e) = self.policy.check_action(action) {
            warn!(action, error = %e, "sandbox: action denied by policy");
            self.emit_audit(
                action,
                params,
                start.elapsed(),
                AuditOutcome::Denied {
                    reason: e.to_string(),
                },
            );
            return Err(e);
        }

        // ── 3. Input validation ───────────────────────────────────────────────
        if let Some(v) = validator {
            if let Err(e) = v.validate_value(params) {
                warn!(action, error = %e, "sandbox: input validation failed");
                self.emit_audit(
                    action,
                    params,
                    start.elapsed(),
                    AuditOutcome::Denied {
                        reason: e.to_string(),
                    },
                );
                return Err(e);
            }
        }

        // ── 4. Determine effective timeout ────────────────────────────────────
        let effective_timeout = self.policy.timeout_ms.map(Duration::from_millis);

        // ── 5. Invoke handler ─────────────────────────────────────────────────
        let result = if let Some(h) = handler {
            let params_map = match params {
                Value::Object(m) => m,
                _ => {
                    let e = SandboxError::ValidationFailed {
                        field: "<root>".to_owned(),
                        reason: "params must be a JSON object".to_owned(),
                    };
                    self.emit_audit(
                        action,
                        params,
                        start.elapsed(),
                        AuditOutcome::Denied {
                            reason: e.to_string(),
                        },
                    );
                    return Err(e);
                }
            };

            // Basic timeout: check elapsed time *before* calling the handler.
            // For true async timeout, callers should use tokio::time::timeout
            // around the whole execute() call.
            if let Some(timeout) = effective_timeout {
                if start.elapsed() >= timeout {
                    let err = SandboxError::Timeout {
                        timeout_ms: timeout.as_millis() as u64,
                    };
                    self.emit_audit(action, params, start.elapsed(), AuditOutcome::Timeout);
                    return Err(err);
                }
            }

            match h(params_map) {
                Ok(v) => v,
                Err(e) => {
                    self.emit_audit(
                        action,
                        params,
                        start.elapsed(),
                        AuditOutcome::Error {
                            message: e.to_string(),
                        },
                    );
                    return Err(e);
                }
            }
        } else {
            Value::Null
        };

        let duration = start.elapsed();
        self.action_count += 1;
        debug!(
            action,
            duration_ms = duration.as_millis(),
            "sandbox: action executed"
        );
        self.emit_audit(action, params, duration, AuditOutcome::Success);

        Ok(ExecutionResult {
            value: Some(result),
            duration,
        })
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn emit_audit(&self, action: &str, params: &Value, duration: Duration, outcome: AuditOutcome) {
        let params_json = serde_json::to_string(params).unwrap_or_else(|_| "{}".to_owned());
        let entry = AuditEntry::new(self.actor.clone(), action, params_json, duration, outcome);
        self.audit_log.record(entry);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ExecutionMode;
    use serde_json::json;

    fn echo_handler() -> ActionHandler {
        Box::new(|params| Ok(Value::Object(params.clone())))
    }

    fn error_handler() -> ActionHandler {
        Box::new(|_| Err(SandboxError::Internal("intentional test error".to_owned())))
    }

    mod test_happy_path {
        use super::*;

        #[test]
        fn allowed_action_succeeds() {
            let policy = SandboxPolicy::builder().allow_actions(["echo"]).build();
            let mut ctx = SandboxContext::new(policy);
            let result = ctx.execute("echo", &json!({"x": 1}), None, Some(&echo_handler()));
            assert!(result.is_ok());
            assert_eq!(ctx.action_count(), 1);
        }

        #[test]
        fn no_handler_returns_null_success() {
            let policy = SandboxPolicy::builder().build();
            let mut ctx = SandboxContext::new(policy);
            let result = ctx.execute("anything", &json!({}), None, None);
            assert!(result.is_ok());
            assert_eq!(result.unwrap().value, Some(Value::Null));
        }

        #[test]
        fn audit_log_records_success() {
            let policy = SandboxPolicy::builder().build();
            let mut ctx = SandboxContext::new(policy);
            ctx.execute("op", &json!({}), None, None).unwrap();
            assert_eq!(ctx.audit_log().len(), 1);
            assert!(ctx.audit_log().successes().len() == 1);
        }

        #[test]
        fn actor_is_attached_to_audit_entry() {
            let policy = SandboxPolicy::builder().build();
            let mut ctx = SandboxContext::new(policy).with_actor("agent-007");
            ctx.execute("op", &json!({}), None, None).unwrap();
            let entries = ctx.audit_log().entries();
            assert_eq!(entries[0].actor.as_deref(), Some("agent-007"));
        }
    }

    mod test_policy_enforcement {
        use super::*;

        #[test]
        fn denied_action_returns_error() {
            let policy = SandboxPolicy::builder().allow_actions(["safe_op"]).build();
            let mut ctx = SandboxContext::new(policy);
            let err = ctx
                .execute("dangerous_op", &json!({}), None, None)
                .unwrap_err();
            assert!(matches!(err, SandboxError::ActionNotAllowed { .. }));
            assert_eq!(ctx.audit_log().denials().len(), 1);
        }

        #[test]
        fn explicitly_denied_action_blocked() {
            let policy = SandboxPolicy::builder().deny_actions(["rm_all"]).build();
            let mut ctx = SandboxContext::new(policy);
            assert!(matches!(
                ctx.execute("rm_all", &json!({}), None, None),
                Err(SandboxError::ActionNotAllowed { .. })
            ));
        }

        #[test]
        fn action_limit_exceeded_returns_error() {
            let policy = SandboxPolicy::builder().max_actions(2).build();
            let mut ctx = SandboxContext::new(policy);
            assert!(ctx.execute("op", &json!({}), None, None).is_ok());
            assert!(ctx.execute("op", &json!({}), None, None).is_ok());
            // Third call should fail
            assert!(matches!(
                ctx.execute("op", &json!({}), None, None),
                Err(SandboxError::ActionLimitExceeded { .. })
            ));
        }

        #[test]
        fn read_only_mode_blocks_write_via_policy() {
            let policy = SandboxPolicy::builder()
                .mode(ExecutionMode::ReadOnly)
                .build();
            // check_write is a separate helper; verify via policy directly
            assert!(matches!(
                policy.check_write("create_mesh"),
                Err(SandboxError::ReadOnlyViolation { .. })
            ));
        }
    }

    mod test_handler_errors {
        use super::*;

        #[test]
        fn handler_error_is_recorded_in_audit_log() {
            let policy = SandboxPolicy::builder().build();
            let mut ctx = SandboxContext::new(policy);
            let result = ctx.execute("op", &json!({}), None, Some(&error_handler()));
            assert!(result.is_err());
            let entries = ctx.audit_log().entries();
            assert_eq!(entries.len(), 1);
            assert!(matches!(entries[0].outcome, AuditOutcome::Error { .. }));
        }

        #[test]
        fn action_count_not_incremented_on_error() {
            let policy = SandboxPolicy::builder().build();
            let mut ctx = SandboxContext::new(policy);
            let _ = ctx.execute("op", &json!({}), None, Some(&error_handler()));
            assert_eq!(ctx.action_count(), 0);
        }
    }

    mod test_validation_integration {
        use super::*;
        use crate::validator::{FieldSchema, InputValidator, ValidationRule};

        #[test]
        fn validation_failure_blocks_execution() {
            let policy = SandboxPolicy::builder().build();
            let mut ctx = SandboxContext::new(policy);
            let validator = InputValidator::new()
                .register("name", FieldSchema::new().rule(ValidationRule::Required));
            // Missing required "name" field
            let result = ctx.execute("op", &json!({"other": 1}), Some(&validator), None);
            assert!(matches!(result, Err(SandboxError::ValidationFailed { .. })));
        }

        #[test]
        fn valid_params_proceed_to_handler() {
            let policy = SandboxPolicy::builder().build();
            let mut ctx = SandboxContext::new(policy);
            let validator = InputValidator::new().register(
                "name",
                FieldSchema::new()
                    .rule(ValidationRule::Required)
                    .rule(ValidationRule::IsString),
            );
            let result = ctx.execute(
                "op",
                &json!({"name": "sphere"}),
                Some(&validator),
                Some(&echo_handler()),
            );
            assert!(result.is_ok());
        }
    }
}
