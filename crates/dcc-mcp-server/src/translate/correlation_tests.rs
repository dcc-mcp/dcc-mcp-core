//! Response-correlation regression tests for the `translate` stdio bridge.
//!
//! Split out of `translate.rs` so that module keeps to the bridge
//! implementation itself (see the file-size policy in `AGENTS.md`).
//!
//! The tests that spawn a child drive the real [`StdioBridge`] — not a
//! re-implementation of it — so a regression in the actor loop, the id
//! correlation or the wait bound is caught here rather than in a mirror.

use super::*;

mod id_key {
    use super::*;

    #[test]
    fn id_key_normalises_numbers_and_strings() {
        assert_eq!(
            request_id_key(Some(&Value::Number(7.into()))),
            Some("7".to_string())
        );
        assert_eq!(
            request_id_key(Some(&Value::String("abc".to_string()))),
            Some("abc".to_string())
        );
    }

    /// A child that round-trips an integer id through a float must still
    /// correlate: `2` and `2.0` name the same request id.
    #[test]
    fn id_key_normalises_integral_floats_to_integers() {
        let integral_float = serde_json::from_str::<Value>("2.0").expect("parse 2.0");
        assert_eq!(
            request_id_key(Some(&integral_float)),
            Some("2".to_string()),
            "a child echoing id 2 as 2.0 must correlate with the filed id 2"
        );
        let negative = serde_json::from_str::<Value>("-3.0").expect("parse -3.0");
        assert_eq!(request_id_key(Some(&negative)), Some("-3".to_string()));
    }

    #[test]
    fn id_key_keeps_non_integral_floats_distinct() {
        let fractional = serde_json::from_str::<Value>("2.5").expect("parse 2.5");
        assert_eq!(request_id_key(Some(&fractional)), Some("2.5".to_string()));
        assert_ne!(
            request_id_key(Some(&fractional)),
            request_id_key(Some(&Value::Number(2.into())))
        );
    }

    #[test]
    fn id_key_rejects_absent_and_non_scalar_ids() {
        assert_eq!(request_id_key(None), None);
        assert_eq!(request_id_key(Some(&Value::Null)), None);
        assert_eq!(request_id_key(Some(&serde_json::json!([1]))), None);
    }

    #[test]
    fn zero_timeout_disables_the_wait_bound() {
        assert_eq!(bridge_timeout(0), None);
        assert_eq!(bridge_timeout(600), Some(Duration::from_secs(600)));
    }
}

mod bridge {
    use super::*;

    /// A stdio child that answers the first request correctly and every later
    /// request with the *previous* request's id. The child is desynchronized:
    /// the payload it emits belongs to a call that already completed.
    const SKEWED_CHILD_PY: &str = r#"
import sys, json

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

previous_id = None
for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue
    req_id = msg.get("id")
    if req_id is None:
        continue
    echoed = req_id if previous_id is None else previous_id
    previous_id = req_id
    send({"jsonrpc": "2.0", "id": echoed, "result": {"echoed": echoed}})
"#;

    /// A stdio child that answers correctly but renders the integer id the way
    /// a JSON number round-tripped through a float does: `2` comes back `2.0`.
    const FLOAT_ECHO_CHILD_PY: &str = r#"
import sys, json

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue
    req_id = msg.get("id")
    if req_id is None:
        continue
    send({"jsonrpc": "2.0", "id": float(req_id), "result": {"ok": True}})
"#;

    /// A stdio child that emits one stray response — an id no caller issued —
    /// before serving every request correctly.
    const STRAY_THEN_HEALTHY_CHILD_PY: &str = r#"
import sys, json

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

send({"jsonrpc": "2.0", "id": 999, "result": {"stray": True}})

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue
    req_id = msg.get("id")
    if req_id is None:
        continue
    send({"jsonrpc": "2.0", "id": req_id, "result": {"ok": req_id}})
"#;

    /// A stdio child that answers slowly, so two calls using the same id can be
    /// filed while the first one is still in flight.
    const SLOW_ECHO_CHILD_PY: &str = r#"
import sys, json, time

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue
    req_id = msg.get("id")
    if req_id is None:
        continue
    time.sleep(0.5)
    send({"jsonrpc": "2.0", "id": req_id, "result": {"ok": req_id}})
"#;

    /// A stdio child that never answers its first request and serves every
    /// later one. Used to prove a timed-out call stops owning its id.
    const SILENT_FIRST_CHILD_PY: &str = r#"
import sys, json

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

answered = 0
for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue
    req_id = msg.get("id")
    if req_id is None:
        continue
    answered += 1
    if answered == 1:
        # Never answer the first request: it has to time out.
        continue
    send({"jsonrpc": "2.0", "id": req_id, "result": {"ok": req_id}})
"#;

    /// A stdio child that exits as soon as it receives a request, without
    /// answering it.
    const EXIT_ON_REQUEST_CHILD_PY: &str = r#"
import sys

for raw in sys.stdin:
    if raw.strip():
        break
"#;
    /// A stdio child that reads one request, closes its own stdin, and then
    /// stays alive with stdout open without ever answering anything.
    ///
    /// `os.close(0)` is what makes the parent's next write fail: closing the
    /// buffered `sys.stdin` instead leaves the read end open on Windows, where
    /// a small write is then still accepted.
    ///
    /// `__MARKER__` is replaced with a temp-file path. The child writes the
    /// marker's contents right after closing stdin, which is the signal the
    /// test waits for: no write issued after that point can be answered. The
    /// child exits once the marker file disappears, so a finished test leaves
    /// no stray process behind.
    const CLOSES_STDIN_CHILD_PY: &str = r#"
import os, sys, time

marker = r'__MARKER__'

# Read the first request, then close our own stdin so the parent's next write
# fails. stdout stays open and no request is ever answered, so the bridge's
# read task is still alive at that point -- which is what keeps this scenario
# distinct from the child simply exiting.
os.read(0, 65536)
os.close(0)
try:
    # The descriptor is gone already; closing the buffered object only stops
    # the interpreter from reporting a stale descriptor at shutdown.
    sys.stdin.close()
except OSError:
    pass

with open(marker, "w") as handle:
    handle.write("closed")

deadline = time.time() + 120.0
while time.time() < deadline and os.path.exists(marker):
    time.sleep(0.05)
"#;

    /// The interpreter `PATH` is expected to resolve for the stdio children.
    ///
    /// Single-sourced so the child command, the availability probe and the
    /// skip diagnostic all name the same interpreter.
    const fn child_program() -> &'static str {
        if cfg!(windows) { "python" } else { "python3" }
    }

    fn child_command(source: &str) -> (String, tempfile::NamedTempFile) {
        write_child_script(source)
    }

    /// Like [`child_command`], but with the `__MARKER__` placeholder replaced
    /// by `marker`, and with the child launched through `program`.
    ///
    /// The path is baked into the script instead of passed as an argument
    /// because [`parse_command`] splits the command on whitespace, so a path
    /// containing spaces would reach the child mangled. It lands in a raw
    /// Python string, so backslashes are rewritten to forward slashes (which
    /// Windows accepts) and nothing else needs escaping.
    fn child_command_with(
        source: &str,
        marker: &std::path::Path,
        program: &str,
    ) -> (String, tempfile::NamedTempFile) {
        let marker = marker.to_string_lossy().replace('\\', "/");
        write_child_script_with(&source.replace("__MARKER__", &marker), program)
    }

    /// Write a stdio child script to a temp file and return its command line.
    fn write_child_script(source: &str) -> (String, tempfile::NamedTempFile) {
        write_child_script_with(source, child_program())
    }

    /// Write a stdio child script to a temp file and return a command line
    /// that runs it with `program`.
    ///
    /// The handle is returned as well: dropping it deletes the script while the
    /// child may still be starting up.
    fn write_child_script_with(source: &str, program: &str) -> (String, tempfile::NamedTempFile) {
        use std::io::Write as _;
        let mut script = tempfile::Builder::new()
            .suffix(".py")
            .tempfile()
            .expect("tempfile");
        script.write_all(source.as_bytes()).expect("write script");
        script.flush().expect("flush");
        (format!("{program} {}", script.path().display()), script)
    }

    /// Block until the child has closed its stdin: until the marker it writes
    /// right after the close is there.
    ///
    /// Waiting for that explicit signal instead of sleeping is what makes the
    /// ordering this test relies on hold: the next write happens after the
    /// close, so it has to fail.
    async fn wait_for_stdin_close(marker: &std::path::Path) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let closed = std::fs::read_to_string(marker).is_ok_and(|body| body.contains("closed"));
            if closed {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the child never closed its stdin; the write-failure path was not reached"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Whether a usable Python interpreter is on `PATH`.
    ///
    /// The bridge tests spawn real child processes, so they need a working
    /// interpreter. Some Windows machines only ship the Microsoft Store
    /// `python.exe` stub, which exits with an error and prints nothing; probing
    /// keeps those machines from reporting failures that are really "no
    /// interpreter". The end-to-end suite in `tests/translate_bridge.rs` has
    /// always needed one too, but that is a separate test target.
    fn python_available() -> bool {
        interpreter_runs(child_program())
    }

    /// Whether `program` is an interpreter this test can actually run.
    fn interpreter_runs(program: &str) -> bool {
        let probe = std::process::Command::new(program)
            .arg("-c")
            .arg("print(1)")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();
        matches!(probe, Ok(out) if out.status.success() && out.stdout.first() == Some(&b'1'))
    }

    /// The interpreter `python` on `PATH` really resolves to.
    ///
    /// Launching a child through this path instead of through `python` itself
    /// is what keeps the write-failure scenario reachable when `python` is a
    /// launcher or a shim (`py.exe`, conda / pyenv-win shims, `uv run`, a
    /// `.cmd` / `.bat` wrapper): such a wrapper survives as an intermediate
    /// process and keeps the inherited read end of the child's stdin pipe
    /// open, so the child closing its own fd 0 never breaks the pipe.
    ///
    /// `None` when the interpreter cannot be resolved, or when it is not
    /// something this test can launch on its own -- notably when the path
    /// contains whitespace, which [`parse_command`] would split. Callers fall
    /// back to [`child_program`] in that case.
    fn resolved_interpreter() -> Option<String> {
        let probe = std::process::Command::new(child_program())
            .arg("-c")
            .arg("import sys;print(sys.executable)")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !probe.status.success() {
            return None;
        }
        let path = String::from_utf8_lossy(&probe.stdout).trim().to_string();
        if path.is_empty() || path.contains(char::is_whitespace) || !interpreter_runs(&path) {
            return None;
        }
        Some(path)
    }

    /// Describe the interpreter `PATH` resolves, for diagnostics only.
    ///
    /// `sys._base_executable` is the useful half: for a launcher it names the
    /// interpreter that was really started, which is what tells a wrapper
    /// apart from a direct binary.
    fn interpreter_details() -> String {
        let probe = std::process::Command::new(child_program())
            .arg("-c")
            .arg("import sys;print(sys.executable);print(getattr(sys,'_base_executable',''))")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();
        match probe {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                let mut lines = stdout
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty());
                let executable = lines.next().unwrap_or("<unknown>");
                let base = lines.next().unwrap_or("<unknown>");
                format!("sys.executable={executable}, sys._base_executable={base}")
            }
            Ok(out) => format!("<probe exited with {}>", out.status),
            Err(e) => format!("<probe failed: {e}>"),
        }
    }

    /// Skip the calling test when no usable Python interpreter is on `PATH`.
    fn skip_without_python() -> bool {
        if python_available() {
            return false;
        }
        eprintln!(
            "skipping: no usable python interpreter on PATH; the stdio bridge tests need one"
        );
        true
    }

    fn request(id: i64) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::Number(id.into())),
            method: "tools/call".to_string(),
            params: None,
        }
    }

    /// Build a bridge over a stdio child script, with the child restarted on
    /// exit when `restart_on_exit` is set.
    ///
    /// The wait bound is short so a regression that strands a call instead of
    /// answering it fails in seconds rather than hanging the suite.
    fn start_bridge_with(
        cmd: String,
        response_timeout: Option<Duration>,
        restart_on_exit: bool,
    ) -> StdioBridge {
        let (tx, rx) = mpsc::channel::<BridgeRequest>(16);
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let actor_pending = pending.clone();
        tokio::spawn(async move {
            run_bridge_actor(cmd, rx, restart_on_exit, 0, actor_pending).await;
        });
        StdioBridge {
            inner: Arc::new(StdioBridgeInner {
                tx,
                response_timeout,
                next_owner: AtomicU64::new(0),
                pending,
            }),
        }
    }

    fn start_bridge(cmd: String) -> StdioBridge {
        start_bridge_with(cmd, Some(Duration::from_secs(5)), false)
    }

    /// A child that answers with the previous request's id must never have that
    /// payload delivered as the current call's result: the stale response
    /// matches no pending id, so the call fails closed after the wait bound
    /// instead of returning another call's result.
    #[tokio::test]
    async fn stale_response_id_is_rejected_before_delivery() {
        if skip_without_python() {
            return;
        }
        let (cmd, _script) = child_command(SKEWED_CHILD_PY);
        let bridge = start_bridge(cmd);

        // First call is correlated: the child echoes id 1 correctly.
        let first = bridge
            .call(request(1))
            .await
            .expect("first call correlates");
        assert_eq!(
            request_id_key(first.id.as_ref()),
            Some("1".to_string()),
            "first call must get its own response"
        );

        // Second call: the child answers id 1 again. That payload belongs to
        // the previous call, so the bridge must refuse to deliver it. A child
        // this broken surfaces as a bounded wait, not as a wrong result.
        let second = bridge.call(request(2)).await;
        let error = second.expect_err("stale response must not be delivered");
        let message = error.to_string();
        assert!(
            message.contains("timed out") || message.contains("transport desync"),
            "the stale payload must be refused, got: {message}"
        );
    }

    /// A child echoing an integer id as `2.0` is answering correctly, so the
    /// response must correlate instead of reading as a desync.
    #[tokio::test]
    async fn float_rendered_response_id_still_correlates() {
        if skip_without_python() {
            return;
        }
        let (cmd, _script) = child_command(FLOAT_ECHO_CHILD_PY);
        let bridge = start_bridge(cmd);

        let response = bridge
            .call(request(2))
            .await
            .expect("an id echoed as 2.0 must correlate with the filed id 2");
        assert_eq!(
            request_id_key(response.id.as_ref()),
            Some("2".to_string()),
            "integral floats and integers must name the same request id"
        );
    }

    /// A stray response may be dropped; it must not strand the child so that
    /// every later call stalls for the full timeout.
    #[tokio::test]
    async fn stray_response_does_not_strand_the_child() {
        if skip_without_python() {
            return;
        }
        let (cmd, _script) = child_command(STRAY_THEN_HEALTHY_CHILD_PY);
        let bridge = start_bridge(cmd);

        // The child emits id 999 once, at startup. There is no call that
        // payload could belong to, so dropping it is enough: the child must
        // keep serving every request correctly.
        for id in [1, 2, 3] {
            let response = bridge
                .call(request(id))
                .await
                .unwrap_or_else(|e| panic!("call {id} must still be served, got: {e}"));
            assert_eq!(
                request_id_key(response.id.as_ref()),
                Some(id.to_string()),
                "call {id} must get its own response"
            );
        }
    }

    /// Two calls using the same JSON-RPC id: the second must be refused, not
    /// filed over the first.
    ///
    /// Filing it would evict the first caller's sender (which kills that call
    /// with a "dropped the channel" error) and let the child's single response
    /// answer whichever call it happened to be filed against.
    #[tokio::test]
    async fn duplicate_request_id_is_rejected_not_replaced() {
        if skip_without_python() {
            return;
        }
        let (cmd, _script) = child_command(SLOW_ECHO_CHILD_PY);
        let bridge = start_bridge(cmd);

        let first = {
            let bridge = bridge.clone();
            tokio::spawn(async move { bridge.call(request(1)).await })
        };
        // Let the actor file the first request before the second one arrives.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let second = bridge
            .call(request(1))
            .await
            .expect("a duplicate id is refused through a JSON-RPC error, not a dropped channel");
        let rejected = second.error.unwrap_or_else(|| {
            panic!(
                "the duplicate id must be rejected, got result: {:?}",
                second.result
            )
        });
        assert!(
            rejected.message.contains("duplicate request id"),
            "the rejection must name the reason, got: {}",
            rejected.message
        );

        // The first call still owns the id and gets its own response.
        let first = first
            .await
            .expect("join")
            .expect("the first caller must not be evicted by the duplicate");
        assert_eq!(
            request_id_key(first.id.as_ref()),
            Some("1".to_string()),
            "the first call must still receive its own response"
        );
    }

    /// A call that times out must stop owning its id, so the next request that
    /// reuses that id is served instead of being refused as a duplicate.
    #[tokio::test]
    async fn timed_out_call_releases_its_request_id() {
        if skip_without_python() {
            return;
        }
        let (cmd, _script) = child_command(SILENT_FIRST_CHILD_PY);
        let bridge = start_bridge_with(cmd, Some(Duration::from_millis(300)), false);

        // The child never answers the first request, so this call must fail on
        // the wait bound rather than hang forever.
        let first = bridge.call(request(1)).await;
        let error = first.expect_err("the silent first request must time out");
        assert!(
            error.to_string().contains("timed out"),
            "expected a bounded wait, got: {error}"
        );

        // Same id again: the child answers every request after the first one.
        // If the timed-out call had kept the entry, this call would be refused
        // as a duplicate (or evict an entry it does not own).
        let retry = bridge
            .call(request(1))
            .await
            .expect("the timed-out call must have released id 1");
        assert!(
            retry.error.is_none(),
            "the retry must be served, not refused as a duplicate: {:?}",
            retry.error
        );
        assert_eq!(
            request_id_key(retry.id.as_ref()),
            Some("1".to_string()),
            "the retry must receive its own response"
        );
    }

    /// When the child exits, calls still in flight fail immediately instead of
    /// waiting out the response bound: nothing is left that could answer them.
    #[tokio::test]
    async fn child_exit_fails_in_flight_calls_immediately() {
        if skip_without_python() {
            return;
        }
        let (cmd, _script) = child_command(EXIT_ON_REQUEST_CHILD_PY);
        // Restarting keeps the actor (and its pending map) alive across the
        // child's death, which is exactly when a leaked entry would linger.
        let bridge = start_bridge_with(cmd, Some(Duration::from_secs(30)), true);

        let started = std::time::Instant::now();
        let outcome = bridge.call(request(1)).await;
        let elapsed = started.elapsed();

        let error = outcome.expect_err("a child that exits without answering must fail the call");
        assert!(
            error.to_string().contains("dropped the channel"),
            "the call must fail because the child went away, got: {error}"
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "the call must fail when the child exits, not after the wait bound, took {elapsed:?}"
        );
    }

    /// A failed write to the child fails **every** call the child can no
    /// longer answer, not just the one whose write failed.
    ///
    /// The child closes its own stdin after the first request, so further
    /// writes fail while the first request is still in flight and unanswered,
    /// and while the bridge's reader is still alive -- the child did not exit,
    /// so this is the write-failure branch rather than the child-exit one.
    ///
    /// Releasing only the failed call's entry -- `retain(|_, entry| entry.owner
    /// != owner)` -- strands the first call for the whole response bound; the
    /// elapsed check below is what separates that from `clear()`.
    ///
    /// The child is launched through the interpreter `python` resolves to
    /// rather than through `python` itself, because a launcher or shim would
    /// stay alive as an intermediate process holding the pipe's read end and
    /// keep the write from ever failing. Only if the scenario stays
    /// unreachable even then does the test skip with a diagnostic instead of
    /// reporting a failure that says nothing about the bridge.
    #[tokio::test]
    async fn stdin_write_failure_fails_every_in_flight_call() {
        if skip_without_python() {
            return;
        }
        let marker = tempfile::Builder::new()
            .suffix(".marker")
            .tempfile()
            .expect("tempfile");
        // A launcher or shim on PATH would survive as an intermediate process
        // holding the pipe's read end open, so run the child through the
        // interpreter `python` actually resolves to.
        let resolved = resolved_interpreter();
        let program = resolved.as_deref().unwrap_or(child_program());
        let (cmd, _script) = child_command_with(CLOSES_STDIN_CHILD_PY, marker.path(), program);
        // A long bound: a regression that strands the in-flight call shows up
        // as a wait of this length rather than as a quick failure.
        let bridge = start_bridge_with(cmd, Some(Duration::from_secs(30)), false);

        let first = {
            let bridge = bridge.clone();
            tokio::spawn(async move { bridge.call(request(1)).await })
        };
        // The child has read request 1 and closed its stdin, and it never
        // answers anything from here on.
        wait_for_stdin_close(marker.path()).await;

        // Drive writes until one of them fails. Which write that is depends on
        // the platform: on Unix every write after the close fails, while on
        // Windows the first one is still accepted, so a single request is not
        // enough to reach the write-failure branch.
        let mut failed_at = None;
        for id in 2..=12 {
            let outcome =
                tokio::time::timeout(Duration::from_secs(1), bridge.call(request(id))).await;
            match outcome {
                // The write went through; the child simply is not answering.
                // Try the next one.
                Err(_) | Ok(Ok(_)) => continue,
                Ok(Err(_)) => {
                    failed_at = Some(std::time::Instant::now());
                    break;
                }
            }
        }
        let Some(failed_at) = failed_at else {
            // Last resort: the child closed its own stdin, yet no write ever
            // reached the failure branch. That means something between this
            // process and the interpreter still holds the inherited read end
            // of the pipe, so the child's `os.close(0)` does not break it and
            // every write keeps succeeding.
            //
            // "No write failed" is never a signal of the regression this test
            // pins: whether a write fails is decided by that IPC boundary, not
            // by how the pending map is released. Skipping here therefore does
            // not weaken the guard -- wherever the boundary does break, the
            // assertions below still run and still discriminate `clear()` from
            // `retain(|_, entry| entry.owner != owner)`.
            eprintln!(
                "skipping: no write to the child's closed stdin failed; something \
                 between this process and the interpreter keeps the pipe's read \
                 end open (child program: {program})"
            );
            eprintln!("skipping: interpreter details — {}", interpreter_details());
            // Let the child exit instead of sleeping out its full deadline.
            let _ = marker.close();
            return;
        };

        // The call that decides the regression: request 1 was filed, is still
        // unanswered, and the child's reader is alive, so clearing the pending
        // map is the only thing that can fail it now.
        let first = first.await.expect("join");
        let elapsed = failed_at.elapsed();
        let error = first.expect_err("the in-flight call must fail once the child stops reading");
        assert!(
            error.to_string().contains("dropped the channel"),
            "the in-flight call must fail on the write failure, got: {error}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "the in-flight call must fail with the failed write, not after the response bound, took {elapsed:?}"
        );

        // Removing the marker lets the child exit instead of sleeping out its
        // full deadline; dropping the handle would do the same.
        let _ = marker.close();
    }
}
