"""Standard DccServerBase registration for the host capability backend tool.

These tests use lightweight server doubles so they run without a compiled
``dcc_mcp_core._core`` extension or a live DCC application.
"""

from __future__ import annotations

from types import SimpleNamespace

from dcc_mcp_core.capabilities import INITIAL_CAPABILITY_KEYS
from dcc_mcp_core.capabilities import PROBE_NOT_RUN
from dcc_mcp_core.capabilities import CapabilityProbeRunner
from dcc_mcp_core.capabilities import build_host_capabilities
from dcc_mcp_core.capabilities import build_unconfigured_host_capabilities
from dcc_mcp_core.capabilities import make_host_capability
from dcc_mcp_core.server_base import DccServerBase


class _Registry:
    def __init__(self):
        self.actions = {}
        self.handlers = {}

    def register(self, name, **kwargs):
        if name in self.actions:
            raise AssertionError(f"duplicate tool registration: {name}")
        self.actions[name] = kwargs

    def get_action(self, name):
        return self.actions.get(name)


class _Inner:
    def __init__(self):
        self.registry = _Registry()

    def register_handler(self, name, handler):
        self.registry.handlers[name] = handler


def _make_server(dcc_name="maya"):
    server = DccServerBase.__new__(DccServerBase)
    server._dcc_name = dcc_name
    server._server = _Inner()
    server._options = SimpleNamespace(sidecar=SimpleNamespace(adapter_version="0.1.6"))
    server._host_capability_provider = None
    server._host_capability_cache = None
    server._host_capability_provider_cache_key = None
    server._host_capability_probe_runner = None
    return server


def _passed_manifest():
    return build_host_capabilities(
        host_version="2026.2",
        adapter_version="0.1.6",
        dcc_type="maya",
        capabilities=[
            make_host_capability(
                "render.arnold",
                True,
                "PASS",
                probed_at="2026-11-20T00:00:00Z",
                evidence_ref="probe://render/arnold",
            )
        ],
    )


def test_standard_registration_needs_no_manual_provider_call(monkeypatch):
    server = _make_server("nuke")
    monkeypatch.setattr(DccServerBase, "_version_string", lambda self: "16.0")

    assert server.register_host_capability_tool() is True
    assert list(server._server.registry.actions) == [
        "describe_capabilities",
        "dcc_host_capabilities",
    ]

    result = server._server.registry.handlers["dcc_host_capabilities"]({})
    assert result["success"] is True
    assert result["context"]["host_version"] == "16.0"
    assert result["context"]["totals"]["not_run"] == len(INITIAL_CAPABILITY_KEYS)


def test_missing_provider_returns_honest_not_run_with_reason():
    manifest = build_unconfigured_host_capabilities(
        dcc_name="houdini",
        host_version="20.5",
        adapter_version="0.1.6",
    )
    record = manifest["capabilities"][0]
    assert record["supported"] is False
    assert record["probe_result"] == PROBE_NOT_RUN
    assert "No host capability provider" in record["reason"]


def test_explicit_registration_is_not_duplicated():
    server = _make_server("maya")

    assert server.register_host_capability_tool() is True
    server.set_host_capability_provider(_passed_manifest)
    assert server.register_host_capability_tool() is False
    assert list(server._server.registry.actions) == [
        "describe_capabilities",
        "dcc_host_capabilities",
    ]

    result = server._server.registry.handlers["dcc_host_capabilities"]({})
    assert result["context"]["totals"]["available"] == 1
    assert result["context"]["totals"]["not_run"] == len(INITIAL_CAPABILITY_KEYS)


def test_provider_injected_after_fallback_registration_rebinds_handler():
    server = _make_server("maya")

    assert server.register_host_capability_tool() is True
    fallback = server._server.registry.handlers["dcc_host_capabilities"]({})
    assert fallback["context"]["totals"]["not_run"] == len(INITIAL_CAPABILITY_KEYS)

    server.set_host_capability_provider(_passed_manifest)
    result = server._server.registry.handlers["dcc_host_capabilities"]({})

    assert result["context"]["totals"]["available"] == 1


def test_provider_lifecycle_restores_not_run_without_duplicate_registration():
    server = _make_server("nuke")

    assert server.register_host_capability_tool() is True
    server.set_host_capability_provider(_passed_manifest)
    server.set_host_capability_provider(None)

    result = server._server.registry.handlers["dcc_host_capabilities"]({})
    assert result["context"]["totals"]["not_run"] == len(INITIAL_CAPABILITY_KEYS)
    assert "No host capability provider" in str(result["context"])
    assert list(server._server.registry.actions) == [
        "describe_capabilities",
        "dcc_host_capabilities",
    ]


def test_removing_probe_runner_rebinds_existing_probe_tool_to_safe_error():
    server = _make_server("maya")
    runner = CapabilityProbeRunner(
        lambda key: {
            "probe_result": "PASS",
            "supported": True,
            "evidence_ref": f"probe://{key}",
        },
        keys=("render.arnold",),
    )

    server.set_host_capability_provider(_passed_manifest, probe_runner=runner)
    assert "capability_probe" in server._server.registry.actions

    server.set_host_capability_provider(_passed_manifest, probe_runner=None)
    result = server._server.registry.handlers["capability_probe"]({})

    assert result["success"] is False
    assert result["error"] == "capability_probe_not_configured"


def test_registration_delegates_provider_state_and_cache_configuration(monkeypatch):
    registrations = []
    monkeypatch.setattr(
        "dcc_mcp_core._server.host_capability_registration.register_capability_tools",
        lambda *args, **kwargs: registrations.append(kwargs) or True,
    )

    server = _make_server("maya")
    controller = server._register_host_capabilities()
    cache = object()

    def identity():
        return ("maya", "2026.2", "0.1.6")

    assert controller is server._register_host_capabilities()
    controller.set_provider(_passed_manifest, cache=cache, provider_cache_key=identity)

    assert server._host_capability_provider is _passed_manifest
    assert server._host_capability_cache is cache
    assert server._host_capability_provider_cache_key is identity
    assert server._host_capability_registration is True
    assert registrations == [
        {
            "dcc_name": "maya",
            "provider": _passed_manifest,
            "cache": cache,
            "provider_cache_key": identity,
            "probe_runner": None,
        }
    ]


def test_standard_capabilities_phase_uses_delegating_registration():
    from dcc_mcp_core._registration import HostCapabilitiesPhase
    from dcc_mcp_core._registration import RegistrationContext

    server = _make_server("houdini")
    calls = []

    def register():
        calls.append(True)
        return True

    server.register_host_capability_tool = register

    HostCapabilitiesPhase().run(RegistrationContext(server=server))

    assert calls == [True]


def test_initial_registration_preserves_overridable_entry_point():
    server = _make_server("maya")
    calls = []

    def register():
        calls.append(True)
        return True

    server.register_host_capability_tool = register

    server._initialize_host_capability_registration("0.1.6")

    assert calls == [True]
