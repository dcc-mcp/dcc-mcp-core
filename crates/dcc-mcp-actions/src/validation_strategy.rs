//! Strategy trait + built-in validators for [`ActionDispatcher`] (#493).
//!
//! `dispatcher.dispatch()` previously interleaved handler lookup, the
//! `enabled` flag check, schema-emptiness branching, and validator
//! invocation in a single match. The validation half is now a single
//! [`ValidationStrategy::validate`] call; the dispatcher selects a
//! strategy per call (via [`select_strategy`]) instead of branching
//! inline.
//!
//! Adding a new validation flavour (e.g. cached compiled schemas, a
//! sandbox-policy precheck, a contract-test mode) means writing a new
//! `ValidationStrategy` impl — `dispatch()` is unaffected.

use serde_json::Value;

use crate::registry::ActionMeta;
use crate::validator::ActionValidator;

/// Outcome returned by [`ValidationStrategy::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationOutcome {
    /// `true` when no JSON Schema check ran (default placeholder
    /// schema, or no metadata available). Surfaced to the caller via
    /// [`crate::dispatcher::DispatchResult::validation_skipped`].
    pub skipped: bool,
}

impl ValidationOutcome {
    pub const RAN: Self = Self { skipped: false };
    pub const SKIPPED: Self = Self { skipped: true };
}

/// Pluggable validation step run before a handler is invoked.
///
/// Implementations are stateless or hold a borrowed handle to the
/// registered metadata; pick one per call via [`select_strategy`].
pub trait ValidationStrategy: Send + Sync {
    /// Validate `params` and return a [`ValidationOutcome`] on success
    /// or a human-readable message on failure (wired into
    /// `DispatchError::ValidationFailed`).
    fn validate(&self, params: &Value) -> Result<ValidationOutcome, String>;
}

/// No-op strategy used when the action has no metadata or its schema
/// carries no real constraints. Always reports `skipped = true`.
pub struct NoOpValidator;

impl ValidationStrategy for NoOpValidator {
    fn validate(&self, _params: &Value) -> Result<ValidationOutcome, String> {
        Ok(ValidationOutcome::SKIPPED)
    }
}

/// Borrowed-meta JSON Schema validator. Keeps the matcher cheap to
/// construct (no clone of the schema) so `dispatch()` can build it on
/// every call without measurable overhead.
pub struct SchemaValidator<'a> {
    meta: &'a ActionMeta,
}

impl<'a> SchemaValidator<'a> {
    pub fn new(meta: &'a ActionMeta) -> Self {
        Self { meta }
    }
}

impl ValidationStrategy for SchemaValidator<'_> {
    fn validate(&self, params: &Value) -> Result<ValidationOutcome, String> {
        let validator = ActionValidator::new(self.meta);
        let result = validator.validate_input(params);
        if result.is_valid() {
            Ok(ValidationOutcome::RAN)
        } else {
            Err(result
                .into_result()
                .err()
                .unwrap_or_else(|| "validation failed".to_string()))
        }
    }
}

/// Pick the right strategy for `meta`, honouring the dispatcher's
/// `skip_empty_schema_validation` flag.
///
/// The result is a boxed `dyn ValidationStrategy` so the dispatcher can
/// keep its body to one line (`strategy.validate(&params)?`) regardless
/// of which flavour was selected.
pub fn select_strategy<'a>(
    meta: Option<&'a ActionMeta>,
    skip_empty_schema_validation: bool,
) -> Box<dyn ValidationStrategy + 'a> {
    let Some(meta) = meta else {
        return Box::new(NoOpValidator);
    };
    let schema = &meta.input_schema;
    let is_empty = schema.is_null()
        || schema.as_object().map(|o| o.is_empty()).unwrap_or(false)
        || crate::dispatcher::is_default_schema(schema);
    if is_empty && skip_empty_schema_validation {
        Box::new(NoOpValidator)
    } else {
        Box::new(SchemaValidator::new(meta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta_with_schema(schema: Value) -> ActionMeta {
        ActionMeta {
            name: "x".into(),
            dcc: "maya".into(),
            input_schema: schema,
            ..Default::default()
        }
    }

    #[test]
    fn noop_strategy_skips() {
        let v = NoOpValidator;
        let out = v.validate(&json!({})).unwrap();
        assert!(out.skipped);
    }

    #[test]
    fn schema_strategy_passes_valid_input() {
        let meta = meta_with_schema(json!({
            "type": "object",
            "required": ["radius"],
            "properties": {"radius": {"type": "number"}}
        }));
        let v = SchemaValidator::new(&meta);
        let out = v.validate(&json!({"radius": 1.0})).unwrap();
        assert!(!out.skipped);
    }

    #[test]
    fn schema_strategy_rejects_bad_input() {
        let meta = meta_with_schema(json!({
            "type": "object",
            "required": ["radius"],
            "properties": {"radius": {"type": "number"}}
        }));
        let v = SchemaValidator::new(&meta);
        let err = v.validate(&json!({})).unwrap_err();
        assert!(!err.is_empty(), "expected validation error message");
    }

    #[test]
    fn select_strategy_picks_noop_for_missing_meta() {
        let strat = select_strategy(None, true);
        assert!(strat.validate(&json!({})).unwrap().skipped);
    }

    #[test]
    fn select_strategy_picks_noop_for_empty_schema_when_flag_set() {
        let meta = meta_with_schema(json!({}));
        let strat = select_strategy(Some(&meta), true);
        assert!(strat.validate(&json!({"x": 1})).unwrap().skipped);
    }

    #[test]
    fn select_strategy_picks_schema_when_flag_unset() {
        let meta = meta_with_schema(json!({}));
        let strat = select_strategy(Some(&meta), false);
        // Empty schema with no required fields => still passes, but it
        // ran the validator (skipped should be false).
        assert!(!strat.validate(&json!({})).unwrap().skipped);
    }
}
