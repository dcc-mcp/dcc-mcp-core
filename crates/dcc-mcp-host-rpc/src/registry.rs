//! Scheme-based factory that picks a [`HostRpcClient`] impl from a
//! `--host-rpc <URI>` value at sidecar startup.
//!
//! The sidecar binary stays DCC-agnostic by linking every supported
//! impl and dispatching on the URI scheme:
//!
//! ```text
//! qtserver://127.0.0.1:18765    → QtServerClient      (Maya, Houdini, 3ds Max, Nuke, …)
//! commandport://127.0.0.1:6042  → CommandPortClient   (Maya — legacy / bootstrap only)
//! ws://127.0.0.1:9001            → WebSocketHostRpcClient (Photoshop UXP, bridge plug-ins)
//! wss://host.example/bridge      → WebSocketHostRpcClient (TLS WebSocket bridge plug-ins)
//! stub://anything               → StubHostRpcClient   (tests / placeholder)
//! ```
//!
//! Adding a new DCC family (Blender JSON-RPC over `http://`, Houdini
//! `hrpyc://`, Photoshop UXP `ws://`) is a matter of writing the
//! `HostRpcClient` impl in this crate and adding one match arm here.
//! The sidecar binary needs **zero** changes per DCC.
//!
//! Scope: this module owns scheme parsing **and** instantiation. The
//! split-out of the actual `connect()` call belongs to the sidecar
//! binary so the caller can pass a `--connect-timeout-secs` flag.

use crate::commandport::{CommandPortClient, URI_SCHEME as COMMANDPORT_SCHEME};
use crate::qtserver::{QtServerClient, URI_SCHEME as QTSERVER_SCHEME};
use crate::websocket::{WS_SCHEME, WSS_SCHEME, WebSocketHostRpcClient};
use crate::{HostRpcClient, HostRpcError, StubHostRpcClient};

/// URI scheme reserved for the placeholder client. Pinned as a const
/// so tests and the dispatcher both reference the same string.
pub const STUB_SCHEME: &str = "stub";

/// Pick a [`HostRpcClient`] impl based on the URI's scheme.
///
/// Returns the **disconnected** client — the caller is responsible
/// for invoking [`HostRpcClient::connect`] with the same URI and a
/// timeout of their choice.
///
/// # Errors
///
/// * [`HostRpcError::TransportError`] when:
///   - The URI is missing `://`.
///   - The scheme is empty.
///   - The scheme is not in the registry (see [`registered_schemes`]).
pub fn client_for_uri(endpoint: &str) -> Result<Box<dyn HostRpcClient>, HostRpcError> {
    let scheme = parse_scheme(endpoint)?;
    match scheme.as_str() {
        QTSERVER_SCHEME => Ok(Box::new(QtServerClient::new())),
        COMMANDPORT_SCHEME => Ok(Box::new(CommandPortClient::new())),
        WS_SCHEME => Ok(Box::new(WebSocketHostRpcClient::with_scheme(WS_SCHEME))),
        WSS_SCHEME => Ok(Box::new(WebSocketHostRpcClient::with_scheme(WSS_SCHEME))),
        STUB_SCHEME => Ok(Box::new(StubHostRpcClient::new())),
        other => Err(HostRpcError::transport(format!(
            "no HostRpcClient registered for scheme {other:?}; \
             supported schemes: {supported}",
            supported = registered_schemes().join(", "),
        ))),
    }
}

/// Every scheme the registry currently knows how to dispatch.
///
/// Kept stable so `--help`-style CLI surfaces in the sidecar binary
/// can enumerate them without duplicating the list.
#[must_use]
pub fn registered_schemes() -> Vec<&'static str> {
    vec![
        QTSERVER_SCHEME,
        COMMANDPORT_SCHEME,
        WS_SCHEME,
        WSS_SCHEME,
        STUB_SCHEME,
    ]
}

/// Lowercase scheme component of a `<scheme>://<rest>` URI.
///
/// Public so test code and adapter integration can validate URIs
/// without instantiating a client (cheap pre-flight check).
pub fn parse_scheme(endpoint: &str) -> Result<String, HostRpcError> {
    let (scheme, _) = endpoint.split_once("://").ok_or_else(|| {
        HostRpcError::transport(format!("URI missing :// separator — got {endpoint:?}"))
    })?;
    if scheme.is_empty() {
        return Err(HostRpcError::transport(format!(
            "URI has empty scheme — got {endpoint:?}"
        )));
    }
    Ok(scheme.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scheme_lowercases_and_strips() {
        assert_eq!(parse_scheme("commandport://x:1").unwrap(), "commandport");
        assert_eq!(parse_scheme("CommandPort://x:1").unwrap(), "commandport");
        assert_eq!(parse_scheme("qtserver://x:1").unwrap(), "qtserver");
        assert_eq!(parse_scheme("QtServer://x:1").unwrap(), "qtserver");
        assert_eq!(parse_scheme("WS://x:1").unwrap(), "ws");
        assert_eq!(parse_scheme("WSS://x:1").unwrap(), "wss");
        assert_eq!(parse_scheme("STUB://anything").unwrap(), "stub");
    }

    #[test]
    fn parse_scheme_rejects_missing_separator() {
        let result = parse_scheme("commandport-host-without-slashes");
        assert!(matches!(result, Err(HostRpcError::TransportError { .. })));
    }

    #[test]
    fn parse_scheme_rejects_empty_scheme() {
        let result = parse_scheme("://nothing");
        assert!(matches!(result, Err(HostRpcError::TransportError { .. })));
    }

    #[test]
    fn client_for_uri_dispatches_commandport() {
        let client = client_for_uri("commandport://127.0.0.1:6042").unwrap();
        assert_eq!(client.uri_scheme(), "commandport");
    }

    #[test]
    fn client_for_uri_dispatches_qtserver() {
        let client = client_for_uri("qtserver://127.0.0.1:18765").unwrap();
        assert_eq!(client.uri_scheme(), "qtserver");
    }

    #[test]
    fn client_for_uri_dispatches_stub() {
        let client = client_for_uri("stub://anything").unwrap();
        assert_eq!(client.uri_scheme(), "stub");
    }

    #[test]
    fn client_for_uri_dispatches_websocket() {
        let client = client_for_uri("ws://127.0.0.1:9001").unwrap();
        assert_eq!(client.uri_scheme(), "ws");
        let client = client_for_uri("wss://example.test/bridge").unwrap();
        assert_eq!(client.uri_scheme(), "wss");
    }

    #[test]
    fn client_for_uri_rejects_unknown_scheme_with_diagnostic() {
        let err = client_for_uri("http://example.com:80")
            .err()
            .expect("unknown scheme must error");
        match err {
            HostRpcError::TransportError { message } => {
                assert!(
                    message.contains("http"),
                    "error should echo the unknown scheme: {message}"
                );
                assert!(
                    message.contains("commandport"),
                    "error should list the supported schemes: {message}"
                );
                assert!(
                    message.contains("qtserver"),
                    "error should list the supported schemes: {message}"
                );
            }
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn registered_schemes_pins_the_set() {
        let schemes = registered_schemes();
        assert!(schemes.contains(&"commandport"));
        assert!(schemes.contains(&"qtserver"));
        assert!(schemes.contains(&"ws"));
        assert!(schemes.contains(&"wss"));
        assert!(schemes.contains(&"stub"));
        // If anyone adds a new scheme here, they MUST also update
        // the match in `client_for_uri`. This test fails loudly so
        // the two stay in sync.
        assert_eq!(schemes.len(), 5);
    }
}
