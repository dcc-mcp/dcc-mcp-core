//! Gateway authentication and endpoint-scope authorization (#1365).
//!
//! The gateway stays open by default for existing localhost-only deployments.
//! As soon as an operator configures a bearer API key or JWT secret, protected
//! REST endpoints require `Authorization: Bearer ...` and enforce action/DCC
//! scope before registration or dispatch mutates gateway state.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

/// Gateway action scopes carried by JWT claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GatewayAuthScope {
    Register,
    Call,
    ReadResources,
    Admin,
}

impl GatewayAuthScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Call => "call",
            Self::ReadResources => "read_resources",
            Self::Admin => "admin",
        }
    }
}

/// Runtime gateway security knobs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewaySecurityConfig {
    /// Static bearer API keys. These mirror the Python `ApiKeyConfig` helper:
    /// matching `Authorization: Bearer <key>` grants all gateway scopes.
    pub api_keys: Vec<String>,
    /// HS256 JWT secret. JWT claims carry `allowed_dcc` and `scopes`.
    pub jwt_secret: Option<String>,
}

impl GatewaySecurityConfig {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn with_api_keys(keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            api_keys: clean_keys(keys),
            jwt_secret: None,
        }
    }

    pub fn with_jwt_secret(secret: impl Into<String>) -> Self {
        Self {
            api_keys: Vec::new(),
            jwt_secret: Some(secret.into()),
        }
    }

    pub fn from_env() -> Self {
        let mut keys = Vec::new();
        if let Ok(value) = std::env::var("DCC_MCP_GATEWAY_API_KEY") {
            keys.push(value);
        }
        if let Ok(value) = std::env::var("DCC_MCP_GATEWAY_API_KEYS") {
            keys.extend(value.split(',').map(str::to_string));
        }
        Self {
            api_keys: clean_keys(keys),
            jwt_secret: std::env::var("DCC_MCP_GATEWAY_JWT_SECRET")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        !self.api_keys.is_empty() || self.jwt_secret.is_some()
    }
}

fn clean_keys(keys: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    keys.into_iter()
        .map(Into::into)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

/// JWT claims accepted by the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayTokenClaims {
    pub sub: String,
    pub iat: Option<u64>,
    pub exp: u64,
    #[serde(default)]
    pub iss: Option<String>,
    #[serde(default)]
    pub allowed_dcc: Vec<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl GatewayTokenClaims {
    pub fn new(
        sub: impl Into<String>,
        exp: u64,
        allowed_dcc: Vec<String>,
        scopes: Vec<String>,
    ) -> Self {
        Self {
            sub: sub.into(),
            iat: Some(now_secs()),
            exp,
            iss: None,
            allowed_dcc,
            scopes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayPrincipal {
    pub subject: String,
    pub allowed_dcc: Vec<String>,
    pub scopes: HashSet<String>,
    pub token_kind: &'static str,
}

impl GatewayPrincipal {
    fn api_key() -> Self {
        Self {
            subject: "apikey".to_string(),
            allowed_dcc: vec!["*".to_string()],
            scopes: ["register", "call", "read_resources", "admin"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            token_kind: "api-key",
        }
    }

    fn from_claims(claims: GatewayTokenClaims) -> Self {
        Self {
            subject: claims.sub,
            allowed_dcc: claims.allowed_dcc,
            scopes: claims
                .scopes
                .into_iter()
                .map(|scope| scope.trim().to_ascii_lowercase())
                .filter(|scope| !scope.is_empty())
                .collect(),
            token_kind: "jwt",
        }
    }
}

/// Immutable request-time security policy.
#[derive(Debug, Clone)]
pub struct GatewaySecurityPolicy {
    config: GatewaySecurityConfig,
}

impl Default for GatewaySecurityPolicy {
    fn default() -> Self {
        Self::disabled()
    }
}

impl GatewaySecurityPolicy {
    pub fn new(config: GatewaySecurityConfig) -> Self {
        Self { config }
    }

    pub fn disabled() -> Self {
        Self {
            config: GatewaySecurityConfig::disabled(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(GatewaySecurityConfig::from_env())
    }

    pub fn is_enabled(&self) -> bool {
        self.config.is_enabled()
    }

    pub fn authorize(
        &self,
        headers: &HeaderMap,
        scope: GatewayAuthScope,
        dcc_type: Option<&str>,
    ) -> Result<GatewayPrincipal, GatewayAuthError> {
        if !self.is_enabled() {
            return Ok(GatewayPrincipal {
                subject: "anonymous".to_string(),
                allowed_dcc: vec!["*".to_string()],
                scopes: HashSet::new(),
                token_kind: "disabled",
            });
        }

        let token = bearer_token(headers)?;
        if self
            .config
            .api_keys
            .iter()
            .any(|accepted| constant_time_eq(token.as_bytes(), accepted.as_bytes()))
        {
            return Ok(GatewayPrincipal::api_key());
        }

        let Some(secret) = self.config.jwt_secret.as_deref() else {
            return Err(GatewayAuthError::unauthorized("invalid bearer token"));
        };
        let claims = decode_claims(token, secret)?;
        let principal = GatewayPrincipal::from_claims(claims);
        self.enforce_scope(&principal, scope)?;
        self.enforce_dcc(&principal, dcc_type)?;
        Ok(principal)
    }

    fn enforce_scope(
        &self,
        principal: &GatewayPrincipal,
        scope: GatewayAuthScope,
    ) -> Result<(), GatewayAuthError> {
        if principal.scopes.contains(scope.as_str()) || principal.scopes.contains("*") {
            return Ok(());
        }
        Err(GatewayAuthError::forbidden(
            "scope-denied",
            format!(
                "token subject '{}' lacks required gateway scope '{}'",
                principal.subject,
                scope.as_str()
            ),
        )
        .with_scope(scope))
    }

    fn enforce_dcc(
        &self,
        principal: &GatewayPrincipal,
        dcc_type: Option<&str>,
    ) -> Result<(), GatewayAuthError> {
        let Some(dcc_type) = dcc_type.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(());
        };
        if principal.allowed_dcc.is_empty()
            || principal.allowed_dcc.iter().any(|allowed| {
                let allowed = allowed.trim();
                allowed == "*" || allowed.eq_ignore_ascii_case(dcc_type)
            })
        {
            return Ok(());
        }
        Err(GatewayAuthError::forbidden(
            "dcc-scope-denied",
            format!(
                "token subject '{}' is not allowed to access DCC '{}'",
                principal.subject, dcc_type
            ),
        )
        .with_dcc(dcc_type))
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{message}")]
pub struct GatewayAuthError {
    pub status: StatusCode,
    pub error: &'static str,
    pub kind: &'static str,
    pub message: String,
    pub required_scope: Option<&'static str>,
    pub dcc_type: Option<String>,
}

impl GatewayAuthError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            error: "unauthorized",
            kind: "unauthorized",
            message: message.into(),
            required_scope: None,
            dcc_type: None,
        }
    }

    fn forbidden(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            error: "forbidden",
            kind,
            message: message.into(),
            required_scope: None,
            dcc_type: None,
        }
    }

    fn with_scope(mut self, scope: GatewayAuthScope) -> Self {
        self.required_scope = Some(scope.as_str());
        self
    }

    fn with_dcc(mut self, dcc_type: impl Into<String>) -> Self {
        self.dcc_type = Some(dcc_type.into());
        self
    }

    pub fn response(&self) -> Response {
        let mut body = json!({
            "ok": false,
            "success": false,
            "error": self.error,
            "message": self.message,
            "error_detail": {
                "kind": self.kind,
                "message": self.message,
            }
        });
        if let Some(scope) = self.required_scope {
            body["error_detail"]["required_scope"] = json!(scope);
        }
        if let Some(dcc_type) = self.dcc_type.as_deref() {
            body["error_detail"]["dcc_type"] = json!(dcc_type);
        }
        let mut response = (self.status, axum::Json(body)).into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer"),
            );
        }
        response
    }
}

pub fn auth_error_value(err: &GatewayAuthError) -> Value {
    json!({
        "error": err.error,
        "kind": err.kind,
        "message": err.message,
        "required_scope": err.required_scope,
        "dcc_type": err.dcc_type,
    })
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, GatewayAuthError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GatewayAuthError::unauthorized("missing Authorization header"))?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            GatewayAuthError::unauthorized("Authorization header must use the Bearer scheme")
        })
}

fn decode_claims(token: &str, secret: &str) -> Result<GatewayTokenClaims, GatewayAuthError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 60;
    validation.validate_exp = true;
    validation.required_spec_claims.clear();
    validation.required_spec_claims.insert("exp".into());
    decode::<GatewayTokenClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|_| GatewayAuthError::unauthorized("invalid bearer token"))
}

#[cfg(test)]
pub(crate) fn issue_gateway_token(
    claims: &GatewayTokenClaims,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
}

#[cfg(feature = "admin")]
pub async fn admin_auth_middleware(
    axum::extract::State(state): axum::extract::State<crate::gateway::admin::state::AdminState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    match state
        .gateway
        .security
        .authorize(&headers, GatewayAuthScope::Admin, None)
    {
        Ok(_) => next.run(request).await,
        Err(err) => err.response(),
    }
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, value.parse().unwrap());
        headers
    }

    #[test]
    fn disabled_policy_allows_requests() {
        let policy = GatewaySecurityPolicy::disabled();
        assert!(
            policy
                .authorize(&HeaderMap::new(), GatewayAuthScope::Call, Some("maya"))
                .is_ok()
        );
    }

    #[test]
    fn api_key_policy_accepts_matching_bearer() {
        let policy =
            GatewaySecurityPolicy::new(GatewaySecurityConfig::with_api_keys(["secret-token"]));
        let principal = policy
            .authorize(
                &headers("Bearer secret-token"),
                GatewayAuthScope::Register,
                Some("maya"),
            )
            .unwrap();
        assert_eq!(principal.token_kind, "api-key");
    }

    #[test]
    fn jwt_policy_enforces_scope_and_dcc() {
        let secret = "gateway-secret-for-tests";
        let claims = GatewayTokenClaims::new(
            "artist-1",
            now_secs() + 3600,
            vec!["maya".to_string()],
            vec!["register".to_string()],
        );
        let token = issue_gateway_token(&claims, secret).unwrap();
        let policy = GatewaySecurityPolicy::new(GatewaySecurityConfig::with_jwt_secret(secret));

        assert!(
            policy
                .authorize(
                    &headers(&format!("Bearer {token}")),
                    GatewayAuthScope::Register,
                    Some("maya"),
                )
                .is_ok()
        );
        assert_eq!(
            policy
                .authorize(
                    &headers(&format!("Bearer {token}")),
                    GatewayAuthScope::Call,
                    Some("maya"),
                )
                .unwrap_err()
                .kind,
            "scope-denied"
        );
        assert_eq!(
            policy
                .authorize(
                    &headers(&format!("Bearer {token}")),
                    GatewayAuthScope::Register,
                    Some("houdini"),
                )
                .unwrap_err()
                .kind,
            "dcc-scope-denied"
        );
    }
}
