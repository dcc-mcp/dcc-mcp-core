//! Structured error envelope shared by every REST endpoint.
//!
//! Keeps the wire shape identical across handlers so clients only need
//! to pattern-match once on [`ServiceErrorKind`]. The `hint` field is
//! deliberately actionable ("load the skill first"), not a stack trace —
//! it is surfaced straight to end users.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Machine-readable error class. Kebab-case so new variants can be
/// added without breaking existing clients that ignore unknown kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceErrorKind {
    /// Requested tool slug is not registered on this DCC instance.
    UnknownSlug,
    /// Tool slug matches multiple actions — caller must disambiguate.
    Ambiguous,
    /// The owning skill has not been loaded. Agent should call
    /// `load_skill` first.
    SkillNotLoaded,
    /// Input parameters failed JSON Schema validation.
    InvalidParams,
    /// Auth gate rejected the request.
    Unauthorized,
    /// Request body could not be parsed.
    BadRequest,
    /// The handler itself returned an error.
    BackendError,
    /// Execution affinity violated — e.g. a main-thread-only tool was
    /// called without an executor available.
    AffinityViolation,
    /// The underlying DCC or dispatcher is not ready yet.
    NotReady,
    /// Catch-all for unexpected server failures.
    Internal,
}

impl ServiceErrorKind {
    /// HTTP status that best matches the error class. Kept conservative
    /// so we never claim "500" for client-side problems.
    #[must_use]
    pub fn http_status(self) -> u16 {
        match self {
            Self::UnknownSlug => 404,
            Self::Ambiguous => 409,
            Self::SkillNotLoaded => 409,
            Self::InvalidParams => 400,
            Self::Unauthorized => 401,
            Self::BadRequest => 400,
            Self::AffinityViolation => 409,
            Self::NotReady => 503,
            Self::BackendError => 502,
            Self::Internal => 500,
        }
    }
}

/// Single wire representation of a failed REST call.
///
/// Intentionally narrow: adding new fields is allowed, but the existing
/// three — `kind` / `message` / `hint` — are a stable contract.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServiceError {
    /// Machine-readable discriminator.
    pub kind: ServiceErrorKind,
    /// Human-readable description.
    pub message: String,
    /// Optional remediation hint. Displayed verbatim to end users.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Request id for correlation with audit logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Candidate slugs for `ambiguous` errors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
}

impl ServiceError {
    /// Short-hand constructor.
    pub fn new(kind: ServiceErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            hint: None,
            request_id: None,
            candidates: Vec::new(),
        }
    }

    /// Attach a remediation hint.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Attach a request id (usually the `X-Request-Id` header value).
    #[must_use]
    pub fn with_request_id(mut self, rid: impl Into<String>) -> Self {
        self.request_id = Some(rid.into());
        self
    }

    /// Attach disambiguation candidates.
    #[must_use]
    pub fn with_candidates(mut self, candidates: Vec<String>) -> Self {
        self.candidates = candidates;
        self
    }

    /// Matching HTTP status.
    #[must_use]
    pub fn http_status(&self) -> u16 {
        self.kind.http_status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_map_is_complete() {
        for kind in [
            ServiceErrorKind::UnknownSlug,
            ServiceErrorKind::Ambiguous,
            ServiceErrorKind::SkillNotLoaded,
            ServiceErrorKind::InvalidParams,
            ServiceErrorKind::Unauthorized,
            ServiceErrorKind::BadRequest,
            ServiceErrorKind::BackendError,
            ServiceErrorKind::AffinityViolation,
            ServiceErrorKind::NotReady,
            ServiceErrorKind::Internal,
        ] {
            // Every variant must produce a valid 4xx/5xx status.
            let code = kind.http_status();
            assert!(
                (400..=599).contains(&code),
                "kind {kind:?} produced non-error HTTP status {code}"
            );
        }
    }

    #[test]
    fn serialized_kind_is_kebab_case() {
        let err = ServiceError::new(ServiceErrorKind::SkillNotLoaded, "x");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["kind"], "skill-not-loaded");
    }

    #[test]
    fn hint_and_candidates_are_omitted_when_empty() {
        let err = ServiceError::new(ServiceErrorKind::Internal, "boom");
        let v = serde_json::to_value(&err).unwrap();
        assert!(v.get("hint").is_none());
        assert!(v.get("candidates").is_none());
        assert!(v.get("request_id").is_none());
    }

    #[test]
    fn builder_round_trip() {
        let err = ServiceError::new(ServiceErrorKind::Ambiguous, "multiple hits")
            .with_hint("pass dcc=maya")
            .with_request_id("req-1")
            .with_candidates(vec!["maya.spheres.create".into()]);
        assert_eq!(err.kind, ServiceErrorKind::Ambiguous);
        assert_eq!(err.hint.as_deref(), Some("pass dcc=maya"));
        assert_eq!(err.request_id.as_deref(), Some("req-1"));
        assert_eq!(err.candidates.len(), 1);
    }
}
