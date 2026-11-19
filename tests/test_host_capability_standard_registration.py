"""Standard DccServerBase registration for the host capability backend tool.

These tests use lightweight server doubles so they run without a compiled
``dcc_mcp_core._core`` extension or a live DCC application.
"""

from __future__ import annotations

from types import SimpleNamespace

from dcc_mcp_core.capabilities import PROBE_NOT_RUN
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
    assert "dcc_host_capabilities" in server._server.registry.actions

    result = server._server.registry.handlers["dcc_host_capabilities"]({})
    assert result["success"] is True
    assert result["context"]["host_version"] == "16.0"
    assert result["context"]["totals"]["not_run"] == 1


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
    server.set_host_capability_provider(_passed_manifest)

    assert server.register_host_capability_tool() is True
    assert server.register_host_capability_tool() is False
    assert list(server._server.registry.actions) == ["dcc_host_capabilities"]

    result = server._server.registry.handlers["dcc_host_capabilities"]({})
    assert result["context"]["totals"]["available"] == 1
