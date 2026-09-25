//! Unit tests for `gateway::port_recovery` (#2405).

use super::*;

use crate::gateway::runner::{ResidentGatewayHealth, probe_resident_gateway_health};

// ── Holder-child handshake (#2405, hardened in #3650) ─────────────
//
// The child owns port selection: it binds, then announces the port it
// actually got on stdout, and the parent waits for that line. Earlier
// revisions picked the port in the parent, spawned the child and only then
// closed the parent listener, so the child's `bind()` had to win a race it
// had no mechanism to win: when the parent's `close()` lost by a few
// milliseconds the child died on `EADDRINUSE`/`WSAEACCES` behind
// `Stdio::null()` and the parent timed out reporting an opaque
// "port holder child never bound port N" (#3650).
//
// Readiness is proven twice: the child says which port it bound, and a real
// `TcpStream::connect` shows something accepts connections there. Neither
// step shells out, neither needs privileges, and neither can be satisfied by
// an unrelated process that happens to sit on an ephemeral port.

/// Marks the test binary as a service-dead holder child instead of a normal
/// test run; without it [`port_holder_child_process`] returns immediately.
const HOLDER_CHILD_ENV: &str = "DCC_MCP_TEST_PORT_HOLDER";
/// Optional preferred port for the holder child. The child falls back to a
/// kernel-assigned port when the preferred one is taken.
const HOLDER_CHILD_PORT_ENV: &str = "DCC_MCP_TEST_PORT_HOLDER_PORT";
/// `<prefix> <port>` — the child bound that port and is accepting.
const HOLDER_READY_PREFIX: &str = "PORT_HOLDER_READY";
/// `<prefix> <error>` — the child could not bind; the parent retries.
const HOLDER_BIND_FAILED_PREFIX: &str = "PORT_HOLDER_BIND_FAILED";

/// Spawn a service-dead holder child and wait until that child is serving
/// the port it reports.
async fn start_service_dead_holder() -> (u16, std::process::Child) {
    spawn_service_dead_port_holder().await
}

/// Regression for the handshake race that turned `Rust coverage` red on main
/// (#3650).
///
/// The old helper bound an ephemeral port in the parent, spawned the child
/// and only then closed the parent listener. Whenever the parent's `close()`
/// lost that race the child's `bind()` failed with `EADDRINUSE`/`WSAEACCES`,
/// the child panicked behind `Stdio::null()`, and the parent timed out
/// reporting an opaque "port holder child never bound port N".
///
/// The collision is reproduced deterministically by holding the child's
/// preferred port for the whole handshake: the child must fall back to a
/// kernel-assigned port and report the one it actually bound, and the test
/// must see a live holder instead of a panic.
#[tokio::test]
async fn holder_child_falls_back_when_its_preferred_port_is_already_taken() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let contested = occupied.local_addr().unwrap().port();

    let (port, mut holder) = spawn_service_dead_port_holder_on(Some(contested)).await;

    assert_ne!(
        port, contested,
        "the holder must report the port it bound, never the one it failed to take"
    );
    assert!(
        try_bind_port_opt("127.0.0.1", port).await.is_none(),
        "the holder must own the port it reported"
    );
    assert!(
        try_bind_port_opt("127.0.0.1", contested).await.is_none(),
        "the contested port must stay held by this test"
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
    let _ = holder.wait();
    drop(occupied);
}

/// The readiness line is the whole handshake, so its parsing is pinned
/// directly: a misread line either stalls a test for the full timeout or,
/// worse, hands a test a port nobody holds.
#[test]
fn holder_handshake_lines_are_classified() {
    assert!(matches!(
        parse_holder_line(&format!("{HOLDER_READY_PREFIX} 45123")),
        Some(HolderSignal::Ready(45123))
    ));
    assert!(
        matches!(
            parse_holder_line(&format!("{HOLDER_BIND_FAILED_PREFIX} os error 10048")),
            Some(HolderSignal::BindFailed(_))
        ),
        "a bind failure must be reported as such so the parent can retry"
    );
    // A port the parent cannot parse is a failure, never a silent skip.
    assert!(
        matches!(
            parse_holder_line(&format!("{HOLDER_READY_PREFIX} not-a-port")),
            Some(HolderSignal::BindFailed(_))
        ),
        "a malported port must fail loudly instead of hanging the suite"
    );
    // libtest banners share the child's stdout and are never readiness.
    assert!(parse_holder_line("running 1 test").is_none());
    assert!(parse_holder_line("").is_none());
}

#[test]
fn unattributed_port_holder_is_never_reaped() {
    // No pidfile and no autolaunch manifest: the PID holding the port may
    // belong to an unrelated application, so recovery must fall back to
    // waiting rather than terminating it.
    let dir = tempfile::tempdir().unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();

    assert!(
        service_dead_port_holder_pid(port, dir.path(), None).is_none(),
        "an unattributed port holder must never be selected for termination"
    );
    drop(occupied);
}

#[tokio::test]
async fn pidfile_attributed_port_holder_is_selected_for_reaping() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (port, holder) = start_service_dead_holder().await;
    let pidfile = dir.path().join("gateway.pid");
    dcc_mcp_gateway_ensure::write_pidfile(&pidfile, holder.id()).unwrap();

    assert_eq!(
        service_dead_port_holder_pid(port, dir.path(), Some(&pidfile)),
        Some(holder.id())
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

#[tokio::test]
async fn autolaunch_manifest_attributes_the_port_holder() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (port, holder) = start_service_dead_holder().await;

    let manifest = serde_json::json!({
        "pid": holder.id(),
        "port": port,
        "executable": "dcc-mcp-server",
    });
    std::fs::write(
        dir.path().join(format!("gateway-autolaunch-{port}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    assert_eq!(
        service_dead_port_holder_pid(port, dir.path(), None),
        Some(holder.id())
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

#[tokio::test]
async fn autolaunch_manifest_for_another_pid_does_not_attribute() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (port, holder) = start_service_dead_holder().await;

    let manifest = serde_json::json!({ "pid": holder.id() + 1 });
    std::fs::write(
        dir.path().join(format!("gateway-autolaunch-{port}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    assert!(
        service_dead_port_holder_pid(port, dir.path(), None).is_none(),
        "a manifest naming a different PID must not authorize a kill"
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

#[tokio::test]
async fn autolaunch_manifest_for_another_port_does_not_attribute() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (port, holder) = start_service_dead_holder().await;

    // The manifest names the holder but a different port, so it is evidence
    // that *some* launcher started a gateway, not that this process holds the
    // port being recovered. Attributing on the PID alone would let a stale
    // manifest — or a recycled PID — authorize killing an unrelated process.
    // `+ 1` would overflow at the top of the ephemeral range.
    let other_port = port.checked_add(1).unwrap_or(port - 1);
    let manifest = serde_json::json!({ "pid": holder.id(), "port": other_port });
    std::fs::write(
        dir.path().join(format!("gateway-autolaunch-{port}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    assert!(
        service_dead_port_holder_pid(port, dir.path(), None).is_none(),
        "a manifest for another port must not authorize a kill"
    );

    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

#[tokio::test]
async fn service_dead_holder_is_reaped_and_the_port_is_released() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let (port, mut holder) = start_service_dead_holder().await;

    // The holder accepts TCP but never answers HTTP — exactly the
    // adversarial state from #2405. The bind fails while it lives.
    assert!(
        try_bind_port_opt("127.0.0.1", port).await.is_none(),
        "the service-dead holder must still own the port before recovery"
    );
    assert_eq!(
        probe_resident_gateway_health("127.0.0.1", port, Duration::from_millis(300)).await,
        ResidentGatewayHealth::Unhealthy,
        "a holder that never answers must probe as unhealthy"
    );

    let released =
        reap_service_dead_port_holder("127.0.0.1", port, holder.id(), Duration::from_secs(10))
            .await;

    assert!(released, "reaping the holder must release the port");
    // Collect the child. Until its parent waits, an exited child keeps its
    // process-table slot on Unix, so the assertion below would be checking
    // zombie bookkeeping instead of whether the holder is gone.
    let _ = holder.try_wait();
    assert!(
        !dcc_mcp_gateway_ensure::is_process_alive(holder.id()),
        "the stale holder must no longer be alive"
    );
    assert!(
        try_bind_port_opt("127.0.0.1", port).await.is_some(),
        "a healthy challenger can now bind the port"
    );
}

#[tokio::test]
async fn reaping_a_nonexistent_holder_reports_failure_without_panicking() {
    if !port_holder_resolution_available() {
        eprintln!("skipping: this platform cannot resolve the PID holding a listening port");
        return;
    }
    let (port, holder) = start_service_dead_holder().await;

    // Stopping an already-dead PID is idempotent, but the port stays held
    // by the live holder, so the release can never be confirmed.
    let released =
        reap_service_dead_port_holder("127.0.0.1", port, 9_999_999, Duration::from_millis(300))
            .await;

    assert!(
        !released,
        "release must not be reported while the port is still held"
    );
    let _ = dcc_mcp_gateway_ensure::stop_process(holder.id());
}

/// Spawn a service-dead holder child and return the port it bound.
///
/// The child selects its own ephemeral port and announces it once `bind()`
/// succeeds, so no part of this handshake depends on the parent releasing a
/// port before the child grabs it. See the [`HOLDER_CHILD_ENV`] block for why
/// that ordering mattered.
pub(crate) async fn spawn_service_dead_port_holder() -> (u16, std::process::Child) {
    spawn_service_dead_port_holder_on(None).await
}

/// Same as [`spawn_service_dead_port_holder`], but ask the child to prefer
/// `preferred_port`; the child falls back to a kernel-assigned port when that
/// one is already taken.
async fn spawn_service_dead_port_holder_on(
    preferred_port: Option<u16>,
) -> (u16, std::process::Child) {
    /// How many times to re-spawn the child when it reports it cannot bind.
    /// Each attempt lets the kernel pick a different ephemeral port, so a
    /// collision is retried away instead of failing the suite.
    const ATTEMPTS: usize = 3;
    /// Per-attempt readiness budget. An instrumented coverage binary takes
    /// seconds to exec, so this is deliberately generous; it only bounds a
    /// genuinely broken child.
    const READY_TIMEOUT: Duration = Duration::from_secs(20);

    let mut last_failure = String::from("no attempt was made");
    for _ in 0..ATTEMPTS {
        let mut child = spawn_port_holder(preferred_port);
        match wait_for_port_holder_ready(&mut child, READY_TIMEOUT).await {
            Ok(port) => return (port, child),
            Err(failure) => {
                last_failure = failure;
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    panic!("service-dead port holder never became ready after {ATTEMPTS} attempts: {last_failure}");
}

/// Spawn a helper process that binds a port and accepts connections without
/// ever responding — the "live but service-dead" holder.
///
/// The helper runs as a filtered libtest case, so the filter must be the
/// fully-qualified test path with `--exact`; a bare name matches nothing and
/// the child exits without binding. `stdout` carries the readiness line and
/// `stderr` is kept so a failure can quote the child's own OS error.
fn spawn_port_holder(preferred_port: Option<u16>) -> std::process::Child {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "gateway::port_recovery::port_recovery_tests::port_holder_child_process",
            "--exact",
            "--nocapture",
        ])
        .env(HOLDER_CHILD_ENV, "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(port) = preferred_port {
        command.env(HOLDER_CHILD_PORT_ENV, port.to_string());
    }
    command.spawn().expect("spawn service-dead port holder")
}

/// Wait for the holder child to announce the port it bound.
///
/// Returns that port after a real `TcpStream::connect` confirms someone is
/// listening, or a description of why the child never got there. The child's
/// stderr is drained into the message so the next failure names the actual OS
/// error instead of a generic timeout — the old `Stdio::null()` child hid
/// exactly that evidence.
async fn wait_for_port_holder_ready(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<u16, String> {
    let stdout = child
        .stdout
        .take()
        .expect("holder child stdout must be piped");
    let stderr = child
        .stderr
        .take()
        .expect("holder child stderr must be piped");

    // Both pipes are read on blocking std threads: the child writes one
    // readiness line and then runs forever, so an async read would either
    // block the runtime or need the same thread anyway. The stdout thread
    // forwards every line; the stderr thread keeps a live transcript.
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;

        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_transcript = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let transcript = std::sync::Arc::clone(&stderr_transcript);
    std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = [0u8; 512];
        loop {
            match std::io::Read::read(&mut stderr, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut sink) = transcript.lock() {
                        sink.push_str(&String::from_utf8_lossy(&buf[..read]));
                    }
                }
            }
        }
    });

    let deadline = tokio::time::Instant::now() + timeout;
    let mut chatter = Vec::new();
    loop {
        loop {
            match line_rx.try_recv() {
                // Unrecognised lines are libtest banners, not readiness.
                Ok(line) => match parse_holder_line(&line) {
                    Some(HolderSignal::Ready(port)) => {
                        return confirm_holder_listening(port).await;
                    }
                    Some(HolderSignal::BindFailed(error)) => {
                        return Err(format!(
                            "holder child could not bind a port: {error}\nchild stderr:\n{}",
                            stderr_snapshot(&stderr_transcript)
                        ));
                    }
                    None => chatter.push(line),
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(format!(
                        "holder child exited before reporting a port (stdout: {chatter:?})\nchild stderr:\n{}",
                        stderr_snapshot(&stderr_transcript)
                    ));
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!(
                "holder child did not report a port within {timeout:?} (stdout: {chatter:?})\nchild stderr:\n{}",
                stderr_snapshot(&stderr_transcript)
            ));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// What the holder child told us on stdout.
enum HolderSignal {
    /// The child bound this port and is accepting connections.
    Ready(u16),
    /// The child could not bind; it carries the OS error.
    BindFailed(String),
}

/// Classify one stdout line from the holder child; `None` is unrelated output.
fn parse_holder_line(line: &str) -> Option<HolderSignal> {
    let line = line.trim();
    if let Some(port) = line.strip_prefix(HOLDER_READY_PREFIX) {
        return match port.trim().parse::<u16>() {
            Ok(port) => Some(HolderSignal::Ready(port)),
            Err(_) => Some(HolderSignal::BindFailed(format!(
                "malformed port in {line:?}"
            ))),
        };
    }
    line.strip_prefix(HOLDER_BIND_FAILED_PREFIX)
        .map(|error| HolderSignal::BindFailed(error.trim().to_string()))
}

/// Prove that `port` accepts connections, so readiness never rests on the
/// child's word alone and never needs a privileged port-table probe.
async fn confirm_holder_listening(port: u16) -> Result<u16, String> {
    match tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    {
        Ok(Ok(_connection)) => Ok(port),
        Ok(Err(error)) => Err(format!(
            "holder child reported port {port} but nothing accepts a connection there: {error}"
        )),
        Err(_) => Err(format!(
            "holder child reported port {port} but connecting to it timed out"
        )),
    }
}

/// Snapshot of the child's stderr, truncated so a runaway child cannot flood
/// the panic message.
fn stderr_snapshot(transcript: &std::sync::Mutex<String>) -> String {
    const MAX_CHARS: usize = 2_000;
    let Ok(guard) = transcript.lock() else {
        return "<stderr unavailable>".to_string();
    };
    let trimmed = guard.trim();
    if trimmed.is_empty() {
        return "<child wrote nothing to stderr>".to_string();
    }
    match trimmed.char_indices().nth(MAX_CHARS) {
        Some((cut, _)) => format!("{} ... [truncated]", &trimmed[..cut]),
        None => trimmed.to_string(),
    }
}

/// Child-process body for the service-dead port holder.
///
/// The test harness only runs this as a real holder when the parent set
/// `DCC_MCP_TEST_PORT_HOLDER`; in a normal test run the variable is absent
/// and this returns immediately so the suite stays green.
///
/// The child picks the port itself — falling back from a preferred one to a
/// kernel-assigned one — and reports what it got, so a bind failure is a
/// recoverable event the parent can retry instead of a panic behind a null
/// stderr.
#[test]
fn port_holder_child_process() {
    if std::env::var_os(HOLDER_CHILD_ENV).is_none() {
        return;
    }
    let preferred_port = std::env::var(HOLDER_CHILD_PORT_ENV)
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok());

    let listener = match bind_holder_listener(preferred_port) {
        Ok(listener) => listener,
        Err(error) => {
            announce(HOLDER_BIND_FAILED_PREFIX, &error.to_string());
            return;
        }
    };
    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(error) => {
            announce(HOLDER_BIND_FAILED_PREFIX, &error.to_string());
            return;
        }
    };
    announce(HOLDER_READY_PREFIX, &port.to_string());

    // Accept connections and hold them open without ever writing a
    // response: clients connect successfully but never get an HTTP reply,
    // which is precisely the adversarial state described in #2405.
    let mut accepted = Vec::new();
    loop {
        match listener.accept() {
            Ok((stream, _peer)) => accepted.push(stream),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Bind the port the holder child will serve, preferring `preferred_port` and
/// falling back to any port the kernel hands out.
///
/// The fallback is the point: this child used to be handed a port the parent
/// had only just closed, so its `bind()` raced the parent's `close()` and lost
/// on loaded CI runners (#3650).
fn bind_holder_listener(preferred_port: Option<u16>) -> std::io::Result<std::net::TcpListener> {
    let Some(port) = preferred_port else {
        return std::net::TcpListener::bind(("127.0.0.1", 0));
    };
    match std::net::TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => Ok(listener),
        Err(error) => {
            eprintln!(
                "port holder: preferred port {port} is unavailable ({error}); \
                 falling back to a kernel-assigned port"
            );
            std::net::TcpListener::bind(("127.0.0.1", 0))
        }
    }
}

/// Report one handshake line to the parent and flush it immediately: the
/// parent waits on this line, and the child runs forever afterwards.
fn announce(prefix: &str, payload: &str) {
    use std::io::Write;

    println!("{prefix} {payload}");
    let _ = std::io::stdout().flush();
}
