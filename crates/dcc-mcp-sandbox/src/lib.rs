//! # dcc-mcp-sandbox
//!
//! Script execution sandbox with API whitelist, audit logging, and input
//! validation for the DCC-MCP ecosystem.
//!
//! ## Overview
//!
//! Enterprise users (game studios, VFX facilities) have strong security
//! requirements that vanilla Python-based DCC MCP integrations cannot
//! satisfy.  This crate provides:
//!
//! - **API whitelist / deny list** — restrict which DCC actions an Agent
//!   may invoke
//! - **Audit log** — tamper-evident, structured record of every action
//!   invocation (actor, params, outcome, duration)
//! - **Input validation** — schema-based validation of Agent-supplied
//!   parameters before they reach DCC code (injection prevention)
//! - **Read-only mode** — Agent can query but not mutate the scene
//! - **Action rate limiting** — cap the number of actions per session
//! - **Path allowlist** — restrict file-system access to project directories
//!
//! ## Quick start (Rust)
//!
//! ```rust
//! use dcc_mcp_sandbox::{SandboxContext, SandboxPolicy};
//! use serde_json::json;
//!
//! let policy = SandboxPolicy::builder()
//!     .allow_actions(["get_scene_info", "list_objects"])
//!     .timeout_ms(5_000)
//!     .build();
//!
//! let mut ctx = SandboxContext::new(policy).with_actor("my-agent");
//! ctx.execute("get_scene_info", &json!({}), None, None).unwrap();
//! println!("audit entries: {}", ctx.audit_log().len());
//! ```

pub mod audit;
pub mod context;
pub mod error;
pub mod policy;
pub mod validator;

#[cfg(feature = "python-bindings")]
pub mod python;

// ── Convenience re-exports ────────────────────────────────────────────────────

pub use audit::{AuditEntry, AuditLog, AuditOutcome};
pub use context::{ActionHandler, ExecutionResult, SandboxContext};
pub use error::SandboxError;
pub use policy::{ExecutionMode, SandboxPolicy, SandboxPolicyBuilder};
pub use validator::{FieldSchema, InputValidator, ValidationRule};

// ── Integration tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn full_sandbox_happy_path() {
        let policy = SandboxPolicy::builder()
            .allow_actions(["get_info", "list_objects"])
            .max_actions(10)
            .build();

        let mut ctx = SandboxContext::new(policy).with_actor("test-agent");

        // First action succeeds
        let r = ctx.execute("get_info", &json!({}), None, None);
        assert!(r.is_ok());

        // Second action succeeds
        let r = ctx.execute("list_objects", &json!({}), None, None);
        assert!(r.is_ok());

        assert_eq!(ctx.action_count(), 2);
        assert_eq!(ctx.audit_log().len(), 2);
        assert_eq!(ctx.audit_log().successes().len(), 2);
    }

    #[test]
    fn denied_action_audit_trail() {
        let policy = SandboxPolicy::builder().allow_actions(["safe"]).build();
        let mut ctx = SandboxContext::new(policy).with_actor("attacker");

        let _ = ctx.execute("rm_everything", &json!({}), None, None);
        assert_eq!(ctx.audit_log().denials().len(), 1);
        assert_eq!(ctx.audit_log().successes().len(), 0);
    }

    #[test]
    fn validation_blocks_injection() {
        use validator::{FieldSchema, InputValidator, ValidationRule};

        let policy = SandboxPolicy::builder().build();
        let mut ctx = SandboxContext::new(policy);

        let validator = InputValidator::new().register(
            "script",
            FieldSchema::new().rule(ValidationRule::IsString).rule(
                ValidationRule::ForbiddenSubstrings(vec![
                    "__import__".to_string(),
                    "exec(".to_string(),
                    "eval(".to_string(),
                ]),
            ),
        );

        let malicious = json!({"script": "__import__('os').system('rm -rf /')"});
        let result = ctx.execute("run_script", &malicious, Some(&validator), None);
        assert!(matches!(result, Err(SandboxError::ValidationFailed { .. })));
    }

    #[test]
    fn policy_serializes_to_json() {
        let policy = SandboxPolicy::builder()
            .allow_actions(["op"])
            .timeout_ms(1000)
            .build();
        let json = serde_json::to_string(&policy).expect("serialization failed");
        assert!(json.contains("op"));
    }
}
