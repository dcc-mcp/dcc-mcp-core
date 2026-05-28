//! Bearer-token authentication for the gateway HTTP registration plane
//! (#1365).
//!
//! This module implements the **minimum** viable authentication and
//! scope-enforcement layer required by epic #1367 to close the local-trust
//! gap once the gateway can be reached over the network. The contract is
//! intentionally small:
//!
//! 1. **No auth by default.** [`GatewayAuth::disabled()`] is the value used
//!    on `main`; every request is accepted exactly as before. Operators opt
//!    in by passing a populated [`GatewayAuth`] to the gateway runner.
//!
//! 2. **One header, one token.** When auth is enabled, callers must send
//!    `Authorization: Bearer <secret>`. The secret is matched against a
//!    static list of pre-shared tokens. No JWT, no OAuth dance — those
//!    plug in through `dcc-mcp-actions::AuditMiddleware`-style middleware
//!    once a richer identity story is needed.
//!
//! 3. **DCC scope is enforced at the token level.** Every token may
//!    declare an `allowed_dcc` set (e.g. `["maya", "blender"]`). On
//!    `POST /v1/instances/register` the gateway compares the incoming
//!    `dcc_type` against the token's set and rejects mismatches with a
//!    structured `unauthorized` envelope.
//!
//! Out-of-scope for this module (tracked in #1367 follow-ups):
//!
//! * In-binary TLS termination — operators run the gateway behind a
//!   reverse proxy that does TLS, mTLS, and rate limiting.
//! * Per-call scope (`call`, `read_resources`, `admin`). Today we only
//!   enforce `register` scope because that is the network boundary
//!   `gateway://instances` exposes.
//!
//! See `docs/guide/gateway.md` § Security and `tests/vrs/traces/core-1365
//! -gateway-auth-negative.jsonl` for the operator-facing contract.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A single pre-shared bearer token plus the DCC scope it is allowed to
/// register.
///
/// `allowed_dcc == None` means "this token can register any `dcc_type`",
/// useful for an operator that bootstraps a multi-DCC studio with a
/// single master token. `allowed_dcc = Some(set)` confines the token to
/// those DCC types and rejects anything else with a structured
/// `dcc_scope_mismatch` envelope.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayAuthToken {
    /// Bearer secret. Never logged in `Debug` form — `Display` is not
    /// implemented on purpose. Operators are responsible for keeping the
    /// values outside of process argv (use a config file or env var).
    pub token: String,
    /// Optional scope: `None` accepts any DCC, `Some(set)` confines the
    /// token to the listed DCC types (`"maya"`, `"blender"`, …).
    pub allowed_dcc: Option<BTreeSet<String>>,
    /// Optional opaque label surfaced in audit log entries — not used
    /// for matching. Defaults to the empty string.
    #[serde(default)]
    pub label: String,
}

impl GatewayAuthToken {
    /// Build a token that accepts any DCC.
    pub fn any_dcc(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            allowed_dcc: None,
            label: String::new(),
        }
    }

    /// Build a token confined to the given DCC types.
    pub fn for_dcc<I, S>(token: impl Into<String>, dccs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            token: token.into(),
            allowed_dcc: Some(dccs.into_iter().map(Into::into).collect()),
            label: String::new(),
        }
    }
}

/// Top-level auth configuration consumed by the gateway.
///
/// When [`GatewayAuth::is_enabled`] is `false` (the default), the
/// gateway behaves exactly as it did before #1365 — every request is
/// accepted. When `true`, callers must supply a matching bearer token
/// on every request the auth layer protects.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GatewayAuth {
    /// List of pre-shared tokens. Order is preserved but not significant
    /// — the first matching token wins.
    pub tokens: Vec<GatewayAuthToken>,
}

impl GatewayAuth {
    /// Disabled auth — every request is accepted. This is the default
    /// `main` behaviour and the value used by every test that does not
    /// specifically exercise auth.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Whether any token is configured. When `false`, callers should
    /// skip auth checks entirely.
    pub fn is_enabled(&self) -> bool {
        !self.tokens.is_empty()
    }

    /// Authorise a `POST /v1/instances/register` request.
    ///
    /// * `authorization_header` — the raw `Authorization` header value as
    ///   received by axum (or `None` if absent).
    /// * `dcc_type` — the `dcc_type` field from the registration body.
    ///
    /// Returns `Ok(())` when the request is allowed and an [`AuthError`]
    /// otherwise. Callers should map the error into the structured 401/
    /// 403 envelope expected by agents.
    pub fn authorize_register(
        &self,
        authorization_header: Option<&str>,
        dcc_type: &str,
    ) -> Result<(), AuthError> {
        if !self.is_enabled() {
            return Ok(());
        }
        let raw = authorization_header.ok_or(AuthError::MissingBearer)?;
        let presented = strip_bearer(raw).ok_or(AuthError::MalformedBearer)?;
        let token = self
            .tokens
            .iter()
            .find(|t| constant_time_eq(t.token.as_bytes(), presented.as_bytes()))
            .ok_or(AuthError::UnknownToken)?;
        if let Some(scope) = token.allowed_dcc.as_ref()
            && !scope.contains(dcc_type)
        {
            return Err(AuthError::DccScopeMismatch {
                presented_dcc: dcc_type.to_string(),
            });
        }
        Ok(())
    }
}

/// Structured authentication / authorisation failure.
///
/// The variants map 1:1 to the `error.kind` field of the JSON envelope
/// returned to agents; see [`AuthError::kind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// `Authorization` header absent on a request that requires it.
    MissingBearer,
    /// `Authorization` header present but not a `Bearer <token>` value.
    MalformedBearer,
    /// `Bearer` value did not match any configured token.
    UnknownToken,
    /// Token was recognised but the requested `dcc_type` is outside its
    /// `allowed_dcc` scope.
    DccScopeMismatch { presented_dcc: String },
}

impl AuthError {
    /// Stable `error.kind` slug for the JSON envelope.
    pub fn kind(&self) -> &'static str {
        match self {
            AuthError::MissingBearer | AuthError::MalformedBearer | AuthError::UnknownToken => {
                "unauthorized"
            }
            AuthError::DccScopeMismatch { .. } => "dcc_scope_mismatch",
        }
    }

    /// Human-readable message suitable for `error.message`.
    pub fn message(&self) -> String {
        match self {
            AuthError::MissingBearer => {
                "Authorization header is required for this endpoint.".to_string()
            }
            AuthError::MalformedBearer => {
                "Authorization header must be of the form 'Bearer <token>'.".to_string()
            }
            AuthError::UnknownToken => "Bearer token is not recognised.".to_string(),
            AuthError::DccScopeMismatch { presented_dcc } => {
                format!("Bearer token is not authorised to register dcc_type={presented_dcc}.")
            }
        }
    }

    /// HTTP status the envelope should ship under.
    pub fn http_status(&self) -> u16 {
        match self {
            AuthError::MissingBearer | AuthError::MalformedBearer | AuthError::UnknownToken => 401,
            AuthError::DccScopeMismatch { .. } => 403,
        }
    }
}

fn strip_bearer(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    let (scheme, rest) = trimmed.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let token = rest.trim();
    if token.is_empty() {
        return None;
    }
    Some(token)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
#[path = "security_tests.rs"]
mod security_tests;
