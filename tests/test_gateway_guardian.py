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
