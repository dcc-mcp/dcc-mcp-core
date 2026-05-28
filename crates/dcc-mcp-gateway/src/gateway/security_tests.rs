//! Unit tests for `gateway::security` (#1365).

use super::*;

#[test]
fn disabled_auth_accepts_every_request() {
    let auth = GatewayAuth::disabled();
    assert!(!auth.is_enabled());
    assert!(auth.authorize_register(None, "maya").is_ok());
    assert!(
        auth.authorize_register(Some("Bearer anything"), "blender")
            .is_ok()
    );
}

#[test]
fn missing_authorization_header_is_rejected_when_enabled() {
    let auth = GatewayAuth {
        tokens: vec![GatewayAuthToken::any_dcc("studio-master")],
    };
    let err = auth.authorize_register(None, "maya").unwrap_err();
    assert_eq!(err.kind(), "unauthorized");
    assert_eq!(err.http_status(), 401);
}

#[test]
fn non_bearer_scheme_is_rejected() {
    let auth = GatewayAuth {
        tokens: vec![GatewayAuthToken::any_dcc("studio-master")],
    };
    let err = auth
        .authorize_register(Some("Basic studio-master"), "maya")
        .unwrap_err();
    assert!(matches!(err, AuthError::MalformedBearer));
    assert_eq!(err.http_status(), 401);
}

#[test]
fn unknown_token_is_rejected() {
    let auth = GatewayAuth {
        tokens: vec![GatewayAuthToken::any_dcc("studio-master")],
    };
    let err = auth
        .authorize_register(Some("Bearer wrong-secret"), "maya")
        .unwrap_err();
    assert!(matches!(err, AuthError::UnknownToken));
    assert_eq!(err.kind(), "unauthorized");
}

#[test]
fn any_dcc_token_accepts_every_dcc_family() {
    let auth = GatewayAuth {
        tokens: vec![GatewayAuthToken::any_dcc("master-token")],
    };
    // Cover multiple DCC families per AGENTS.md multi-DCC guardrails.
    for dcc in ["maya", "blender", "houdini", "photoshop", "zbrush"] {
        assert!(
            auth.authorize_register(Some("Bearer master-token"), dcc)
                .is_ok(),
            "{dcc} rejected by any-DCC token"
        );
    }
}

#[test]
fn dcc_scope_mismatch_returns_structured_error() {
    let auth = GatewayAuth {
        tokens: vec![GatewayAuthToken::for_dcc("maya-only", ["maya"])],
    };
    // Photoshop must be rejected even with a valid token.
    let err = auth
        .authorize_register(Some("Bearer maya-only"), "photoshop")
        .unwrap_err();
    match err {
        AuthError::DccScopeMismatch { ref presented_dcc } => {
            assert_eq!(presented_dcc, "photoshop");
        }
        other => panic!("expected DccScopeMismatch, got {other:?}"),
    }
    assert_eq!(err.kind(), "dcc_scope_mismatch");
    assert_eq!(err.http_status(), 403);
    assert!(err.message().contains("photoshop"));
    // Maya is still accepted by the same token.
    assert!(
        auth.authorize_register(Some("Bearer maya-only"), "maya")
            .is_ok()
    );
}

#[test]
fn dcc_scope_can_list_multiple_dccs() {
    let auth = GatewayAuth {
        tokens: vec![GatewayAuthToken::for_dcc(
            "studio-token",
            ["maya", "blender"],
        )],
    };
    assert!(
        auth.authorize_register(Some("Bearer studio-token"), "maya")
            .is_ok()
    );
    assert!(
        auth.authorize_register(Some("Bearer studio-token"), "blender")
            .is_ok()
    );
    assert!(
        auth.authorize_register(Some("Bearer studio-token"), "photoshop")
            .is_err()
    );
}

#[test]
fn bearer_header_is_case_insensitive_on_scheme_only() {
    let auth = GatewayAuth {
        tokens: vec![GatewayAuthToken::any_dcc("secret")],
    };
    // Scheme matching is case-insensitive ("Bearer" / "bearer" / "BEARER").
    assert!(
        auth.authorize_register(Some("bearer secret"), "maya")
            .is_ok()
    );
    assert!(
        auth.authorize_register(Some("BEARER secret"), "maya")
            .is_ok()
    );
    // Token comparison is byte-exact (case-sensitive).
    assert!(
        auth.authorize_register(Some("Bearer SECRET"), "maya")
            .is_err()
    );
}

#[test]
fn empty_bearer_value_is_malformed() {
    let auth = GatewayAuth {
        tokens: vec![GatewayAuthToken::any_dcc("secret")],
    };
    let err = auth
        .authorize_register(Some("Bearer    "), "maya")
        .unwrap_err();
    assert!(matches!(err, AuthError::MalformedBearer));
}
