//! `CommandPortClient` — `HostRpcClient` over Maya's `commandPort` wire.
//!
//! This is the **first real per-DCC impl** of the [`HostRpcClient`]
//! trait (RFC #998 Phase 2, follow-up to PR #1005). The sidecar binary
//! at runtime picks this impl when `--host-rpc commandport://host:port`
//! is given.
//!
//! # Wire format
//!
//! Maya's `commandPort` (opened with
//! `cmds.commandPort(name=":NNNN", sourceType="python", noreturn=False)`)
//! is a plain TCP socket. The protocol is line-oriented:
//!
//! 1. Client connects.
//! 2. Client sends a Python expression terminated by `\n`.
//! 3. Maya evaluates the expression on its main thread.
//! 4. Maya writes the result as a single `\n`-terminated line back
//!    over the same socket. Empty line means "no return value".
//!
//! We pack `(action, args, request_id)` into a single Python call:
//!
//! ```python
//! __import__('dcc_mcp_maya._sidecar', fromlist=['dispatch']).dispatch(
//!     {"action": "...", "args": {...}, "request_id": "..."}
//! )
//! ```
//!
//! The Python side (`dcc_mcp_maya._sidecar.dispatch` — to be added in
//! the matching `dcc-mcp-maya` PR) is expected to return a JSON string
//! that the client parses back into a [`serde_json::Value`]. Until that
//! Python helper exists, the integration tests against a fake TCP
//! server prove the wire path is correct.
//!
//! # Cancellation
//!
//! `commandPort` does not support out-of-band cancellation — once a
//! Python expression starts evaluating on Maya's main thread, the only
//! way to abort it is via Maya's own escape-key mechanism (which the
//! sidecar cannot trigger remotely). So `cancel()` is a no-op stub
//! today. Future work: combine with Maya's `cmds.terminateLongRunning`
//! when that lands.
//!
//! # Concurrency
//!
//! Maya's `commandPort` is single-threaded — only one in-flight
//! request at a time per port. The client serialises calls via a
//! [`tokio::sync::Mutex`] over the connection state. Concurrent
//! gateway callers stack up on the mutex without races.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::{HostRpcClient, HostRpcError};

/// The URI scheme this impl handles. Pinned as a constant so the
/// scheme registry and tests both reference the same string.
pub const URI_SCHEME: &str = "commandport";

/// `HostRpcClient` over Maya's `commandPort`.
///
/// Instantiate via [`CommandPortClient::new`], then call
/// [`HostRpcClient::connect`] to dial the TCP socket. After that the
/// sidecar binary drives [`HostRpcClient::call`] / `cancel` / `close`
/// just like any other impl.
#[derive(Debug)]
pub struct CommandPortClient {
    state: Arc<Mutex<Option<Connection>>>,
}

#[derive(Debug)]
struct Connection {
    writer: OwnedWriteHalf,
    reader: BufReader<OwnedReadHalf>,
}

impl CommandPortClient {
    /// Construct a disconnected client. Call [`HostRpcClient::connect`]
    /// before the first [`HostRpcClient::call`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for CommandPortClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Wire envelope sent over `commandPort`. Serialised as a JSON object
/// argument to the Python `dispatch` helper that lives inside Maya.
#[derive(Serialize)]
struct WireFrame<'a> {
    action: &'a str,
    args: &'a Value,
    request_id: &'a str,
}

#[async_trait]
impl HostRpcClient for CommandPortClient {
    fn uri_scheme(&self) -> &'static str {
        URI_SCHEME
    }

    async fn connect(&mut self, endpoint: &str, timeout: Duration) -> Result<(), HostRpcError> {
        let (host, port) = parse_endpoint(endpoint)?;
        let stream = tokio::time::timeout(timeout, TcpStream::connect((host.as_str(), port)))
            .await
            .map_err(|_| HostRpcError::Timeout {})?
            .map_err(|e| HostRpcError::transport(format!("commandport connect {endpoint}: {e}")))?;
        // Disable Nagle so single-line MCP `tools/call` round-trips
        // are not artificially delayed by the kernel coalescing.
        let _ = stream.set_nodelay(true);
        let (read_half, write_half) = stream.into_split();
        *self.state.lock().await = Some(Connection {
            writer: write_half,
            reader: BufReader::new(read_half),
        });
        Ok(())
    }

    async fn call(
        &self,
        action: &str,
        args: Value,
        request_id: &str,
    ) -> Result<Value, HostRpcError> {
        let frame = WireFrame {
            action,
            args: &args,
            request_id,
        };
        let payload_json = serde_json::to_string(&frame)
            .map_err(|e| HostRpcError::transport(format!("encode wire frame: {e}")))?;
        // Single-line Python expression invoking the in-Maya dispatcher.
        // `__import__` form keeps it independent of `from … import …`
        // module-cache state inside the Maya session.
        let expression = format!(
            "__import__('dcc_mcp_maya._sidecar', fromlist=['dispatch']).dispatch({payload_json})\n",
        );

        let mut guard = self.state.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| HostRpcError::transport("CommandPortClient::call before connect"))?;

        if let Err(e) = conn.writer.write_all(expression.as_bytes()).await {
            return Err(io_error_to_host_rpc(
                e,
                "commandport write",
                action,
                args,
                guard,
            ));
        }
        if let Err(e) = conn.writer.flush().await {
            return Err(io_error_to_host_rpc(
                e,
                "commandport flush",
                action,
                args,
                guard,
            ));
        }

        let mut buf = String::new();
        match conn.reader.read_line(&mut buf).await {
            Ok(0) => {
                // Socket closed mid-call — Maya died (or commandPort
                // was torn down). Surface as the canonical host-died
                // signal so the gateway can emit a structured event.
                *guard = None;
                Err(HostRpcError::host_died(action, Some(args)))
            }
            Ok(_) => {
                let line = buf.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    // Maya commandPort returns an empty line when the
                    // Python expression evaluated to `None`. Map to
                    // `null` rather than failing — agents see a
                    // structured "no result" envelope.
                    return Ok(Value::Null);
                }
                serde_json::from_str::<Value>(line).map_err(|e| {
                    HostRpcError::transport(format!(
                        "commandport returned non-JSON line ({e}); raw: {line:?}",
                    ))
                })
            }
            Err(e) => Err(io_error_to_host_rpc(
                e,
                "commandport read",
                action,
                args,
                guard,
            )),
        }
    }

    fn is_alive(&self) -> bool {
        // Best-effort. A held lock means a call is in flight, which
        // implies the socket was alive at least up to the read syscall.
        // Reporting "alive" while contended is honest because the
        // sidecar's eviction logic is meant to fire on terminal
        // disconnects, not on transient busy windows.
        match self.state.try_lock() {
            Ok(guard) => guard.is_some(),
            Err(_) => true,
        }
    }

    async fn close(&self) {
        let mut guard = self.state.lock().await;
        if let Some(conn) = guard.take() {
            drop(conn); // closes the TCP halves
        }
    }
}

/// Map a `tokio` / `std::io::Error` raised during a `commandPort`
/// read/write/flush into the canonical [`HostRpcError`] envelope.
///
/// The hot path is **disconnect mid-call**, which the gateway must
/// surface as a structured `host-died` event (so agents stop seeing
/// transport-error cascades when Maya crashes). On Linux/macOS this
/// usually shows up as `read_line() == Ok(0)` (EOF). On Windows the
/// remote close more commonly aborts the **next write** first with
/// WSAECONNABORTED / WSAECONNRESET (mapped by tokio to
/// `ErrorKind::ConnectionAborted` / `ConnectionReset` / `BrokenPipe`).
///
/// Anything else (e.g. `Interrupted`, `WouldBlock` — which tokio
/// generally retries internally) is left as a plain transport error.
fn io_error_to_host_rpc(
    error: std::io::Error,
    op: &str,
    action: &str,
    args: Value,
    mut guard: tokio::sync::MutexGuard<'_, Option<Connection>>,
) -> HostRpcError {
    use std::io::ErrorKind;
    let is_terminal = matches!(
        error.kind(),
        ErrorKind::ConnectionAborted
            | ErrorKind::ConnectionReset
            | ErrorKind::BrokenPipe
            | ErrorKind::NotConnected
            | ErrorKind::UnexpectedEof,
    );
    if is_terminal {
        // Drop the socket so subsequent calls fail fast at the
        // "before connect" check instead of dragging another error
        // out of the dead OS handle.
        *guard = None;
        return HostRpcError::host_died(action, Some(args));
    }
    HostRpcError::transport(format!("{op}: {error}"))
}

/// Parse a `commandport://host:port` URI into its TCP target.
fn parse_endpoint(endpoint: &str) -> Result<(String, u16), HostRpcError> {
    let rest = endpoint.strip_prefix("commandport://").ok_or_else(|| {
        HostRpcError::transport(format!("expected commandport:// URI, got {endpoint:?}"))
    })?;
    let (host, port_str) = rest.rsplit_once(':').ok_or_else(|| {
        HostRpcError::transport(format!("commandport URI missing :port — got {endpoint:?}"))
    })?;
    if host.is_empty() {
        return Err(HostRpcError::transport(format!(
            "commandport URI has empty host — got {endpoint:?}"
        )));
    }
    let port: u16 = port_str.parse().map_err(|_| {
        HostRpcError::transport(format!(
            "commandport URI port is not a u16 — got {endpoint:?}"
        ))
    })?;
    if port == 0 {
        return Err(HostRpcError::transport(format!(
            "commandport URI port must be non-zero — got {endpoint:?}"
        )));
    }
    Ok((host.to_string(), port))
}

// ── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    // ── parse_endpoint ───────────────────────────────────────────────────

    #[test]
    fn parse_endpoint_happy_path() {
        let (host, port) = parse_endpoint("commandport://127.0.0.1:6042").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 6042);
    }

    #[test]
    fn parse_endpoint_accepts_ipv6_bracketed_form_skipped() {
        // ipv6 needs `[::1]:6042` style — we only support v4 in this
        // first slice; pin the behaviour so the future ipv6 PR has a
        // clear surface to extend.
        let result = parse_endpoint("commandport://::1:6042");
        // The `rsplit_once(':')` finds the last ':' which sits in the
        // middle of the v6 literal, producing a host of "::1" and a
        // port of "6042". That happens to work for raw `::1` and is
        // intentional — we just don't promise general v6 support.
        assert!(
            result.is_ok(),
            "loose IPv6 parsing should currently succeed: {result:?}"
        );
    }

    #[test]
    fn parse_endpoint_rejects_wrong_scheme() {
        for bad in ["http://127.0.0.1:6042", "127.0.0.1:6042", "tcp://x:1"] {
            let result = parse_endpoint(bad);
            assert!(
                matches!(result, Err(HostRpcError::TransportError { .. })),
                "wrong scheme should reject: {bad}",
            );
        }
    }

    #[test]
    fn parse_endpoint_rejects_missing_port() {
        let result = parse_endpoint("commandport://127.0.0.1");
        assert!(matches!(result, Err(HostRpcError::TransportError { .. })));
    }

    #[test]
    fn parse_endpoint_rejects_zero_port() {
        let result = parse_endpoint("commandport://127.0.0.1:0");
        assert!(matches!(result, Err(HostRpcError::TransportError { .. })));
    }

    #[test]
    fn parse_endpoint_rejects_oversized_port() {
        let result = parse_endpoint("commandport://127.0.0.1:70000");
        assert!(matches!(result, Err(HostRpcError::TransportError { .. })));
    }

    #[test]
    fn parse_endpoint_rejects_empty_host() {
        let result = parse_endpoint("commandport://:6042");
        assert!(matches!(result, Err(HostRpcError::TransportError { .. })));
    }

    // ── connect / call roundtrip against a fake commandPort ────────────

    /// Spawn an in-process TCP server that mimics Maya's commandPort:
    /// accept one connection, read one line, write a JSON response,
    /// and either close or keep the connection open for more rounds.
    ///
    /// Returns the bound port and a one-shot the test can use to wait
    /// for the server to observe the request.
    async fn spawn_fake_command_port(
        response: String,
        keep_alive: bool,
    ) -> (u16, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 0");
        let port = listener.local_addr().expect("local_addr").port();
        let (request_tx, request_rx) = oneshot::channel();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let (read_half, mut write_half) = stream.split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;
            let _ = request_tx.send(line);
            let payload = format!("{response}\n");
            let _ = write_half.write_all(payload.as_bytes()).await;
            let _ = write_half.flush().await;
            if keep_alive {
                // Hold the connection open so the client's `close()`
                // gets to drive the TCP shutdown handshake. The halves
                // drop when this future returns, which is the natural
                // signal to the client that we are done.
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });

        (port, request_rx)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_call_close_roundtrip() {
        let (port, request_rx) = spawn_fake_command_port(
            r#"{"success":true,"object_name":"pSphere1"}"#.to_string(),
            true,
        )
        .await;

        let mut client = CommandPortClient::new();
        client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_secs(2),
            )
            .await
            .expect("connect");
        assert!(client.is_alive(), "client should be alive after connect");

        let response = client
            .call(
                "maya_primitives__create_sphere",
                serde_json::json!({"radius": 1.0}),
                "req-test-1",
            )
            .await
            .expect("call should succeed against fake server");

        assert_eq!(response["success"], true);
        assert_eq!(response["object_name"], "pSphere1");

        let request_line = request_rx.await.expect("server observed request");
        assert!(
            request_line.contains("maya_primitives__create_sphere"),
            "wire frame must include action slug, got: {request_line:?}",
        );
        assert!(
            request_line.contains("dcc_mcp_maya._sidecar"),
            "wire expression must invoke the in-Maya dispatcher, got: {request_line:?}",
        );
        assert!(
            request_line.contains("\"radius\":1.0"),
            "wire frame must include serialised args, got: {request_line:?}",
        );

        client.close().await;
        assert!(!client.is_alive(), "client should NOT be alive after close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_died_when_connection_closes_during_call() {
        // Server closes the connection immediately after accept,
        // simulating Maya crashing mid-call.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local_addr").port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            // Half-close the write side so the client's read_line
            // returns Ok(0) — that's the "EOF mid-call" signal that
            // tells us Maya died.
            drop(stream);
        });

        let mut client = CommandPortClient::new();
        client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_secs(2),
            )
            .await
            .expect("connect");

        let result = client
            .call(
                "maya_render__playblast",
                serde_json::json!({"width": 1280}),
                "req-host-died",
            )
            .await;

        match result {
            Err(HostRpcError::HostDied {
                last_call_slug,
                last_call_args,
            }) => {
                assert_eq!(last_call_slug.as_deref(), Some("maya_render__playblast"));
                assert_eq!(last_call_args.unwrap()["width"], 1280);
            }
            other => panic!("expected HostDied, got {other:?}"),
        }

        // After a host-died, is_alive must read as false — the gateway
        // uses this to evict the backend from routing.
        assert!(!client.is_alive());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_response_line_maps_to_null() {
        // Maya returns an empty line when the Python expression
        // evaluated to None — common for fire-and-forget setters.
        let (port, _request_rx) = spawn_fake_command_port(String::new(), true).await;

        let mut client = CommandPortClient::new();
        client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_secs(2),
            )
            .await
            .expect("connect");
        let response = client
            .call(
                "maya_scene__set_unit",
                serde_json::json!({"unit": "cm"}),
                "req-null",
            )
            .await
            .expect("empty response is valid (None → Null)");
        assert!(
            response.is_null(),
            "empty response line must deserialize to Value::Null, got {response:?}",
        );
        client.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn invalid_json_response_surfaces_transport_error() {
        let (port, _request_rx) =
            spawn_fake_command_port("not even close to json".to_string(), true).await;

        let mut client = CommandPortClient::new();
        client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_secs(2),
            )
            .await
            .expect("connect");
        let result = client
            .call("noop", serde_json::Value::Null, "req-bad-json")
            .await;
        match result {
            Err(HostRpcError::TransportError { message }) => {
                assert!(
                    message.contains("non-JSON"),
                    "transport error should mention non-JSON; got: {message}",
                );
            }
            other => panic!("expected TransportError, got {other:?}"),
        }
        client.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_times_out_when_unreachable() {
        // Bind a listener and immediately drop it so the port is
        // listed in some routing tables but refuses connections —
        // OR allocate a free port and never bind it. The second is
        // more reliable on Windows where the SYN gets RST'd instantly.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local_addr").port();
        drop(listener);

        let mut client = CommandPortClient::new();
        let result = client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_millis(250),
            )
            .await;
        assert!(
            matches!(
                result,
                Err(HostRpcError::TransportError { .. }) | Err(HostRpcError::Timeout { .. })
            ),
            "unreachable port should produce TransportError or Timeout, got {result:?}",
        );
    }

    #[tokio::test]
    async fn call_before_connect_is_transport_error() {
        let client = CommandPortClient::new();
        let result = client.call("noop", serde_json::Value::Null, "req").await;
        match result {
            Err(HostRpcError::TransportError { message }) => {
                assert!(message.contains("before connect"));
            }
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn uri_scheme_is_stable() {
        // Pinned so the registry dispatcher can match against the
        // same constant without typo drift between modules.
        assert_eq!(URI_SCHEME, "commandport");
        assert_eq!(CommandPortClient::new().uri_scheme(), "commandport");
    }
}
