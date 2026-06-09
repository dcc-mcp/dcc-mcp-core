from __future__ import annotations

import os
import time

import dcc_mcp_core._server.gateway_guardian as gg


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


def test_ensure_gateway_daemon_spawn_failure_returns_embedded_fallback_reason(monkeypatch):
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
        registry_dir=None,
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

    def _crash_after_one(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        raise RuntimeError("deliberate probe crash")

    monkeypatch.setattr(gg, "_is_healthy", _crash_after_one)

    guardian = gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="crash-test",
        probe_interval_secs=0.05,
        failure_threshold=5,
    )

    guardian.start()
    time.sleep(0.2)
    guardian.stop(timeout=2.0)

    status = guardian.status()
    assert status["crash_count"] >= 1, f"Expected crash_count >= 1, got {status['crash_count']}"
    # Guardian must report running=False after stop.
    assert status["guardian_running"] is False


def test_guardian_run_continues_after_exception(monkeypatch):
    """P0: _run() loop survives exception and keeps probing."""
    calls = []

    def _probe(*args, **kwargs):
        calls.append(1)
        if len(calls) == 1:
            raise RuntimeError("first probe crash")
        return False  # subsequent probes return healthy=False (no crash)

    monkeypatch.setattr(gg, "_is_healthy", _probe)

    guardian = gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="resilient-test",
        probe_interval_secs=0.05,
        failure_threshold=5,
    )

    guardian.start()
    time.sleep(0.5)
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


def test_build_gateway_daemon_command_omits_persist_by_default(monkeypatch, tmp_path):
    monkeypatch.setattr(gg, "_resolve_server_bin", lambda: "dcc-mcp-server")
    cmd, env = gg.build_gateway_daemon_command(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(tmp_path),
        dcc_type="maya",
    )
    assert "--gateway-persist" not in cmd
    assert "--gateway-idle-timeout-secs" not in cmd
    assert "DCC_MCP_GATEWAY_PERSIST" not in env
    assert "DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS" not in env


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

# ── Version-aware takeover tests ────────────────────────────────────────────


def test_parse_semver_strips_leading_v():
    assert gg._parse_semver("v0.12.29") == (0, 12, 29)
    assert gg._parse_semver("V1.2.3") == (1, 2, 3)


def test_parse_semver_ignores_prerelease():
    assert gg._parse_semver("1.0.0-rc1") == (1, 0, 0)
    assert gg._parse_semver("v2.1.0-alpha.3") == (2, 1, 0)


def test_parse_semver_missing_components_default_to_zero():
    assert gg._parse_semver("1") == (1, 0, 0)
    assert gg._parse_semver("1.2") == (1, 2, 0)
    assert gg._parse_semver("") == (0, 0, 0)


def test_is_newer_version_numeric_comparison():
    assert gg._is_newer_version("0.12.29", "0.12.6") is True
    assert gg._is_newer_version("0.12.6", "0.12.29") is False
    assert gg._is_newer_version("1.0.0", "1.0.0") is False
    assert gg._is_newer_version("2.0.0", "1.99.99") is True
    assert gg._is_newer_version("0.13.0", "0.12.29") is True


def test_system_time_now_json_format():
    ts = gg._system_time_now_json()
    assert isinstance(ts, dict)
    assert "secs_since_epoch" in ts
    assert "nanos_since_epoch" in ts
    assert isinstance(ts["secs_since_epoch"], int)
    assert isinstance(ts["nanos_since_epoch"], int)
    assert ts["secs_since_epoch"] > 0
    assert 0 <= ts["nanos_since_epoch"] < 1_000_000_000


def test_sentry_stale_fresh_entry():
    now = gg._system_time_now_json()
    entry = {
        "dcc_type": "__gateway__",
        "last_heartbeat": dict(now),
    }
    assert gg._sentry_stale(entry, 30.0) is False


def test_sentry_stale_outdated_entry():
    now = time.time()
    old = {
        "secs_since_epoch": int(now - 120),
        "nanos_since_epoch": 0,
    }
    entry = {
        "dcc_type": "__gateway__",
        "last_heartbeat": old,
    }
    assert gg._sentry_stale(entry, 30.0) is True


def test_sentry_stale_missing_heartbeat():
    assert gg._sentry_stale({"dcc_type": "__gateway__"}, 30.0) is True
    assert gg._sentry_stale({"dcc_type": "__gateway__", "last_heartbeat": None}, 30.0) is True


def test_write_and_cleanup_takeover_sentinel(tmp_path):
    reg = tmp_path / "registry"
    host, port = "127.0.0.1", 9765
    version = "0.18.0"

    ok = gg._write_takeover_sentinel(reg, host, port, version)
    assert ok is True

    entries = gg._read_services_json(reg)
    sentinels = [e for e in entries if e.get("dcc_type") == "__gateway__"]
    assert len(sentinels) == 1
    assert sentinels[0]["host"] == host
    assert sentinels[0]["port"] == port
    assert sentinels[0]["version"] == version
    assert "last_heartbeat" in sentinels[0]
    assert "registered_at" in sentinels[0]

    # Writing again with a new version replaces the old sentinel.
    ok = gg._write_takeover_sentinel(reg, host, port, "0.19.0")
    assert ok is True
    entries = gg._read_services_json(reg)
    sentinels = [e for e in entries if e.get("dcc_type") == "__gateway__"]
    assert len(sentinels) == 1
    assert sentinels[0]["version"] == "0.19.0"

    # Cleanup removes the sentinel.
    ok = gg._cleanup_takeover_sentinel(reg, host, port)
    assert ok is True
    entries = gg._read_services_json(reg)
    sentinels = [e for e in entries if e.get("dcc_type") == "__gateway__"]
    assert len(sentinels) == 0


def test_write_takeover_sentinel_with_adapter_info(tmp_path):
    reg = tmp_path / "registry"
    ok = gg._write_takeover_sentinel(
        reg, "127.0.0.1", 9765, "0.18.0",
        adapter_version="0.3.0",
        adapter_dcc="maya",
    )
    assert ok is True
    entries = gg._read_services_json(reg)
    sentinel = entries[0]
    assert sentinel["adapter_version"] == "0.3.0"
    assert sentinel["adapter_dcc"] == "maya"


def test_get_core_version_from_env(monkeypatch):
    monkeypatch.setenv("DCC_MCP_CORE_VERSION", "1.2.3")
    assert gg._get_core_version() == "1.2.3"


def test_get_core_version_empty_env_returns_none(monkeypatch):
    monkeypatch.setenv("DCC_MCP_CORE_VERSION", "")
    # Without a real package install, importlib.metadata will fail.
    # _get_core_version tries the env first; empty → tries importlib → tries dcc_mcp_core.
    # In test env, dcc_mcp_core should be importable and have __version__.
    v = gg._get_core_version()
    assert v is not None and len(v) > 0


def test_try_version_takeover_no_sentinel_returns_already_healthy(tmp_path, monkeypatch):
    """When services.json has no sentinel, _try_version_takeover should not trigger."""
    reg = tmp_path / "registry"
    monkeypatch.setattr(gg, "_get_core_version", lambda: "0.18.0")

    result = gg._try_version_takeover(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(reg),
        dcc_type="blender",
        timeout_secs=5.0,
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"


def test_try_version_takeover_when_newer(tmp_path, monkeypatch):
    """When we're newer than the running sentinel, takeover should trigger."""
    reg = tmp_path / "registry"
    monkeypatch.setattr(gg, "_get_core_version", lambda: "0.19.0")

    # Write an older sentinel first.
    now = gg._system_time_now_json()
    services = tmp_path / "registry" / "services.json"
    services.parent.mkdir(parents=True, exist_ok=True)
    import json as _json
    with services.open("w") as f:
        _json.dump([{
            "dcc_type": "__gateway__",
            "instance_id": "fake-old-gateway",
            "host": "127.0.0.1",
            "port": 9765,
            "version": "0.18.0",
            "status": "available",
            "last_heartbeat": dict(now),
            "registered_at": dict(now),
        }], f)

    health = iter([True, True, False])  # healthy, still healthy, then yields
    monkeypatch.setattr(gg, "_is_healthy", lambda *a, **k: next(health))

    seen_spawn = []

    def _fake_spawn(cmd, **kw):
        seen_spawn.append(cmd)
        return {"ok": True, "pid": 9999}

    monkeypatch.setattr(gg, "launch_detached", _fake_spawn)
    monkeypatch.setattr(gg, "_wait_gateway_ready", lambda *a, **k: True)

    result = gg._try_version_takeover(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(reg),
        dcc_type="blender",
        timeout_secs=5.0,
    )
    assert result["ok"] is True
    assert result["reason"] == "version_takeover_spawned"
    assert result.get("version") == "0.19.0"
    assert len(seen_spawn) == 1

    # Sentinel should be cleaned up after successful takeover.
    entries = gg._read_services_json(reg)
    sentinels = [e for e in entries if e.get("dcc_type") == "__gateway__"]
    assert len(sentinels) == 0


def test_try_version_takeover_when_running_is_newer(tmp_path, monkeypatch):
    """When running gateway sentinel is newer than us, no takeover."""
    reg = tmp_path / "registry"
    monkeypatch.setattr(gg, "_get_core_version", lambda: "0.17.0")

    now = gg._system_time_now_json()
    services = tmp_path / "registry" / "services.json"
    services.parent.mkdir(parents=True, exist_ok=True)
    import json as _json
    with services.open("w") as f:
        _json.dump([{
            "dcc_type": "__gateway__",
            "instance_id": "fake-newer-gateway",
            "host": "127.0.0.1",
            "port": 9765,
            "version": "0.18.0",
            "status": "available",
            "last_heartbeat": dict(now),
            "registered_at": dict(now),
        }], f)

    # _is_healthy should NOT be called because takeover is skipped early.
    result = gg._try_version_takeover(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(reg),
        dcc_type="blender",
        timeout_secs=5.0,
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"


def test_try_version_takeover_stale_sentinel_is_ignored(tmp_path, monkeypatch):
    """A stale sentinel should not prevent takeover."""
    reg = tmp_path / "registry"
    monkeypatch.setattr(gg, "_get_core_version", lambda: "0.19.0")

    now = time.time()
    old = {"secs_since_epoch": int(now - 120), "nanos_since_epoch": 0}
    services = tmp_path / "registry" / "services.json"
    services.parent.mkdir(parents=True, exist_ok=True)
    import json as _json
    with services.open("w") as f:
        _json.dump([{
            "dcc_type": "__gateway__",
            "instance_id": "fake-stale-gateway",
            "host": "127.0.0.1",
            "port": 9765,
            "version": "0.20.0",  # newer, but stale
            "status": "available",
            "last_heartbeat": dict(old),
            "registered_at": dict(old),
        }], f)

    # No fresh sentinel → should_takeover stays False → already_healthy
    result = gg._try_version_takeover(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(reg),
        dcc_type="blender",
        timeout_secs=5.0,
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"


def test_try_version_takeover_old_gateway_timeout(tmp_path, monkeypatch):
    """When old gateway does not yield within timeout, return already_healthy."""
    reg = tmp_path / "registry"
    monkeypatch.setattr(gg, "_get_core_version", lambda: "0.19.0")

    now = gg._system_time_now_json()
    services = tmp_path / "registry" / "services.json"
    services.parent.mkdir(parents=True, exist_ok=True)
    import json as _json
    with services.open("w") as f:
        _json.dump([{
            "dcc_type": "__gateway__",
            "instance_id": "stubborn-gateway",
            "host": "127.0.0.1",
            "port": 9765,
            "version": "0.18.0",
            "status": "available",
            "last_heartbeat": dict(now),
            "registered_at": dict(now),
        }], f)

    # Old gateway stays healthy (never yields).
    monkeypatch.setattr(gg, "_is_healthy", lambda *a, **k: True)

    # Override wait timeout to a short value for test speed.
    monkeypatch.setattr(gg, "_VERSION_TAKEOVER_WAIT_SECS", 0.1)

    result = gg._try_version_takeover(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=str(reg),
        dcc_type="blender",
        timeout_secs=5.0,
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"
    assert result.get("version_takeover_timeout") is True


def test_ensure_gateway_daemon_version_takeover_disabled(monkeypatch):
    """When enable_version_takeover=False, version check is skipped entirely."""
    called_takeover = []

    def _fake_takeover(**kw):
        called_takeover.append(1)
        return {"ok": True, "reason": "already_healthy"}

    monkeypatch.setattr(gg, "_is_healthy", lambda *a, **k: True)
    monkeypatch.setattr(gg, "_try_version_takeover", _fake_takeover)

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="blender",
        enable_version_takeover=False,
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"
    assert len(called_takeover) == 0


def test_ensure_gateway_daemon_version_takeover_enabled(monkeypatch):
    """When enable_version_takeover=True (default), version check is called."""
    monkeypatch.setattr(gg, "_is_healthy", lambda *a, **k: True)
    monkeypatch.setattr(
        gg,
        "_try_version_takeover",
        lambda **kw: {"ok": True, "reason": "already_healthy"},
    )

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9765,
        registry_dir=None,
        dcc_type="blender",
    )
    assert result["ok"] is True
    assert result["reason"] == "already_healthy"


def test_ensure_gateway_daemon_takeover_integration(tmp_path, monkeypatch):
    """Integration test: ensure_gateway_daemon with version takeover."""
    reg = tmp_path / "registry"

    # Simulate an older gateway running.
    now = gg._system_time_now_json()
    services = tmp_path / "registry" / "services.json"
    services.parent.mkdir(parents=True, exist_ok=True)
    import json as _json
    with services.open("w") as f:
        _json.dump([{
            "dcc_type": "__gateway__",
            "instance_id": "old-integration-gw",
            "host": "127.0.0.1",
            "port": 9876,
            "version": "0.17.0",
            "status": "available",
            "last_heartbeat": dict(now),
            "registered_at": dict(now),
        }], f)

    # We have a newer version.
    monkeypatch.setattr(gg, "_get_core_version", lambda: "0.18.0")

    # Gateway health: first call True → second call True → third call False (yield)
    health_checks = iter([True, True, False])

    def _health(host, port, **kw):
        val = next(health_checks)
        return val

    monkeypatch.setattr(gg, "_is_healthy", _health)

    seen_spawn = []

    def _fake_spawn(cmd, **kw):
        seen_spawn.append(cmd)
        return {"ok": True, "pid": 8888}

    monkeypatch.setattr(gg, "launch_detached", _fake_spawn)
    monkeypatch.setattr(gg, "_wait_gateway_ready", lambda *a, **k: True)

    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=9876,
        registry_dir=str(reg),
        dcc_type="houdini",
    )
    assert result["ok"] is True
    assert result["reason"] == "version_takeover_spawned"
    assert len(seen_spawn) == 1
