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

/// Python bootstrap shipped over the wire on every ``connect``.
///
/// The bootstrap installs ``dcc_mcp_maya._sidecar`` as a virtual
/// module by ``types.ModuleType`` + ``compile`` + ``exec``, wiring it
/// to the dispatcher in ``dcc_mcp_maya.sidecar._dispatcher``. This
/// means the wire-format entry point name is owned by **this binary**
/// rather than by a static ``.py`` shim file inside the Maya install
/// — sidecar protocol upgrades are atomic with the binary upgrade.
///
/// The source is embedded at build time via ``include_str!`` so the
/// bootstrap version cannot drift away from the rest of the client.
/// Idempotent on re-entry: each connect re-runs ``_install()`` but a
/// matching ``__dcc_mcp_bootstrap__`` returns immediately.
const SIDECAR_BOOTSTRAP_PY: &str = include_str!("../python/maya_sidecar_bootstrap.py");

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

    /// Ship the [`SIDECAR_BOOTSTRAP_PY`] payload over the live socket.
    ///
    /// Called once per [`HostRpcClient::connect`] right after the TCP
    /// handshake completes. The wire line is a self-contained Python
    /// expression — Rust's ``{:?}`` debug format produces a
    /// Python-compatible string literal (matching the escape rules
    /// for ``\n`` / ``\"`` / ``\\``), so we can wrap the source in
    /// a single ``exec(compile(<lit>, '<x>', 'exec'))`` invocation
    /// without any extra encoding step.
    ///
    /// Maya's ``commandPort`` with ``sourceType='python'`` always
    /// replies with **one** line per request — the ``str()``/``repr()``
    /// of the eval result. ``exec`` returns ``None``, so the bootstrap
    /// reply is the literal ``None`` on most Maya versions (or empty
    /// on some 2024 builds). We accept any non-traceback content;
    /// Python-level failures in the bootstrap body would otherwise
    /// only surface during the first real ``dispatch()`` call.
    async fn send_bootstrap(&self) -> Result<(), HostRpcError> {
        let line = format!(
            "__import__('builtins').exec(__import__('builtins').compile({src:?}, \
             '<dcc-mcp-sidecar-bootstrap>', 'exec'))\n",
            src = SIDECAR_BOOTSTRAP_PY,
        );

        let mut guard = self.state.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            HostRpcError::transport("CommandPortClient::send_bootstrap before connect")
        })?;

        if let Err(e) = conn.writer.write_all(line.as_bytes()).await {
            return Err(io_error_to_host_rpc(
                e,
                "commandport bootstrap write",
                "<bootstrap>",
                Value::Null,
                guard,
            ));
        }
        if let Err(e) = conn.writer.flush().await {
            return Err(io_error_to_host_rpc(
                e,
                "commandport bootstrap flush",
                "<bootstrap>",
                Value::Null,
                guard,
            ));
        }

        let mut buf = String::new();
        match conn.reader.read_line(&mut buf).await {
            Ok(0) => {
                *guard = None;
                Err(HostRpcError::host_died("<bootstrap>", None))
            }
            Ok(_) => {
                let trimmed = buf.trim_end_matches(['\r', '\n']);
                if trimmed.contains("Traceback") || trimmed.contains("SyntaxError:") {
                    Err(HostRpcError::transport(format!(
                        "commandport bootstrap raised inside Maya: {trimmed}",
                    )))
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(io_error_to_host_rpc(
                e,
                "commandport bootstrap read",
                "<bootstrap>",
                Value::Null,
                guard,
            )),
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

        // TCP handshake done; install the wire-frame entry point
        // inside Maya by shipping the Python bootstrap. Bounded by
        // the same timeout the caller passed to `connect` so the
        // "connection is live AND usable" deadline stays predictable.
        tokio::time::timeout(timeout, self.send_bootstrap())
            .await
            .map_err(|_| HostRpcError::Timeout {})??;
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
    let prefix = "commandport://";
    let rest = endpoint
        .get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .map(|_| &endpoint[prefix.len()..])
        .ok_or_else(|| {
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
    fn parse_endpoint_accepts_case_insensitive_scheme() {
        let (host, port) = parse_endpoint("CommandPort://127.0.0.1:6042").unwrap();
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

    /// Spawn an in-process TCP server that mimics Maya's commandPort.
    ///
    /// Reads one line at a time, sends back the next pre-staged
    /// response line, and repeats until the client drops or the
    /// caller's response queue is empty. Each request line is
    /// forwarded over `requests` so tests can assert on the exact
    /// wire bytes.
    ///
    /// The first response **must** correspond to the bootstrap line
    /// the client sends right after TCP connect — typically `None`
    /// (Maya's `exec()` eval result). Subsequent responses are the
    /// per-`call()` payloads.
    async fn spawn_fake_command_port_multi(
        responses: Vec<String>,
        keep_alive: bool,
    ) -> (u16, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 0");
        let port = listener.local_addr().expect("local_addr").port();
        let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let (read_half, mut write_half) = stream.split();
            let mut reader = BufReader::new(read_half);
            for response in responses {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                let _ = request_tx.send(line);
                let payload = format!("{response}\n");
                if write_half.write_all(payload.as_bytes()).await.is_err() {
                    return;
                }
                let _ = write_half.flush().await;
            }
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

    /// Backwards-friendly single-call wrapper used by tests that
    /// only want to verify the call path. Pre-stages two responses:
    /// one for the bootstrap (always `None`) and one for the real
    /// per-test response.
    async fn spawn_fake_command_port(
        response: String,
        keep_alive: bool,
    ) -> (u16, oneshot::Receiver<String>) {
        let (port, mut requests) =
            spawn_fake_command_port_multi(vec!["None".to_string(), response], keep_alive).await;
        let (tx, rx) = oneshot::channel();

        // Drop the bootstrap line; surface the real call line.
        tokio::spawn(async move {
            let _bootstrap = requests.recv().await;
            if let Some(call_line) = requests.recv().await {
                let _ = tx.send(call_line);
            }
        });

        (port, rx)
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
        // Server replies to the bootstrap normally, then drops the
        // connection before the first real call — simulating Maya
        // crashing AFTER its commandPort was wired but mid-call.
        let (port, mut requests) =
            spawn_fake_command_port_multi(vec!["None".to_string()], false).await;

        let mut client = CommandPortClient::new();
        client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_secs(2),
            )
            .await
            .expect("connect");

        // Drain the bootstrap line so the channel does not retain it.
        let _bootstrap = requests.recv().await;

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

    // ── bootstrap injection (Stage 1 of dynamic-module-install) ───

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_ships_bootstrap_as_first_wire_line() {
        // Fake commandPort: capture the FIRST line the client sends
        // and verify it carries the embedded bootstrap source.
        let (port, mut requests) =
            spawn_fake_command_port_multi(vec!["None".to_string()], true).await;

        let mut client = CommandPortClient::new();
        client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_secs(2),
            )
            .await
            .expect("connect (with bootstrap)");

        let bootstrap_line = requests
            .recv()
            .await
            .expect("bootstrap line should be observed by the server");

        // The wire is `exec(compile(<src_lit>, '<bootstrap>', 'exec'))`.
        assert!(
            bootstrap_line.contains("__import__('builtins').exec"),
            "bootstrap line must wrap exec(): {bootstrap_line:?}"
        );
        assert!(
            bootstrap_line.contains("<dcc-mcp-sidecar-bootstrap>"),
            "bootstrap line must tag its compiled filename: {bootstrap_line:?}"
        );
        // The dispatcher entry-point name MUST appear in the embedded
        // source — that's the contract every later call relies on.
        assert!(
            bootstrap_line.contains("dcc_mcp_maya._sidecar"),
            "bootstrap source must reference the wire-frame entry-point name"
        );
        assert!(
            bootstrap_line.contains("dcc_mcp_maya.sidecar._dispatcher"),
            "bootstrap source must reference the dispatcher import path"
        );

        client.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_surfaces_bootstrap_traceback_as_transport_error() {
        // Fake commandPort returns a Python traceback for the
        // bootstrap line — emulates a syntax error or import storm
        // inside Maya. The client must surface this as a transport
        // error so operators see it during connect rather than at
        // the first real call.
        let (port, _requests) = spawn_fake_command_port_multi(
            vec!["Traceback (most recent call last): SyntaxError: bogus".to_string()],
            true,
        )
        .await;

        let mut client = CommandPortClient::new();
        let result = client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_secs(2),
            )
            .await;

        match result {
            Err(HostRpcError::TransportError { message }) => {
                assert!(
                    message.contains("bootstrap raised"),
                    "transport error should explain bootstrap context: {message}"
                );
                assert!(
                    message.contains("Traceback") || message.contains("SyntaxError"),
                    "transport error should echo the Maya-side error: {message}"
                );
            }
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bootstrap_payload_is_python_string_literal_safe() {
        // The bootstrap line uses Rust's `{:?}` Debug format to embed
        // the source as a Python string literal. Pin that the
        // literal round-trips: every backslash / quote / newline in
        // the bootstrap source must show up correctly escaped in the
        // wire bytes.
        let (port, mut requests) =
            spawn_fake_command_port_multi(vec!["None".to_string()], true).await;
        let mut client = CommandPortClient::new();
        client
            .connect(
                &format!("commandport://127.0.0.1:{port}"),
                Duration::from_secs(2),
            )
            .await
            .expect("connect");
        let bootstrap_line = requests.recv().await.expect("bootstrap line");

        // Newlines in the source must NOT appear literally — they
        // should be `\n` two-byte sequences inside the Python string
        // literal Maya's `compile()` will parse.
        let body = bootstrap_line.trim_end_matches('\n');
        let inner_newlines = body.matches('\n').count();
        assert_eq!(
            inner_newlines, 0,
            "wire line must be a single Python line (`{body:?}` has embedded newlines)"
        );
        // Double-quote pairs inside the literal must be escaped.
        // If the source had un-escaped quotes the literal would close
        // prematurely and the rest of the source would land as Python
        // tokens — Maya would syntax-error.
        // Counting `\"` occurrences is a heuristic, but `repr(SRC)`
        // guarantees at least one (the literal's opening) — the test
        // mainly guards against future regressions where someone
        // switches to `{}` formatting.
        assert!(
            body.contains('"'),
            "literal opening/closing quotes must reach the wire"
        );
        client.close().await;
    }

    #[test]
    fn embedded_bootstrap_source_pins_known_contract() {
        // Static pin: bootstrap source MUST contain the canonical
        // module name + dispatcher import + bootstrap version
        // marker. Refactors that move the dispatcher or rename the
        // wire entry must update the bootstrap deliberately.
        assert!(SIDECAR_BOOTSTRAP_PY.contains("dcc_mcp_maya._sidecar"));
        assert!(SIDECAR_BOOTSTRAP_PY.contains("dcc_mcp_maya.sidecar._dispatcher"));
        assert!(SIDECAR_BOOTSTRAP_PY.contains("_BOOTSTRAP_VERSION"));
        assert!(SIDECAR_BOOTSTRAP_PY.contains("types.ModuleType"));
        assert!(SIDECAR_BOOTSTRAP_PY.contains("sys.modules"));
    }
}
