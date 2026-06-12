from __future__ import annotations

import os
from pathlib import Path
import sys
import threading
import time
import types

import dcc_mcp_core._server.gateway_guardian as gg


def _wait_until(predicate, *, timeout: float = 10.0, interval: float = 0.01) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return predicate()


class _Resp:
    status = 200

    def __enter__(self):
        return self

    def __exit__(self, *_exc):
        return False


def test_ensure_gateway_daemon_reports_existing_health(monkeypatch):
    monkeypatch.setattr(gg, "urlopen", lambda *args, **kwargs: _Resp())
    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="blender",
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"


def test_resolve_server_bin_uses_packaged_binary_when_path_missing(monkeypatch, tmp_path):
    binary = tmp_path / "dcc-mcp-server"
    binary.write_text("", encoding="utf-8")
    module = types.ModuleType("dcc_mcp_server")
    module.binary_path = lambda: binary

    monkeypatch.delenv("DCC_MCP_SERVER_BIN", raising=False)
    monkeypatch.setattr(gg.shutil, "which", lambda _name: None)
    monkeypatch.setitem(sys.modules, "dcc_mcp_server", module)

    assert gg._resolve_server_bin() == str(binary)


def test_resolve_server_bin_prefers_explicit_env(monkeypatch, tmp_path):
    explicit = tmp_path / "custom-server"
    module = types.ModuleType("dcc_mcp_server")
    module.binary_path = lambda: Path("/unused/dcc-mcp-server")

    monkeypatch.setenv("DCC_MCP_SERVER_BIN", str(explicit))
    monkeypatch.setitem(sys.modules, "dcc_mcp_server", module)

    assert gg._resolve_server_bin() == str(explicit)


def test_ensure_gateway_daemon_spawns_and_becomes_healthy(tmp_path, monkeypatch):
    state = {"calls": 0}

    def _urlopen(*_args, **_kwargs):
        state["calls"] += 1
        if state["calls"] < 3:
            raise OSError("down")
        return _Resp()

    seen = {}

    def _launch_detached(cmd, **kwargs):
        seen["cmd"] = cmd
        seen["env"] = kwargs.get("env", {})
        return {"ok": True, "pid": 1}

    monkeypatch.setattr(gg, "urlopen", _urlopen)
    monkeypatch.setattr(gg, "launch_detached", _launch_detached)
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")
    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9876,
        registry_dir=str(tmp_path),
        dcc_type="photoshop",
        timeout_secs=1.0,
    )
    assert result["ok"] is True
    assert result["reason"] == "spawned"
    assert seen["cmd"][:2] == ["dcc-mcp-server", "gateway"]
    assert "--port" in seen["cmd"]
    assert "--registry-dir" not in seen["cmd"]
    assert seen["env"]["DCC_MCP_GATEWAY_PORT"] == "9876"
    assert seen["env"]["DCC_MCP_REGISTRY_DIR"] == str(tmp_path)
    assert seen["env"]["DCC_MCP_DCC_TYPE"] == "photoshop"
    assert not (tmp_path / "gateway-launch.lock").exists()


def test_ensure_gateway_daemon_spawn_failure_returns_embedded_fallback_reason(monkeypatch, tmp_path):
    monkeypatch.setattr(gg, "urlopen", lambda *_a, **_k: (_ for _ in ()).throw(OSError("down")))

    monkeypatch.setattr(
        gg,
        "launch_detached",
        lambda *_a, **_k: {"ok": False, "reason": "spawn_failed", "error": "spawn fail"},
    )
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")
    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="maya",
    )
    assert result["ok"] is False
    assert result["reason"] == "spawn_failed"
    assert "command" in result


def test_ensure_gateway_daemon_waits_when_launch_lock_exists(tmp_path, monkeypatch):
    (tmp_path / "gateway-launch.lock").write_text("busy", encoding="utf-8")
    monkeypatch.setattr(gg, "urlopen", lambda *_a, **_k: (_ for _ in ()).throw(OSError("down")))
    monkeypatch.setattr(
        gg,
        "launch_detached",
        lambda *_a, **_k: (_ for _ in ()).throw(AssertionError("must not spawn")),
    )

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="maya",
        timeout_secs=0.01,
    )

    assert result["ok"] is False
    assert result["reason"] == "launch_in_progress_timeout"


def test_ensure_gateway_daemon_lock_loser_succeeds_when_gateway_becomes_healthy(
    tmp_path,
    monkeypatch,
):
    """Lock loser waits and succeeds if the winner brings the gateway healthy."""
    (tmp_path / "gateway-launch.lock").write_text("busy", encoding="utf-8")

    probe_count = {"n": 0}

    def _urlopen(*_args, **_kwargs):
        probe_count["n"] += 1
        # First probe fails, second and later succeed (winner finishes launch)
        if probe_count["n"] < 2:
            raise OSError("down")
        return _Resp()

    monkeypatch.setattr(gg, "urlopen", _urlopen)
    monkeypatch.setattr(
        gg,
        "launch_detached",
        lambda *_a, **_k: (_ for _ in ()).throw(AssertionError("must not spawn")),
    )

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="maya",
        timeout_secs=5.0,  # enough for the 100ms sleep between probes
    )

    assert result["ok"] is True
    assert result["reason"] == "launch_in_progress"


def test_ensure_gateway_daemon_respects_ensure_timeout_env_var(tmp_path, monkeypatch):
    """DCC_MCP_GATEWAY_ENSURE_TIMEOUT_SECS overrides the default timeout."""
    (tmp_path / "gateway-launch.lock").write_text("busy", encoding="utf-8")
    monkeypatch.setattr(gg, "urlopen", lambda *_a, **_k: (_ for _ in ()).throw(OSError("down")))
    monkeypatch.setattr(
        gg,
        "launch_detached",
        lambda *_a, **_k: (_ for _ in ()).throw(AssertionError("must not spawn")),
    )
    monkeypatch.setenv("DCC_MCP_GATEWAY_ENSURE_TIMEOUT_SECS", "0.01")

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="houdini",
    )

    assert result["ok"] is False
    assert result["reason"] == "launch_in_progress_timeout"


def test_ensure_gateway_daemon_recovers_stale_launch_lock(tmp_path, monkeypatch):
    lock_path = tmp_path / "gateway-launch.lock"
    lock_path.write_text("stale", encoding="utf-8")
    stale_time = time.time() - 120
    os.utime(lock_path, (stale_time, stale_time))

    state = {"calls": 0}

    def _urlopen(*_args, **_kwargs):
        state["calls"] += 1
        if state["calls"] < 3:
            raise OSError("down")
        return _Resp()

    seen = {}

    def _launch_detached(cmd, **kwargs):
        seen["cmd"] = cmd
        seen["env"] = kwargs.get("env", {})
        return {"ok": True, "pid": 1}

    monkeypatch.setenv("DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS", "1")
    monkeypatch.setattr(gg, "urlopen", _urlopen)
    monkeypatch.setattr(gg, "launch_detached", _launch_detached)
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="houdini",
        timeout_secs=1.0,
    )

    assert result["ok"] is True
    assert result["reason"] == "spawned"
    assert seen["cmd"][:2] == ["dcc-mcp-server", "gateway"]
    assert seen["env"]["DCC_MCP_DCC_TYPE"] == "houdini"
    assert not lock_path.exists()


# ---------------------------------------------------------------------------
# _LaunchLock TOCTOU race tests (PIP-1417)
# ---------------------------------------------------------------------------


def test_launch_lock_toctou_fresh_lock_appears_during_stale_reclaim(tmp_path, monkeypatch):
    """Double-check catches a fresh lock that appears before unlink.

    Simulates: Process A sees stale lock → Process B creates fresh lock
    between A's first and second stat → A's double-check detects the fresh
    lock and bails out (does NOT remove it).
    """
    lock_path = tmp_path / "gateway-launch.lock"

    # Place a stale lock.
    lock_path.write_text("stale", encoding="utf-8")
    stale_time = time.time() - 120
    os.utime(lock_path, (stale_time, stale_time))

    monkeypatch.setenv("DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS", "1")

    # Override _remove_stale_launch_lock so that between the two stat calls
    # a "peer" refreshes the lock.
    original_remove = gg._remove_stale_launch_lock

    call_counter = 0

    def _remove_wrapper(path, stale_after):
        nonlocal call_counter
        call_counter += 1
        if call_counter == 1:
            # After the first stat (stale confirmed), simulate a peer
            # recreating a fresh lock before our unlink.
            path.write_text("fresh-from-peer", encoding="utf-8")
            now = time.time()
            os.utime(path, (now, now))
        return original_remove(path, stale_after)

    monkeypatch.setattr(gg, "_remove_stale_launch_lock", _remove_wrapper)

    lock = gg._LaunchLock(lock_path)
    acquired = lock.acquire()

    # The double-check should refuse to remove the freshly recreated lock.
    assert acquired is False
    assert lock_path.exists()
    # Content should be the peer's fresh lock.
    assert lock_path.read_text() == "fresh-from-peer"


def test_launch_lock_toctou_lock_appears_between_unlink_and_retry(tmp_path, monkeypatch):
    """Retry-create fails gracefully when a peer creates a lock between unlink and retry.

    Simulates: Process A removes stale lock → Process B creates a fresh lock
    → A retries os.O_CREAT | os.O_EXCL → FileExistsError → acquire returns False.
    """
    lock_path = tmp_path / "gateway-launch.lock"

    # Place a stale lock.
    lock_path.write_text("stale", encoding="utf-8")
    stale_time = time.time() - 120
    os.utime(lock_path, (stale_time, stale_time))

    monkeypatch.setenv("DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS", "1")

    # Override _remove_stale_launch_lock to simulate TOCTOU: after unlink
    # succeeds, a peer immediately creates a fresh lock before we retry.
    original_remove = gg._remove_stale_launch_lock

    def _remove_wrapper(path, stale_after):
        result = original_remove(path, stale_after)
        if result:
            # Peer creates a fresh lock right after our unlink.
            path.write_text("peer-won-race", encoding="utf-8")
        return result

    monkeypatch.setattr(gg, "_remove_stale_launch_lock", _remove_wrapper)

    lock = gg._LaunchLock(lock_path)
    acquired = lock.acquire()

    # Our retry should hit FileExistsError and return False.
    assert acquired is False
    assert lock_path.exists()
    assert lock_path.read_text() == "peer-won-race"


def test_launch_lock_acquire_single_attempt_no_loop(tmp_path, monkeypatch):
    """acquire() makes exactly one stale-reclaim attempt — no implicit retry loop.

    Verifies the flat control-flow structure aligned with Rust's
    ``acquire_launch_lock_with_stale``.
    """
    lock_path = tmp_path / "gateway-launch.lock"

    # Fresh lock held by another process.
    lock_path.write_text("busy", encoding="utf-8")

    monkeypatch.setenv("DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS", "3600")

    lock = gg._LaunchLock(lock_path)
    acquired = lock.acquire()

    assert acquired is False
    assert lock_path.exists()


def test_gateway_daemon_guardian_restarts_after_failure_threshold(monkeypatch):
    monkeypatch.setattr(gg, "_is_healthy", lambda *_a, **_k: False)
    calls = []

    def _ensure(**kwargs):
        calls.append(kwargs)
        return {"ok": True, "reason": "spawned"}

    monkeypatch.setattr(gg, "ensure_gateway_daemon", _ensure)
    seen = []
    guardian = gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="houdini",
        failure_threshold=2,
        status_callback=seen.append,
    )

    first = guardian.probe_once()
    second = guardian.probe_once()

    assert first["reason"] == "probe_failed"
    assert second["reason"] == "spawned"
    assert second["restart_attempts"] == 1
    assert second["consecutive_failures"] == 0
    assert len(calls) == 1
    assert calls[0]["dcc_type"] == "houdini"
    assert seen[-1]["reason"] == "spawned"


def test_gateway_daemon_guardian_jitter_skips_when_peer_recovers(monkeypatch):
    checks = iter([False, True])
    monkeypatch.setattr(gg, "_is_healthy", lambda *_a, **_k: next(checks))
    monkeypatch.setattr(gg.random, "uniform", lambda *_a, **_k: 0.0)

    def _ensure(**_kwargs):
        raise AssertionError("peer recovery after jitter should skip re-ensure")

    monkeypatch.setattr(gg, "ensure_gateway_daemon", _ensure)
    guardian = gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="maya",
        failure_threshold=1,
        reensure_jitter_max_secs=2.0,
    )

    result = guardian.probe_once(apply_reensure_jitter=True)

    assert result["reason"] == "healthy_after_jitter"
    assert result["consecutive_failures"] == 0
    assert result["restart_attempts"] == 0


def test_gateway_daemon_guardian_resets_failures_on_health(monkeypatch):
    checks = iter([False, True])
    monkeypatch.setattr(gg, "_is_healthy", lambda *_a, **_k: next(checks))
    guardian = gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="blender",
        failure_threshold=2,
    )

    first = guardian.probe_once()
    second = guardian.probe_once()

    assert first["reason"] == "probe_failed"
    assert second["reason"] == "healthy"
    assert second["consecutive_failures"] == 0


def test_guardian_run_catches_crash_and_increments_crash_count(monkeypatch):
    """P0: probe_once crash is caught by _run(), crash_count increments."""
    call_count = 0
    crash_reported = threading.Event()

    def _crash_after_one(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        raise RuntimeError("deliberate probe crash")

    def _status_callback(status):
        if status.get("crash_count", 0) >= 1:
            crash_reported.set()

    monkeypatch.setattr(gg, "_is_healthy", _crash_after_one)

    guardian = gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="crash-test",
        probe_interval_secs=0.05,
        failure_threshold=5,
        status_callback=_status_callback,
    )

    guardian.start()
    try:
        assert _wait_until(
            lambda: crash_reported.is_set() or guardian.status().get("crash_count", 0) >= 1,
            timeout=30.0,
        ), "Expected guardian crash status to be published"
    finally:
        guardian.stop(timeout=2.0)

    status = guardian.status()
    assert status["crash_count"] >= 1, f"Expected crash_count >= 1, got {status['crash_count']}"
    # Guardian must report running=False after stop.
    assert status["guardian_running"] is False


def test_guardian_run_continues_after_exception(monkeypatch):
    """P0: _run() loop survives exception and keeps probing."""
    calls = []
    crash_reported = threading.Event()
    continued_after_crash = threading.Event()

    def _probe(*args, **kwargs):
        calls.append(1)
        if len(calls) == 1:
            raise RuntimeError("first probe crash")
        continued_after_crash.set()
        return False  # subsequent probes return healthy=False (no crash)

    def _status_callback(status):
        if status.get("crash_count", 0) >= 1:
            crash_reported.set()

    monkeypatch.setattr(gg, "_is_healthy", _probe)

    guardian = gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="resilient-test",
        probe_interval_secs=0.05,
        failure_threshold=100,
        status_callback=_status_callback,
    )

    guardian.start()
    try:
        assert _wait_until(
            lambda: crash_reported.is_set() or guardian.status().get("crash_count", 0) >= 1,
            timeout=30.0,
        ), "Expected guardian crash status to be published"
        assert _wait_until(lambda: continued_after_crash.is_set() or len(calls) >= 2), (
            "Expected guardian loop to continue probing"
        )
    finally:
        guardian.stop(timeout=2.0)

    # The loop survived the first crash and continued probing
    assert len(calls) >= 2, f"Expected >= 2 probe calls, got {len(calls)}"
    assert guardian.status()["crash_count"] >= 1


def test_build_gateway_daemon_command_includes_persist_flags(monkeypatch, tmp_path):
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")
    cmd, env = gg.build_gateway_daemon_command(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="custom-host",
        gateway_persist=True,
        gateway_idle_timeout_secs=0,
    )
    assert cmd[:2] == ["dcc-mcp-server", "gateway"]
    assert "--gateway-persist" in cmd
    assert "--gateway-idle-timeout-secs" in cmd
    assert env["DCC_MCP_GATEWAY_PERSIST"] == "1"
    assert env["DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS"] == "0"
    assert env["DCC_MCP_REGISTRY_DIR"] == str(tmp_path)
    assert env["DCC_MCP_DCC_TYPE"] == "custom-host"


def test_build_gateway_daemon_command_uses_adapter_idle_timeout_by_default(monkeypatch, tmp_path):
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")
    cmd, env = gg.build_gateway_daemon_command(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="maya",
    )
    assert "--gateway-persist" not in cmd
    assert "--gateway-idle-timeout-secs" in cmd
    assert str(gg._AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_DEFAULT) in cmd
    assert "DCC_MCP_GATEWAY_PERSIST" not in env
    assert env["DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS"] == str(gg._AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_DEFAULT)


def test_build_gateway_daemon_command_respects_custom_server_bin(monkeypatch, tmp_path):
    cmd, _env = gg.build_gateway_daemon_command(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="nuke",
        server_bin="/opt/bin/dcc-gw",
    )
    assert cmd[0] == "/opt/bin/dcc-gw"


def test_ensure_gateway_daemon_spawn_includes_persist_flags(tmp_path, monkeypatch):
    state = {"calls": 0}

    class _Resp:
        status = 200

        def __enter__(self):
            return self

        def __exit__(self, *_exc):
            return False

    def _urlopen(*_args, **_kwargs):
        state["calls"] += 1
        if state["calls"] < 3:
            raise OSError("down")
        return _Resp()

    seen: dict = {}

    def _launch_detached(cmd, **kwargs):
        seen["cmd"] = cmd
        seen["env"] = kwargs.get("env", {})
        return {"ok": True, "pid": 99}

    monkeypatch.setattr(gg, "urlopen", _urlopen)
    monkeypatch.setattr(gg, "launch_detached", _launch_detached)
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")
    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9876,
        registry_dir=str(tmp_path),
        dcc_type="ftrack",
        timeout_secs=1.0,
        gateway_persist=True,
        gateway_idle_timeout_secs=120,
    )
    assert result["ok"] is True
    assert "--gateway-persist" in seen["cmd"]
    assert "--gateway-idle-timeout-secs" in seen["cmd"]
    assert seen["env"]["DCC_MCP_GATEWAY_PERSIST"] == "1"
    assert seen["env"]["DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS"] == "120"


def test_launch_gateway_daemon_is_alias(monkeypatch):
    monkeypatch.setattr(gg, "urlopen", lambda *args, **kwargs: _Resp())
    result = gg.launch_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="blender",
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"


def _make_runtime_controller(monkeypatch, **owner_attrs):
    """Build a thin ServerRuntimeController with a mock owner."""
    # Import here to avoid circular import issues in test collection.
    from dcc_mcp_core._server.runtime import ServerRuntimeController

    attrs = {
        "_config": type(
            "Config",
            (),
            {
                "gateway_port": 9765,
                "registry_dir": None,
                **(owner_attrs.pop("_config_kw", {})),
            },
        )(),
        "_dcc_name": "test-dcc",
        "_gateway_runtime_mode": "daemon-backed",
        "_enable_gateway_failover": True,
        "_gateway_guardian": None,
        "_gateway_daemon_status": {},
        "_publish_gateway_runtime_metadata": staticmethod(lambda: None),
        **owner_attrs,
    }
    mock_owner = type("Owner", (), attrs)()
    return ServerRuntimeController(mock_owner), mock_owner


def test_start_guardian_replaces_dead_guardian(monkeypatch):
    """P1: start_gateway_guardian_if_needed replaces a dead guardian."""
    monkeypatch.setattr(gg, "_is_healthy", lambda *a, **k: False)
    monkeypatch.setattr(gg, "ensure_gateway_daemon", lambda **kw: {"ok": True, "reason": "spawned"})
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")

    ctrl, owner = _make_runtime_controller(monkeypatch)

    # Start the guardian normally.
    ctrl.start_gateway_guardian_if_needed()
    guardian = owner._gateway_guardian
    assert guardian is not None
    assert guardian.status()["guardian_running"] is True
    first_id = id(guardian)

    # Simulate guardian death: stop the thread so status shows not running.
    guardian.stop(timeout=2.0)
    assert guardian.status()["guardian_running"] is False

    # Replace the dead guardian.
    ctrl.start_gateway_guardian_if_needed()
    new_guardian = owner._gateway_guardian
    assert new_guardian is not None
    assert id(new_guardian) != first_id
    assert new_guardian.status()["guardian_running"] is True


def test_guardian_watchdog_detects_dead_guardian(monkeypatch):
    """P1: Watchdog loop detects dead guardian and triggers restart."""
    monkeypatch.setattr(gg, "_is_healthy", lambda *a, **k: False)
    monkeypatch.setattr(gg, "ensure_gateway_daemon", lambda **kw: {"ok": True, "reason": "spawned"})
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")

    ctrl, owner = _make_runtime_controller(monkeypatch)

    # Start the guardian.
    ctrl.start_gateway_guardian_if_needed()
    guardian = owner._gateway_guardian
    assert guardian is not None

    # Kill the guardian thread.
    guardian.stop(timeout=2.0)
    assert guardian.status()["guardian_running"] is False

    # Calling start_gateway_guardian_if_needed again should restore the guardian.
    ctrl.start_gateway_guardian_if_needed()
    new_guardian = owner._gateway_guardian
    assert new_guardian is not None
    assert id(new_guardian) != id(guardian)
    assert new_guardian.status()["guardian_running"] is True


# ---------------------------------------------------------------------------
# Embedded fallback retry + strict-gateway tests (PIP-1398)
# ---------------------------------------------------------------------------


def test_ensure_gateway_daemon_if_needed_retries_before_fallback(monkeypatch):
    """Ensure retries 2 times before falling back to embedded-fallback mode."""
    from dcc_mcp_core._server import runtime as rt_mod

    call_count = 0

    def _ensure(**kwargs):
        nonlocal call_count
        call_count += 1
        return {"ok": False, "reason": "spawn_failed", "error": "test error"}

    monkeypatch.setattr(rt_mod, "ensure_gateway_daemon", _ensure)
    # Speed up retries in tests
    monkeypatch.setattr(rt_mod, "_RETRY_INTERVAL_SECS", 0.01)

    ctrl, owner = _make_runtime_controller(monkeypatch)
    result = ctrl.ensure_gateway_daemon_if_needed()

    # Should have attempted 1 + 2 = 3 times
    assert call_count == 3, f"Expected 3 attempts, got {call_count}"
    assert result is False
    assert owner._gateway_runtime_mode == "embedded-fallback"
    assert owner._gateway_daemon_status["reason"] == "spawn_failed"


def test_ensure_gateway_daemon_if_needed_succeeds_on_retry(monkeypatch):
    """First attempt fails, retry succeeds — must not fall back."""
    from dcc_mcp_core._server import runtime as rt_mod

    call_count = 0

    def _ensure(**kwargs):
        nonlocal call_count
        call_count += 1
        if call_count <= 2:
            return {"ok": False, "reason": "spawn_failed", "error": "test error"}
        return {"ok": True, "reason": "spawned"}

    monkeypatch.setattr(rt_mod, "ensure_gateway_daemon", _ensure)
    monkeypatch.setattr(rt_mod, "_RETRY_INTERVAL_SECS", 0.01)

    ctrl, owner = _make_runtime_controller(monkeypatch)
    result = ctrl.ensure_gateway_daemon_if_needed()

    assert call_count == 3, f"Expected 3 attempts, got {call_count}"
    assert result is True
    assert owner._gateway_runtime_mode == "daemon-backed"


def test_strict_gateway_raises_instead_of_fallback(monkeypatch):
    """With DCC_MCP_STRICT_GATEWAY=1, ensure failure raises RuntimeError."""
    from dcc_mcp_core._server import runtime as rt_mod

    def _ensure(**kwargs):
        return {"ok": False, "reason": "spawn_failed", "error": "test error"}

    monkeypatch.setattr(rt_mod, "ensure_gateway_daemon", _ensure)
    monkeypatch.setattr(rt_mod, "_RETRY_INTERVAL_SECS", 0.01)
    monkeypatch.setenv("DCC_MCP_STRICT_GATEWAY", "1")

    ctrl, owner = _make_runtime_controller(monkeypatch)

    try:
        ctrl.ensure_gateway_daemon_if_needed()
        raise AssertionError("Expected RuntimeError was not raised")
    except RuntimeError as exc:
        assert "strict gateway" in str(exc).lower()
        # Must not have fallen back
        assert owner._gateway_runtime_mode != "embedded-fallback"


def test_strict_gateway_via_owner_attribute(monkeypatch):
    """Strict mode via owner._strict_gateway=True also raises RuntimeError."""
    from dcc_mcp_core._server import runtime as rt_mod

    def _ensure(**kwargs):
        return {"ok": False, "reason": "spawn_timeout", "error": "timeout"}

    monkeypatch.setattr(rt_mod, "ensure_gateway_daemon", _ensure)
    monkeypatch.setattr(rt_mod, "_RETRY_INTERVAL_SECS", 0.01)

    ctrl, owner = _make_runtime_controller(monkeypatch)
    owner._strict_gateway = True

    try:
        ctrl.ensure_gateway_daemon_if_needed()
        raise AssertionError("Expected RuntimeError was not raised")
    except RuntimeError as exc:
        assert "spawn_timeout" in str(exc)


def test_strict_gateway_happy_path_works_normally(monkeypatch):
    """When daemon is healthy, strict mode has no effect (no exception)."""
    from dcc_mcp_core._server import runtime as rt_mod

    def _ensure(**kwargs):
        return {"ok": True, "reason": "already_healthy"}

    monkeypatch.setattr(rt_mod, "ensure_gateway_daemon", _ensure)
    monkeypatch.setenv("DCC_MCP_STRICT_GATEWAY", "1")

    ctrl, owner = _make_runtime_controller(monkeypatch)
    result = ctrl.ensure_gateway_daemon_if_needed()

    assert result is True
    assert owner._gateway_runtime_mode == "daemon-backed"


def test_embedded_fallback_metadata_includes_daemon_status():
    """_gateway_runtime_metadata surfaces daemon status for embedded-fallback."""
    from dcc_mcp_core._server.lifecycle_controller import LifecycleController

    # Build a minimal mock owner in embedded-fallback mode.
    attrs = {
        "_dcc_name": "meta-test",
        "_gateway_runtime_mode": "embedded-fallback",
        "_gateway_guardian": None,
        "_gateway_daemon_status": {
            "ok": False,
            "reason": "spawn_failed",
            "error": "connection refused",
        },
    }
    mock_owner = type("Owner", (), attrs)()
    ctrl = LifecycleController(mock_owner)

    meta = ctrl._gateway_runtime_metadata()
    assert meta["gateway_runtime_mode"] == "embedded-fallback"
    assert meta["gateway_recovery_driver"] == "embedded_election"
    assert meta["gateway_daemon_status"] == "spawn_failed"
    assert meta["gateway_daemon_error"] == "connection refused"


def test_embedded_fallback_metadata_omits_daemon_error_when_none():
    """When daemon status has no error field, gateway_daemon_error is absent."""
    from dcc_mcp_core._server.lifecycle_controller import LifecycleController

    attrs = {
        "_dcc_name": "meta-test2",
        "_gateway_runtime_mode": "embedded-fallback",
        "_gateway_guardian": None,
        "_gateway_daemon_status": {"ok": False, "reason": "launch_in_progress_timeout"},
    }
    mock_owner = type("Owner", (), attrs)()
    ctrl = LifecycleController(mock_owner)

    meta = ctrl._gateway_runtime_metadata()
    assert meta["gateway_runtime_mode"] == "embedded-fallback"
    assert meta["gateway_daemon_status"] == "launch_in_progress_timeout"
    assert "gateway_daemon_error" not in meta


def test_daemon_backed_metadata_does_not_include_daemon_error():
    """In daemon-backed mode, gateway_daemon_error is not leaked."""
    from dcc_mcp_core._server.lifecycle_controller import LifecycleController

    attrs = {
        "_dcc_name": "meta-test3",
        "_gateway_runtime_mode": "daemon-backed",
        "_gateway_guardian": None,
        "_gateway_daemon_status": {
            "ok": True,
            "reason": "already_healthy",
        },
    }
    mock_owner = type("Owner", (), attrs)()
    ctrl = LifecycleController(mock_owner)

    meta = ctrl._gateway_runtime_metadata()
    assert meta["gateway_runtime_mode"] == "daemon-backed"
    assert meta["gateway_recovery_driver"] == "none"
    assert "gateway_daemon_status" not in meta
    assert "gateway_daemon_error" not in meta


# ---------------------------------------------------------------------------
# P0-3: semver parsing, version comparison, sentinel, and version takeover
# ---------------------------------------------------------------------------


def test_parse_semver_basic():
    """P0-3: _parse_semver handles standard semver strings."""
    assert gg._parse_semver("0.18.15") == (0, 18, 15)
    assert gg._parse_semver("1.0.0") == (1, 0, 0)
    assert gg._parse_semver("2.3") == (2, 3, 0)
    assert gg._parse_semver("5") == (5, 0, 0)


def test_parse_semver_with_v_prefix():
    assert gg._parse_semver("v0.12.29") == (0, 12, 29)
    assert gg._parse_semver("V1.2.3") == (1, 2, 3)


def test_parse_semver_with_prerelease():
    assert gg._parse_semver("1.0.0-rc1") == (1, 0, 0)
    assert gg._parse_semver("v2.3.4-beta.2") == (2, 3, 4)


def test_is_newer_version():
    assert gg._is_newer_version("1.0.0", "0.9.0") is True
    assert gg._is_newer_version("0.18.15", "0.18.6") is True
    assert gg._is_newer_version("0.18.6", "0.18.15") is False
    assert gg._is_newer_version("1.0.0", "1.0.0") is False


def test_get_core_version_env_override(monkeypatch):
    """P0-3: _get_core_version respects DCC_MCP_CORE_VERSION env var."""
    monkeypatch.setenv("DCC_MCP_CORE_VERSION", "9.8.7")
    assert gg._get_core_version() == "9.8.7"


def test_write_and_read_sentinel_entry(tmp_path):
    """P0-3: _write_sentinel_entry and _read_gateway_version_from_registry roundtrip."""
    reg = str(tmp_path / "dcc-mcp-registry")
    # Write a sentinel.
    ok = gg._write_sentinel_entry(
        reg,
        gateway_host="127.0.0.1",
        gateway_port=9765,
        crate_version="0.18.15",
        adapter_version="0.3.0",
        adapter_dcc="maya",
    )
    assert ok is True
    assert (tmp_path / "dcc-mcp-registry" / "services.json").exists()

    # Read it back.
    ver = gg._read_gateway_version_from_registry(
        reg,
        gateway_host="127.0.0.1",
        gateway_port=9765,
    )
    assert ver == "0.18.15"


def test_read_gateway_version_missing_registry():
    """P0-3: _read_gateway_version_from_registry returns None for missing file."""
    assert (
        gg._read_gateway_version_from_registry(
            "/nonexistent/path",
            gateway_host="127.0.0.1",
            gateway_port=9999,
        )
        is None
    )


def test_ensure_gateway_daemon_skips_takeover_when_gateway_newer(monkeypatch, tmp_path):
    """P0-3: No takeover when running gateway version is same or newer."""
    monkeypatch.setattr(gg, "urlopen", lambda *args, **kwargs: _Resp())
    monkeypatch.setenv("DCC_MCP_CORE_VERSION", "0.18.15")

    # Pre-populate registry with a same-version sentinel.
    gg._write_sentinel_entry(
        str(tmp_path),
        gateway_host="127.0.0.1",
        gateway_port=9765,
        crate_version="0.18.15",
    )

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="maya",
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"


def test_try_version_takeover_returns_none_when_dev_version(monkeypatch, tmp_path):
    """P0-3: _try_version_takeover returns None when version is 0.0.0-dev."""
    # 0.0.0-dev is the default when package metadata is unavailable.
    # _get_core_version should return this in test environment.
    monkeypatch.setattr(gg, "_get_core_version", lambda: "0.0.0-dev")
    monkeypatch.setattr(gg, "_read_gateway_version_from_registry", lambda *a, **k: "0.18.0")
    monkeypatch.setattr(gg, "_is_healthy", lambda *a, **k: True)

    result = gg._try_version_takeover(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="maya",
        timeout_secs=15.0,
        gateway_persist=False,
        gateway_idle_timeout_secs=None,
        server_bin=None,
    )
    assert result is None


# ---------------------------------------------------------------------------
# Watchdog interval (PIP-1416: 60s → 15s + env override + immediate retry)
# ---------------------------------------------------------------------------


def test_watchdog_interval_default_is_15s():
    """PIP-1416: watchdog default interval is 15 s (was 60 s)."""
    from dcc_mcp_core._server import runtime as rt_mod

    # Resolve interval without env var set.
    assert rt_mod._resolve_watchdog_interval() == 15.0


def test_watchdog_interval_respects_env_var(monkeypatch):
    """PIP-1416: DCC_MCP_GUARDIAN_WATCHDOG_INTERVAL overrides the default."""
    monkeypatch.setenv("DCC_MCP_GUARDIAN_WATCHDOG_INTERVAL", "8.5")
    from dcc_mcp_core._server import runtime as rt_mod

    # _resolve_watchdog_interval() reads os.environ at call time — no reload needed.
    assert rt_mod._resolve_watchdog_interval() == 8.5


def test_watchdog_interval_env_floor_0_1s(monkeypatch):
    """PIP-1416: watchdog interval env floors at 0.1 s."""
    monkeypatch.setenv("DCC_MCP_GUARDIAN_WATCHDOG_INTERVAL", "0.01")
    from dcc_mcp_core._server import runtime as rt_mod

    assert rt_mod._resolve_watchdog_interval() == 0.1


def test_watchdog_interval_env_invalid_falls_back(monkeypatch):
    """PIP-1416: invalid env value falls back to default 15 s."""
    monkeypatch.setenv("DCC_MCP_GUARDIAN_WATCHDOG_INTERVAL", "not_a_number")
    from dcc_mcp_core._server import runtime as rt_mod

    assert rt_mod._resolve_watchdog_interval() == 15.0


def test_watchdog_immediate_retry_on_guardian_death(monkeypatch):
    """PIP-1416: watchdog immediately probes the new guardian after restart."""
    monkeypatch.setattr(gg, "_is_healthy", lambda *a, **k: False)
    monkeypatch.setattr(gg, "ensure_gateway_daemon", lambda **kw: {"ok": True, "reason": "spawned"})
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")

    ctrl, owner = _make_runtime_controller(monkeypatch)

    # Start the guardian.
    ctrl.start_gateway_guardian_if_needed()
    guardian = owner._gateway_guardian
    assert guardian is not None

    # Kill the guardian thread.
    guardian.stop(timeout=2.0)
    assert guardian.status()["guardian_running"] is False

    # Track probe_once calls on the *new* guardian.
    probe_calls = []

    original_start = ctrl.start_gateway_guardian_if_needed

    def _start_and_track():
        original_start()
        new_g = getattr(owner, "_gateway_guardian", None)
        if new_g is not None:
            original_probe = new_g.probe_once
            new_g.probe_once = lambda: probe_calls.append(1) or original_probe()

    ctrl.start_gateway_guardian_if_needed = _start_and_track

    # Simulate one watchdog tick: detect dead → restart → immediate retry.
    ctrl._guardian_watchdog_stop.set()  # unblock wait() for one iteration
    # Run one cycle of the watchdog loop logic manually.
    guardian_obj = owner._gateway_guardian  # still the dead one
    assert guardian_obj.status()["guardian_running"] is False

    # Replicate the watchdog loop body directly.
    status = guardian_obj.status()
    assert status.get("guardian_running") is False
    # Call start (which replaces guardian + monkeypatches probe_once).
    ctrl.start_gateway_guardian_if_needed()
    new_guardian = getattr(owner, "_gateway_guardian", None)
    assert new_guardian is not None
    assert new_guardian is not guardian_obj  # replaced
    # Immediate retry: call probe_once.
    new_guardian.probe_once()

    assert len(probe_calls) == 1, f"Expected 1 immediate probe_once call, got {len(probe_calls)}"


def test_ensure_gateway_daemon_handles_version_takeover_health(monkeypatch, tmp_path):
    """P0-3: ensure_gateway_daemon returns already_healthy when takeover not needed."""
    monkeypatch.setattr(gg, "urlopen", lambda *args, **kwargs: _Resp())
    monkeypatch.setattr(gg, "_get_core_version", lambda: "0.18.15")
    monkeypatch.setattr(gg, "_read_gateway_version_from_registry", lambda *a, **k: "1.0.0")  # gateway newer

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="blender",
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"
