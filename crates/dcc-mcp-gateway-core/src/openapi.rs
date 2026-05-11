//! Pure OpenAPI gateway value types (issue #845).
//!
//! The gateway runtime owns HTTP dispatch and spec parsing. The credential
//! contract itself is a small value surface that can live in the domain crate so
//! callers can construct OpenAPI mounts without depending on the gateway runtime.

use std::fmt;

/// How credentials are transmitted to the backend REST service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthKind {
    /// `Authorization: Bearer <token>` header.
    Bearer,
    /// Arbitrary header name (e.g. `X-API-Key`).
    ApiKey,
    /// `Authorization: Basic base64(<user>:<pass>)` header.
    Basic,
}

impl fmt::Display for AuthKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer => write!(f, "Bearer"),
            Self::ApiKey => write!(f, "ApiKey"),
            Self::Basic => write!(f, "Basic"),
        }
    }
}

/// Credential bundle forwarded to the backend REST service on every call.
///
/// When [`AuthConfig::value`] starts with `$`, the remainder is treated as an
/// environment-variable name; the actual secret is resolved at call time so it
/// is never stored in memory longer than necessary.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// How the credential is transported.
    pub kind: AuthKind,
    /// Token, key, or `$ENV_VAR` reference.
    pub value: String,
    /// Header name for `ApiKey`; ignored for `Bearer` and `Basic`
    /// (which always use `Authorization`).
    pub header: String,
}

impl AuthConfig {
    /// Construct a `Bearer` token auth config.
    ///
    /// `value` may be a literal token or `"$ENV_VAR"` to resolve at call time.
    #[must_use]
    pub fn bearer(value: impl Into<String>) -> Self {
        Self {
            kind: AuthKind::Bearer,
            value: value.into(),
            header: "Authorization".to_string(),
        }
    }

    /// Construct an API-key auth config with a custom header name.
    ///
    /// `value` may be a literal key or `"$ENV_VAR"` to resolve at call time.
    #[must_use]
    pub fn api_key(header: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            kind: AuthKind::ApiKey,
            value: value.into(),
            header: header.into(),
        }
    }

    /// Construct a `Basic` auth config.
    ///
    /// `value` should be `"<user>:<pass>"` or `"$ENV_VAR"` where the env-var
    /// expands to `"<user>:<pass>"`. Base64 encoding is applied at call time.
    #[must_use]
    pub fn basic(value: impl Into<String>) -> Self {
        Self {
            kind: AuthKind::Basic,
            value: value.into(),
            header: "Authorization".to_string(),
        }
    }

    /// Resolve the credential value, expanding `$ENV_VAR` references.
    ///
    /// Returns `None` when the env-var is not set; the caller should treat this
    /// as a configuration error.
    #[must_use]
    pub fn resolve_value(&self) -> Option<String> {
        if let Some(var_name) = self.value.strip_prefix('$') {
            std::env::var(var_name).ok()
        } else {
            Some(self.value.clone())
        }
    }

    /// Build the raw header value string to inject into the outbound request.
    ///
    /// Returns `None` when the secret could not be resolved (env-var missing).
    #[must_use]
    pub fn header_value(&self) -> Option<String> {
        let raw = self.resolve_value()?;
        match self.kind {
            AuthKind::Bearer => Some(format!("Bearer {raw}")),
            AuthKind::ApiKey => Some(raw),
            AuthKind::Basic => Some(format!("Basic {}", base64_encode(raw.as_bytes()))),
        }
    }
}

/// Minimal base64 encoder (standard RFC 4648 alphabet, no padding variation).
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let combined = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((combined >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((combined >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((combined >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(combined & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_kind_display_is_stable() {
        assert_eq!(AuthKind::Bearer.to_string(), "Bearer");
        assert_eq!(AuthKind::ApiKey.to_string(), "ApiKey");
        assert_eq!(AuthKind::Basic.to_string(), "Basic");
    }

    #[test]
    fn bearer_header_value() {
        let cfg = AuthConfig::bearer("tok123");
        assert_eq!(cfg.header_value(), Some("Bearer tok123".to_string()));
    }

    #[test]
    fn api_key_header_value() {
        let cfg = AuthConfig::api_key("X-API-Key", "secret");
        assert_eq!(cfg.header, "X-API-Key");
        assert_eq!(cfg.header_value(), Some("secret".to_string()));
    }

    #[test]
    fn basic_header_value_encodes_correctly() {
        let cfg = AuthConfig::basic("user:pass");
        assert_eq!(cfg.header_value(), Some("Basic dXNlcjpwYXNz".to_string()));
    }

    #[test]
    fn env_var_resolution_missing() {
        let cfg = AuthConfig::bearer("$__TEST_MISSING_VAR_12345__");
        assert_eq!(cfg.resolve_value(), None);
        assert_eq!(cfg.header_value(), None);
    }
}
