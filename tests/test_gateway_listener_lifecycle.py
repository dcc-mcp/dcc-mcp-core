"""Listener intent survives adapter-owned launch and recovery boundaries (#2699).

All daemon/election calls are intercepted. Wildcard intent is inspected only
as configuration/argv; these tests never bind a wildcard socket or touch a
resident gateway.
"""

from __future__ import annotations

from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer
from dcc_mcp_core._server import gateway_guardian as gg
from dcc_mcp_core._server import runtime as rt
from dcc_mcp_core._server.config import build_mcp_http_config
from dcc_mcp_core._server.lifecycle_controller import LifecycleController
from dcc_mcp_core.server import DccServerOptions


def _owner(tmp_path, dcc_name="blender", **kwargs):
    options = DccServerOptions.from_env(dcc_name, tmp_path, gateway_port=19765, **kwargs)
    config = build_mcp_http_config(options, package_version="test", version_provider=lambda: "test")
    return SimpleNamespace(
        _config=config,
        _dcc_name=dcc_name,
        _enable_gateway_failover=True,
        _strict_gateway=False,
        _gateway_guardian=None,
        _gateway_election=None,
        _gateway_runtime_mode="unknown",
        _gateway_daemon_status={},
        _publish_gateway_runtime_metadata=Mock(),
        is_gateway=False,
    )


@pytest.mark.parametrize("dcc_name", ["blender", "photoshop"])
@pytest.mark.parametrize("remote_host,remote_port", [("127.0.0.1", 0), ("127.0.0.1", 19766), ("0.0.0.0", 19766)])
def test_all_runtime_ensure_attempts_preserve_policy(tmp_path, monkeypatch, dcc_name, remote_host, remote_port):
    owner = _owner(tmp_path, dcc_name, gateway_remote_host=remote_host, gateway_remote_port=remote_port)
    seen = []
    monkeypatch.setattr(rt.time, "sleep", lambda _: None)
    monkeypatch.setattr(rt, "ensure_gateway_daemon", lambda **kw: seen.append(kw) or {"ok": False, "reason": "test"})
    assert rt.ServerRuntimeController(owner).ensure_gateway_daemon_if_needed() is False
    assert owner._gateway_runtime_mode == "embedded-fallback"
    assert len(seen) == 3
    for call in seen:
        assert call["gateway_remote_host"] == remote_host
        assert call["gateway_remote_port"] == remote_port
    assert owner._config.gateway_remote_host == remote_host
    assert owner._config.gateway_remote_port == remote_port


@pytest.mark.parametrize("remote_port", [0, 19766])
def test_spawn_command_overrides_conflicting_environment(tmp_path, monkeypatch, remote_port):
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_HOST", "0.0.0.0")
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_PORT", "59765")
    monkeypatch.setattr(gg, "_is_application_ready", lambda *_a, **_k: False)
    monkeypatch.setattr(gg, "_wait_gateway_ready", lambda *_a, **_k: True)
    launches = []
    monkeypatch.setattr(gg, "launch_detached", lambda cmd, **kw: launches.append((cmd, kw)) or {"ok": True})
    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=19765,
        registry_dir=str(tmp_path),
        dcc_type="photoshop",
        gateway_remote_host="127.0.0.1",
        gateway_remote_port=remote_port,
        server_bin="test-owned-server",
    )
    assert result["ok"] is True
    assert len(launches) == 1
    cmd, kwargs = launches[0]
    assert cmd[cmd.index("--remote-host") + 1] == "127.0.0.1"
    assert cmd[cmd.index("--remote-port") + 1] == str(remote_port)
    assert kwargs["env"]["DCC_MCP_GATEWAY_REMOTE_HOST"] == "127.0.0.1"
    assert kwargs["env"]["DCC_MCP_GATEWAY_REMOTE_PORT"] == str(remote_port)


def test_runtime_guardian_receives_resolved_policy(tmp_path, monkeypatch):
    owner = _owner(tmp_path, gateway_remote_port=0)
    owner._gateway_runtime_mode = "daemon-backed"
    seen = []
    monkeypatch.setattr(
        rt, "GatewayDaemonGuardian", lambda **kw: seen.append(kw) or SimpleNamespace(start=lambda: False)
    )
    rt.ServerRuntimeController(owner).start_gateway_guardian_if_needed()
    assert seen[0]["gateway_remote_host"] == "127.0.0.1"
    assert seen[0]["gateway_remote_port"] == 0


@pytest.mark.parametrize("healthy", [False, True])
def test_guardian_restart_and_version_checks_keep_policy(tmp_path, monkeypatch, healthy):
    calls = []
    monkeypatch.setattr(gg, "_is_application_ready", lambda *_a, **_k: healthy)
    monkeypatch.setattr(gg, "ensure_gateway_daemon", lambda **kw: calls.append(kw) or {"ok": True})
    monkeypatch.setattr(gg, "_try_version_takeover", lambda **kw: calls.append(kw) or None)
    guardian = gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1",
        gateway_port=19765,
        registry_dir=str(tmp_path),
        dcc_type="blender",
        gateway_remote_host="127.0.0.1",
        gateway_remote_port=0,
        failure_threshold=1,
    )
    guardian.probe_once()
    assert calls[0]["gateway_remote_host"] == "127.0.0.1"
    assert calls[0]["gateway_remote_port"] == 0


@pytest.mark.parametrize(
    "remote_host,remote_port", [(None, None), ("127.0.0.1", 0), ("127.0.0.1", 19766), ("0.0.0.0", 19766)]
)
def test_promotion_reuses_resolved_native_configuration(tmp_path, remote_host, remote_port):
    owner = _owner(tmp_path, gateway_remote_host=remote_host, gateway_remote_port=remote_port)
    config = owner._config
    original = Mock()
    owner._handle = original
    received = []
    owner._server = SimpleNamespace(start=lambda: received.append(config) or SimpleNamespace(is_gateway=True))
    assert LifecycleController(owner)._upgrade_to_gateway() is True
    original.shutdown.assert_called_once_with()
    assert received == [config]
    assert config.gateway_remote_host == (remote_host if remote_host is not None else "127.0.0.1")
    assert config.gateway_remote_port == (remote_port if remote_port is not None else 0)


@pytest.mark.parametrize("election", [None, SimpleNamespace(get_status=lambda: {})])
def test_diagnostics_distinguish_configuration_from_observed_state(tmp_path, election):
    owner = _owner(tmp_path, gateway_remote_port=0)
    owner._gateway_election = election
    owner._gateway_runtime_mode = "daemon-backed"
    owner._gateway_daemon_status = {"ok": True, "reason": "already_healthy"}
    status = LifecycleController(owner).get_gateway_election_status()
    assert status["enabled"] is True
    assert status["remote_listener"] == {
        "configured_host": "127.0.0.1",
        "configured_port": 0,
        "configured_enabled": False,
        "scope": "adapter_started_gateways",
        "effective_state": "unverified",
    }


def test_sidecar_environment_preserves_explicit_zero(tmp_path, monkeypatch):
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_HOST", "0.0.0.0")
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_PORT", "59765")
    owner = _owner(tmp_path, gateway_remote_host="127.0.0.1", gateway_remote_port=0)
    sidecar = SidecarBackedSkillServer("blender", owner._config, host_rpc="127.0.0.1:1")
    env = sidecar._launch_environment()
    assert env["DCC_MCP_GATEWAY_REMOTE_HOST"] == "127.0.0.1"
    assert env["DCC_MCP_GATEWAY_REMOTE_PORT"] == "0"


def test_resident_healthy_gateway_is_not_reconfigured(tmp_path, monkeypatch):
    monkeypatch.setattr(gg, "_is_application_ready", lambda *_a, **_k: True)
    monkeypatch.setattr(gg, "_get_core_version", lambda: "99.0.0")
    monkeypatch.setattr(gg, "_read_gateway_version_from_registry", lambda *_a, **_k: "0.1.0")
    monkeypatch.setattr(gg, "_request_gateway_yield", lambda *_a, **_k: pytest.fail("must not reconfigure resident"))
    monkeypatch.setattr(gg, "launch_detached", lambda *_a, **_k: pytest.fail("must not replace resident"))
    result = gg.ensure_gateway_daemon(
        gateway_host="127.0.0.1",
        gateway_port=19765,
        registry_dir=str(tmp_path),
        dcc_type="blender",
        gateway_remote_host="127.0.0.1",
        gateway_remote_port=0,
    )
    assert result == {"ok": True, "reason": "already_healthy"}


@pytest.mark.parametrize("managed", [False, True])
def test_only_adapter_managed_sidecars_delegate_daemon_ownership(tmp_path, monkeypatch, managed):
    from dcc_mcp_core._runtime import server_factory
    from dcc_mcp_core._runtime import sidecar_skill_server as sidecar_module

    monkeypatch.setattr(server_factory, "is_core_extension_available", lambda: False)
    owner = _owner(tmp_path, gateway_remote_port=0, host_rpc="commandport://127.0.0.1:1")
    options = DccServerOptions.from_env("blender", tmp_path, host_rpc="commandport://127.0.0.1:1")
    sidecar = server_factory.create_adapter_server(
        "blender",
        owner._config,
        options,
        gateway_managed_by_adapter=managed,
    )
    seen = []
    monkeypatch.setattr(sidecar_module, "build_sidecar_command", lambda **kw: seen.append(kw) or {"success": True})
    monkeypatch.setattr(
        sidecar_module,
        "launch_sidecar",
        lambda **kw: (
            seen.append(kw)
            or {
                "success": True,
                "mcp_url": "http://127.0.0.1:19767/mcp",
            }
        ),
    )
    handle = sidecar.start()
    try:
        assert handle.port == 19767
        for call in seen:
            assert call["no_ensure_gateway"] is managed
            assert call["legacy_gateway_election"] is managed
            assert call["gateway_port"] == 19765
            assert call["gateway_remote_host"] == "127.0.0.1"
            assert call["gateway_remote_port"] == 0
    finally:
        handle.shutdown()


def test_disabled_primary_gateway_remains_zero_at_sidecar_boundary(tmp_path, monkeypatch):
    from dcc_mcp_core._runtime import sidecar_skill_server as sidecar_module

    monkeypatch.setenv("DCC_MCP_GATEWAY_PORT", "9765")
    owner = _owner(tmp_path, gateway_remote_port=0)
    owner._config.gateway_port = 0
    sidecar = SidecarBackedSkillServer("photoshop", owner._config, host_rpc="commandport://127.0.0.1:1")
    calls = []
    monkeypatch.setattr(sidecar_module, "build_sidecar_command", lambda **kw: calls.append(kw) or {"success": True})
    monkeypatch.setattr(
        sidecar_module,
        "launch_sidecar",
        lambda **kw: (
            calls.append(kw)
            or {
                "success": True,
                "mcp_url": "http://127.0.0.1:19767/mcp",
            }
        ),
    )
    sidecar.start().shutdown()
    assert all(call["gateway_port"] == 0 for call in calls)


@pytest.mark.parametrize("factory_name", ["create_adapter_server", "create_skill_server"])
@pytest.mark.parametrize("kind", ["none", "missing", "null", "zero", "positive"])
@pytest.mark.parametrize("environment_port", [None, "19768"])
def test_factory_sidecars_keep_default_and_explicit_gateway_ports_distinct(
    tmp_path, monkeypatch, factory_name, kind, environment_port
):
    from dcc_mcp_core._runtime import server_factory
    from dcc_mcp_core._runtime import sidecar_skill_server as sidecar_module

    monkeypatch.setattr(server_factory, "is_core_extension_available", lambda: False)
    monkeypatch.setattr(SidecarBackedSkillServer, "discover", Mock())
    monkeypatch.setenv("DCC_MCP_HOST_RPC", "commandport://127.0.0.1:1")
    monkeypatch.setenv("DCC_MCP_REGISTRY_DIR", str(tmp_path))
    if environment_port is None:
        monkeypatch.delenv("DCC_MCP_GATEWAY_PORT", raising=False)
    else:
        monkeypatch.setenv("DCC_MCP_GATEWAY_PORT", environment_port)
    config = None
    expected_port = None
    if kind != "none":
        config = SimpleNamespace(port=0, server_name="factory-regression", endpoint_path="/mcp")
        if kind != "missing":
            expected_port = {"null": None, "zero": 0, "positive": 19769}[kind]
            config.gateway_port = expected_port

    calls = []
    plans = []
    real_build = sidecar_module.build_sidecar_command

    def build(**kwargs):
        calls.append(kwargs)
        plan = real_build(**kwargs)
        plans.append(plan)
        return plan

    def launch(**kwargs):
        calls.append(kwargs)
        return {"success": True, "mcp_url": "http://127.0.0.1:19767/mcp"}

    monkeypatch.setattr(sidecar_module, "build_sidecar_command", build)
    monkeypatch.setattr(sidecar_module, "launch_sidecar", launch)
    sidecar = getattr(server_factory, factory_name)("blender", config)
    sidecar.start().shutdown()

    assert len(calls) == 2
    for call in calls:
        assert call["gateway_port"] == expected_port
        assert call["no_ensure_gateway"] is False
        assert call["legacy_gateway_election"] is False
    resolved_port = expected_port if expected_port is not None else int(environment_port or "9765")
    assert plans[0]["success"] is True
    assert plans[0]["gateway_port"] == resolved_port
    command = plans[0]["command"]
    assert command[command.index("--gateway-port") + 1] == str(resolved_port)


def test_public_sidecar_composition_keeps_python_ensure_and_crash_recovery(tmp_path, monkeypatch):
    from dcc_mcp_core import DccServerBase
    from dcc_mcp_core._runtime import core_availability
    from dcc_mcp_core._runtime import sidecar_skill_server as sidecar_module

    monkeypatch.setattr(core_availability, "_CORE_AVAILABLE", False)
    options = DccServerOptions.from_env(
        "photoshop",
        tmp_path,
        gateway_port=19765,
        gateway_remote_port=0,
        registry_dir=str(tmp_path),
        host_rpc="commandport://127.0.0.1:1",
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
        enable_checkpoint_persistence=False,
        enable_checkpoint_tools=False,
    )
    stages = []

    def ensured(**kw):
        stages.append(("ensure", kw))
        return {"ok": True, "reason": "test_owned_spawn"}

    monkeypatch.setattr(rt, "ensure_gateway_daemon", ensured)
    monkeypatch.setattr(gg, "ensure_gateway_daemon", ensured)
    monkeypatch.setattr(gg, "_is_application_ready", lambda *_a, **_k: False)
    monkeypatch.setattr(gg.GatewayDaemonGuardian, "start", lambda self: True)
    monkeypatch.setattr(rt.ServerRuntimeController, "_start_guardian_watchdog", lambda self: None)
    monkeypatch.setattr(
        sidecar_module, "build_sidecar_command", lambda **kw: stages.append(("build", kw)) or {"success": True}
    )
    monkeypatch.setattr(
        sidecar_module,
        "launch_sidecar",
        lambda **kw: (
            stages.append(("launch", kw))
            or {
                "success": True,
                "mcp_url": "http://127.0.0.1:19767/mcp",
            }
        ),
    )
    server = DccServerBase(options)
    try:
        server.start(install_atexit_hook=False)
        assert [stage for stage, _kw in stages] == ["ensure", "build", "launch"]
        for _stage, kwargs in stages:
            assert kwargs["gateway_remote_port"] == 0
            assert kwargs["gateway_remote_host"] == "127.0.0.1"
        for _stage, kwargs in stages[1:]:
            assert kwargs["no_ensure_gateway"] is True
            assert kwargs["legacy_gateway_election"] is True
            assert kwargs["gateway_port"] == 19765
        assert server.get_gateway_election_status()["gateway_recovery_driver"] == "daemon_guardian"
        guardian = server._gateway_guardian
        assert guardian is not None
        for _ in range(guardian.failure_threshold):
            guardian.probe_once()
        assert [stage for stage, _kw in stages] == ["ensure", "build", "launch", "ensure"]
        assert stages[-1][1]["gateway_remote_port"] == 0
    finally:
        server.stop()
