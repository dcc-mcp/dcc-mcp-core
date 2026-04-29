//! Per-step execution policies: timeout, retry backoff, idempotency keys.
//!
//! This module lands the **types + parser + helpers** that the upcoming
//! workflow executor (#348) will consume to enforce:
//!
//! - **Timeout** — absolute wall-clock deadline per step attempt.
//! - **Retry** — bounded re-execution with fixed / linear / exponential
//!   backoff, jitter, and optional error-kind filter.
//! - **Idempotency** — a templated key whose rendered value lets the
//!   executor short-circuit a step when a prior successful run under the
//!   same key exists.
//!
//! Runtime enforcement is **not** performed here — this module only
//! parses, validates, and exposes helpers (`RetryPolicy::next_delay`) so
//! that the executor PR can plug in without reshaping the schema.
//!
//! See issue [#353](https://github.com/loonghao/dcc-mcp-core/issues/353).

use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::ValidationError;

// ── Public enums ─────────────────────────────────────────────────────────

/// Backoff shape used between retry attempts.
///
/// All variants are bounded by [`RetryPolicy::max_delay`] and modulated by
/// [`RetryPolicy::jitter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackoffKind {
    /// Constant `initial_delay` between attempts.
    Fixed,
    /// `initial_delay * attempt_number` (1-indexed).
    Linear,
    /// `initial_delay * 2^(attempt_number - 1)` (1-indexed).
    #[default]
    Exponential,
}

impl BackoffKind {
    /// Lowercase string form used in serde + Python bindings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Linear => "linear",
            Self::Exponential => "exponential",
        }
    }
}

/// How idempotency keys are scoped when the executor consults the
/// JobManager for a prior successful run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IdempotencyScope {
    /// Keys are unique within a single workflow invocation (default).
    #[default]
    Workflow,
    /// Keys are globally unique across all workflow invocations.
    Global,
}

impl IdempotencyScope {
    /// Lowercase string form used in serde + Python bindings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Global => "global",
        }
    }
}

// ── RetryPolicy ──────────────────────────────────────────────────────────

/// Retry policy applied by the executor when a step attempt fails.
///
/// Construct this from YAML via [`RawStepPolicy::into_policy`].
///
/// # Invariants (checked by the parser)
///
/// - `max_attempts >= 1` (1 = no retry).
/// - `initial_delay <= max_delay`.
/// - `jitter` is clamped to `[0.0, 1.0]` (out-of-range values produce a
///   warning on parse and are silently clamped).
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    /// Hard cap on the number of **attempts** (not retries). `1` means
    /// the step runs exactly once.
    pub max_attempts: u32,
    /// Shape of the inter-attempt delay.
    pub backoff: BackoffKind,
    /// Base delay for the first retry.
    pub initial_delay: Duration,
    /// Upper bound the computed delay is clamped to.
    pub max_delay: Duration,
    /// Relative jitter in `[0.0, 1.0]`. The executor applies
    /// `delay * (1 + rand(-jitter, +jitter))` at call-site.
    pub jitter: f32,
    /// Optional filter over error kinds. `None` = every error is
    /// retryable. `Some(vec![])` = nothing is retryable.
    pub retry_on: Option<Vec<String>>,
}

impl RetryPolicy {
    /// Compute the **base** backoff for the given 1-indexed attempt
    /// number, *before* jitter is applied.
    ///
    /// - `attempt_number == 1` — returns `Duration::ZERO` (no pre-delay
    ///   for the very first attempt).
    /// - `attempt_number >= 2` — returns the shaped delay, clamped to
    ///   [`Self::max_delay`].
    ///
    /// The executor multiplies this value by `1 + rand(-jitter, +jitter)`
    /// at call-site; keeping this function deterministic makes it
    /// trivially unit-testable.
    #[must_use]
    pub fn next_delay(&self, attempt_number: u32) -> Duration {
        if attempt_number <= 1 {
            return Duration::ZERO;
        }
        let n = attempt_number - 1;
        let initial_ms = self.initial_delay.as_millis() as u64;
        let raw_ms: u64 = match self.backoff {
            BackoffKind::Fixed => initial_ms,
            BackoffKind::Linear => initial_ms.saturating_mul(u64::from(n)),
            BackoffKind::Exponential => {
                // 1st retry (attempt_number == 2) → initial * 2^0 = initial.
                // Saturate shift to avoid u64 overflow on large attempt counts.
                let shift = (n - 1).min(63);
                initial_ms.saturating_mul(1u64 << shift)
            }
        };
        let max_ms = self.max_delay.as_millis() as u64;
        Duration::from_millis(raw_ms.min(max_ms))
    }

    /// Whether this policy considers `error_kind` retryable.
    #[must_use]
    pub fn is_retryable(&self, error_kind: &str) -> bool {
        match &self.retry_on {
            None => true,
            Some(allow) => allow.iter().any(|k| k == error_kind),
        }
    }
}

// ── StepPolicy ───────────────────────────────────────────────────────────

/// Composite per-step execution policy.
///
/// Attached to a [`crate::Step`] via [`crate::Step::policy`]; defaults to
/// [`StepPolicy::default`] when the YAML omits every knob.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepPolicy {
    /// Absolute wall-clock timeout for **one** attempt. `None` = no
    /// timeout.
    pub timeout: Option<Duration>,
    /// Retry policy. `None` = single attempt, no retry.
    pub retry: Option<RetryPolicy>,
    /// Templated idempotency key. Rendered by the executor against the
    /// step context before consulting the JobManager.
    pub idempotency_key: Option<String>,
    /// Scope for the rendered idempotency key.
    pub idempotency_scope: IdempotencyScope,
    /// Optional time-to-live for cached entries, in seconds. `None` (or
    /// `Some(0)`) means the cached entry lives until its scope is
    /// purged. Persistent backends honour this via `expires_at`; the
    /// in-memory cache treats it as an `Instant + Duration` deadline.
    pub idempotency_ttl_secs: Option<u64>,
}

impl StepPolicy {
    /// Whether every knob is at its default (no timeout, no retry, no
    /// key). Cheap way for the executor to skip the policy path.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.timeout.is_none()
            && self.retry.is_none()
            && self.idempotency_key.is_none()
            && matches!(self.idempotency_scope, IdempotencyScope::Workflow)
            && self.idempotency_ttl_secs.is_none()
    }
}

// ── Raw / YAML-shaped mirrors ────────────────────────────────────────────

/// Raw YAML shape of the `retry:` block before validation/normalisation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawRetryPolicy {
    /// See [`RetryPolicy::max_attempts`]. Required.
    pub max_attempts: u32,
    /// See [`RetryPolicy::backoff`]. Defaults to `exponential`.
    #[serde(default)]
    pub backoff: Option<BackoffKind>,
    /// See [`RetryPolicy::initial_delay`]. Milliseconds.
    #[serde(default)]
    pub initial_delay_ms: Option<u64>,
    /// See [`RetryPolicy::max_delay`]. Milliseconds.
    #[serde(default)]
    pub max_delay_ms: Option<u64>,
    /// See [`RetryPolicy::jitter`]. Relative, clamped on parse.
    #[serde(default)]
    pub jitter: Option<f32>,
    /// See [`RetryPolicy::retry_on`]. Error-kind allowlist.
    #[serde(default)]
    pub retry_on: Option<Vec<String>>,
}

/// Raw YAML shape of the per-step policy block before validation.
///
/// Fields correspond 1:1 to the schema documented in `docs/guide/workflows.md`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawStepPolicy {
    /// See [`StepPolicy::timeout`]. Seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Raw retry block.
    #[serde(default)]
    pub retry: Option<RawRetryPolicy>,
    /// See [`StepPolicy::idempotency_key`]. Mustache-style template.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// See [`StepPolicy::idempotency_scope`]. Defaults to `workflow`.
    #[serde(default)]
    pub idempotency_scope: Option<IdempotencyScope>,
    /// See [`StepPolicy::idempotency_ttl_secs`]. Seconds.
    #[serde(default)]
    pub idempotency_ttl_secs: Option<u64>,
}

impl RawStepPolicy {
    /// Validate + normalise a raw policy attached to a step.
    ///
    /// `known_idents` is the set of identifiers the idempotency-key
    /// template is permitted to reference (typically workflow inputs +
    /// ids of prior steps).
    ///
    /// # Errors
    ///
    /// Returns the first [`ValidationError`] encountered. Emits a
    /// `tracing::warn!` if the raw jitter was out of range and had to be
    /// clamped.
    pub fn into_policy(
        self,
        step_id: &str,
        known_idents: &HashSet<String>,
    ) -> Result<StepPolicy, ValidationError> {
        // Timeout.
        let timeout = match self.timeout_secs {
            None => None,
            Some(0) => {
                return Err(ValidationError::InvalidPolicy {
                    step_id: step_id.to_string(),
                    reason: "timeout_secs must be > 0".to_string(),
                });
            }
            Some(n) => Some(Duration::from_secs(n)),
        };

        // Retry.
        let retry = match self.retry {
            None => None,
            Some(raw) => Some(raw.into_policy(step_id)?),
        };

        // Idempotency key — template reference check.
        if let Some(key) = &self.idempotency_key {
            check_template_refs(step_id, key, known_idents)?;
        }

        Ok(StepPolicy {
            timeout,
            retry,
            idempotency_key: self.idempotency_key,
            idempotency_scope: self.idempotency_scope.unwrap_or_default(),
            idempotency_ttl_secs: self.idempotency_ttl_secs.filter(|n| *n > 0),
        })
    }
}

impl RawRetryPolicy {
    /// Validate + normalise the raw retry block.
    ///
    /// # Errors
    ///
    /// - `max_attempts < 1`.
    /// - `initial_delay > max_delay`.
    pub fn into_policy(self, step_id: &str) -> Result<RetryPolicy, ValidationError> {
        if self.max_attempts < 1 {
            return Err(ValidationError::InvalidPolicy {
                step_id: step_id.to_string(),
                reason: format!(
                    "retry.max_attempts must be >= 1 (got {})",
                    self.max_attempts
                ),
            });
        }

        let backoff = self.backoff.unwrap_or_default();
        let initial_ms = self.initial_delay_ms.unwrap_or(500);
        let max_ms = self.max_delay_ms.unwrap_or(10_000);
        if initial_ms > max_ms {
            return Err(ValidationError::InvalidPolicy {
                step_id: step_id.to_string(),
                reason: format!(
                    "retry.initial_delay_ms ({initial_ms}) must be <= max_delay_ms ({max_ms})"
                ),
            });
        }

        // Jitter: clamp to [0, 1] with a warning on out-of-range input.
        let jitter = match self.jitter {
            None => 0.0_f32,
            Some(j) => {
                if !j.is_finite() || !(0.0..=1.0).contains(&j) {
                    tracing::warn!(
                        step_id = step_id,
                        jitter = j,
                        "retry.jitter out of range [0.0, 1.0]; clamping"
                    );
                    j.clamp(0.0, 1.0)
                } else {
                    j
                }
            }
        };

        Ok(RetryPolicy {
            max_attempts: self.max_attempts,
            backoff,
            initial_delay: Duration::from_millis(initial_ms),
            max_delay: Duration::from_millis(max_ms),
            jitter,
            retry_on: self.retry_on,
        })
    }
}

// ── Template reference check ─────────────────────────────────────────────

/// Extract `{{ ident }}` identifiers from a mustache-ish template.
///
/// Only **dotted identifier chains** (e.g. `{{inputs.scene_id}}` or
/// `{{steps.export.frame_range}}`) are recognised; anything with
/// whitespace or operators inside the braces is rejected as an invalid
/// template reference.
///
/// The returned iterator yields the **root** identifier of each
/// reference, which is the one that must be present in the
/// `known_idents` set at parse time.
fn extract_template_roots(template: &str) -> Result<Vec<String>, String> {
    let mut roots = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let after_open = &rest[open + 2..];
        let close = after_open
            .find("}}")
            .ok_or_else(|| format!("unterminated template reference in {template:?}"))?;
        let inner = after_open[..close].trim();
        if inner.is_empty() {
            return Err(format!("empty template reference in {template:?}"));
        }
        // The root is everything before the first '.' (if any). The full
        // inner segment must be a dotted identifier chain.
        for seg in inner.split('.') {
            if seg.is_empty()
                || !seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                || seg
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(true)
            {
                return Err(format!(
                    "invalid template reference {inner:?} in {template:?}"
                ));
            }
        }
        let root = inner.split('.').next().unwrap().to_string();
        roots.push(root);
        rest = &after_open[close + 2..];
    }
    Ok(roots)
}

/// Re-run the template reference check against a richer known-identifier
/// set. Called from [`crate::WorkflowSpec::validate`] after step-id
/// collection.
pub fn check_template_refs_pub(
    step_id: &str,
    template: &str,
    known: &HashSet<String>,
) -> Result<(), ValidationError> {
    check_template_refs(step_id, template, known)
}

fn check_template_refs(
    step_id: &str,
    template: &str,
    known: &HashSet<String>,
) -> Result<(), ValidationError> {
    let roots =
        extract_template_roots(template).map_err(|reason| ValidationError::InvalidPolicy {
            step_id: step_id.to_string(),
            reason,
        })?;
    for root in roots {
        if !known.contains(&root) {
            return Err(ValidationError::UnknownTemplateVar {
                step_id: step_id.to_string(),
                template: template.to_string(),
                var: root,
            });
        }
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn known(vars: &[&str]) -> HashSet<String> {
        vars.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn backoff_fixed_is_constant() {
        let p = RetryPolicy {
            max_attempts: 5,
            backoff: BackoffKind::Fixed,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_millis(10_000),
            jitter: 0.0,
            retry_on: None,
        };
        assert_eq!(p.next_delay(1), Duration::ZERO);
        assert_eq!(p.next_delay(2), Duration::from_millis(500));
        assert_eq!(p.next_delay(5), Duration::from_millis(500));
    }

    #[test]
    fn backoff_linear_scales_by_attempt() {
        let p = RetryPolicy {
            max_attempts: 5,
            backoff: BackoffKind::Linear,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_millis(10_000),
            jitter: 0.0,
            retry_on: None,
        };
        assert_eq!(p.next_delay(2), Duration::from_millis(500));
        assert_eq!(p.next_delay(3), Duration::from_millis(1_000));
        assert_eq!(p.next_delay(4), Duration::from_millis(1_500));
    }

    #[test]
    fn backoff_exponential_doubles() {
        let p = RetryPolicy {
            max_attempts: 6,
            backoff: BackoffKind::Exponential,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_millis(10_000),
            jitter: 0.0,
            retry_on: None,
        };
        assert_eq!(p.next_delay(2), Duration::from_millis(500));
        assert_eq!(p.next_delay(3), Duration::from_millis(1_000));
        assert_eq!(p.next_delay(4), Duration::from_millis(2_000));
        assert_eq!(p.next_delay(5), Duration::from_millis(4_000));
    }

    #[test]
    fn backoff_clamps_to_max_delay() {
        let p = RetryPolicy {
            max_attempts: 20,
            backoff: BackoffKind::Exponential,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_millis(3_000),
            jitter: 0.0,
            retry_on: None,
        };
        // 2^10 * 500 would be huge — should clamp.
        assert_eq!(p.next_delay(15), Duration::from_millis(3_000));
    }

    #[test]
    fn retry_on_filters_error_kinds() {
        let p = RetryPolicy {
            max_attempts: 3,
            backoff: BackoffKind::Fixed,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1_000),
            jitter: 0.0,
            retry_on: Some(vec!["timeout".into(), "transient".into()]),
        };
        assert!(p.is_retryable("timeout"));
        assert!(!p.is_retryable("permission_denied"));
    }

    #[test]
    fn retry_on_none_retries_everything() {
        let p = RetryPolicy {
            max_attempts: 3,
            backoff: BackoffKind::Fixed,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1_000),
            jitter: 0.0,
            retry_on: None,
        };
        assert!(p.is_retryable("anything"));
    }

    #[test]
    fn raw_retry_rejects_zero_max_attempts() {
        let raw = RawRetryPolicy {
            max_attempts: 0,
            ..Default::default()
        };
        assert!(matches!(
            raw.into_policy("s1"),
            Err(ValidationError::InvalidPolicy { .. })
        ));
    }

    #[test]
    fn raw_retry_rejects_inverted_delays() {
        let raw = RawRetryPolicy {
            max_attempts: 3,
            initial_delay_ms: Some(5_000),
            max_delay_ms: Some(1_000),
            ..Default::default()
        };
        assert!(matches!(
            raw.into_policy("s1"),
            Err(ValidationError::InvalidPolicy { .. })
        ));
    }

    #[test]
    fn raw_retry_clamps_jitter_out_of_range() {
        let raw = RawRetryPolicy {
            max_attempts: 3,
            jitter: Some(2.0),
            ..Default::default()
        };
        let p = raw.into_policy("s1").unwrap();
        assert_eq!(p.jitter, 1.0);

        let raw2 = RawRetryPolicy {
            max_attempts: 3,
            jitter: Some(-0.5),
            ..Default::default()
        };
        let p2 = raw2.into_policy("s1").unwrap();
        assert_eq!(p2.jitter, 0.0);
    }

    #[test]
    fn raw_step_policy_rejects_zero_timeout() {
        let raw = RawStepPolicy {
            timeout_secs: Some(0),
            ..Default::default()
        };
        assert!(matches!(
            raw.into_policy("s1", &known(&[])),
            Err(ValidationError::InvalidPolicy { .. })
        ));
    }

    #[test]
    fn raw_step_policy_parses_full_block() {
        let raw = RawStepPolicy {
            timeout_secs: Some(300),
            retry: Some(RawRetryPolicy {
                max_attempts: 3,
                backoff: Some(BackoffKind::Exponential),
                initial_delay_ms: Some(500),
                max_delay_ms: Some(10_000),
                jitter: Some(0.25),
                retry_on: Some(vec!["transient".into(), "timeout".into()]),
            }),
            idempotency_key: Some("export_{{scene_id}}_{{frame_range}}".into()),
            idempotency_scope: Some(IdempotencyScope::Global),
            idempotency_ttl_secs: Some(86_400),
        };
        let p = raw
            .into_policy("export_fbx", &known(&["scene_id", "frame_range"]))
            .unwrap();
        assert_eq!(p.timeout, Some(Duration::from_secs(300)));
        let r = p.retry.unwrap();
        assert_eq!(r.max_attempts, 3);
        assert_eq!(r.backoff, BackoffKind::Exponential);
        assert_eq!(r.jitter, 0.25);
        assert_eq!(p.idempotency_scope, IdempotencyScope::Global);
        assert_eq!(p.idempotency_ttl_secs, Some(86_400));
    }

    #[test]
    fn raw_step_policy_treats_zero_ttl_as_no_ttl() {
        let raw = RawStepPolicy {
            idempotency_ttl_secs: Some(0),
            ..Default::default()
        };
        let p = raw.into_policy("s1", &known(&[])).unwrap();
        assert_eq!(
            p.idempotency_ttl_secs, None,
            "Some(0) must collapse to None so adapters can plumb env vars without \
             accidentally creating already-expired entries"
        );
    }

    #[test]
    fn template_unknown_var_rejected() {
        let raw = RawStepPolicy {
            idempotency_key: Some("export_{{nope}}".into()),
            ..Default::default()
        };
        let err = raw
            .into_policy("export_fbx", &known(&["scene_id"]))
            .unwrap_err();
        assert!(matches!(err, ValidationError::UnknownTemplateVar { .. }));
    }

    #[test]
    fn template_malformed_rejected() {
        let raw = RawStepPolicy {
            idempotency_key: Some("bad_{{ 1nope }}".into()),
            ..Default::default()
        };
        assert!(matches!(
            raw.into_policy("s1", &known(&[])),
            Err(ValidationError::InvalidPolicy { .. })
        ));
    }

    #[test]
    fn template_dotted_chain_checks_root_only() {
        let raw = RawStepPolicy {
            idempotency_key: Some("k_{{inputs.scene.path}}".into()),
            ..Default::default()
        };
        let p = raw.into_policy("s1", &known(&["inputs"])).unwrap();
        assert!(p.idempotency_key.is_some());
    }

    #[test]
    fn step_policy_default_is_empty() {
        assert!(StepPolicy::default().is_empty());
    }

    #[test]
    fn backoff_kind_as_str_roundtrip() {
        for k in [
            BackoffKind::Fixed,
            BackoffKind::Linear,
            BackoffKind::Exponential,
        ] {
            let s = serde_yaml_ng::to_string(&k).unwrap();
            let parsed: BackoffKind = serde_yaml_ng::from_str(&s).unwrap();
            assert_eq!(k, parsed);
            assert!(!k.as_str().is_empty());
        }
    }
}
