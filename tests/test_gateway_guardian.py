from __future__ import annotations

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

    def _popen(cmd, **kwargs):
        seen["cmd"] = cmd
        seen["env"] = kwargs.get("env", {})
        return object()

    monkeypatch.setattr(gg, "urlopen", _urlopen)
    monkeypatch.setattr(gg.subprocess, "Popen", _popen)
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

    def _boom(*_args, **_kwargs):
        raise RuntimeError("spawn fail")

    monkeypatch.setattr(gg.subprocess, "Popen", _boom)
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
        gg.subprocess,
        "Popen",
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
