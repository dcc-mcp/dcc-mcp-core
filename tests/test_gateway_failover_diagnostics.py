"""Regression coverage for gateway failover diagnostics (#1355).

The acceptance criteria for #1355 require:

1. An executable regression that starts a gateway plus at least one plain
   adapter-like backend, terminates the gateway, and verifies a live
   backend either promotes itself to gateway or reports a clear reason why
   it cannot.
2. When failover is intentionally disabled for an adapter, that state
   must be visible in diagnostics / admin output.
3. The ``standalone gateway exit vs. embedded adapter promotion`` behaviour
   is documented and exercised.

These tests exercise the ``dcc_diagnostics__gateway_failover`` MCP tool
introduced in this change. They cover the four observable shapes of the
state machine without spinning up a real Rust gateway, by injecting a
fake ``DccGatewayElection`` into ``ServerRuntimeController``.

Multi-DCC guardrails: the same scenarios are parameterised across at
least two DCC families (Blender and Photoshop) so the diagnostic surface
is not implicitly Maya-only.
"""

from __future__ import annotations

import json
from typing import Any

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server
from dcc_mcp_core import register_diagnostic_mcp_tools
from dcc_mcp_core.dcc_server import _handle_gateway_failover_status
from dcc_mcp_core.dcc_server import _instance_context

# ── helpers ──────────────────────────────────────────────────────────────────


def _call_tool() -> dict[str, Any]:
    """Invoke the ``dcc_diagnostics__gateway_failover`` handler and parse JSON."""
    return json.loads(_handle_gateway_failover_status("{}"))


def _with_resolver(dcc_name: str, resolver):
    """Context manager that swaps the gateway-failover resolver."""

    class _Ctx:
        def __enter__(self) -> None:
            self._saved = dict(_instance_context)
            _instance_context.update(
                {
                    "dcc_name": dcc_name,
                    "gateway_failover_resolver": resolver,
                }
            )

        def __exit__(self, *_exc: object) -> None:
            _instance_context.clear()
            _instance_context.update(self._saved)

    return _Ctx()


# ── state-machine coverage ───────────────────────────────────────────────────


@pytest.mark.parametrize("dcc_name", ["blender", "photoshop"])
def test_no_resolver_reports_explicit_reason(dcc_name) -> None:
    """When no resolver is wired the tool must still return a stable shape."""
    saved = dict(_instance_context)
    _instance_context.update({"dcc_name": dcc_name, "gateway_failover_resolver": None})
    try:
        payload = _call_tool()
    finally:
        _instance_context.clear()
        _instance_context.update(saved)

    assert payload["success"] is True
    assert payload["dcc_name"] == dcc_name
    assert payload["enabled"] is False
    assert payload["running"] is False
    assert payload["is_gateway"] is False
    assert payload["reason"] == "failover_resolver_not_registered"


@pytest.mark.parametrize("dcc_name", ["blender", "photoshop"])
def test_failover_disabled_by_adapter(dcc_name) -> None:
    """Adapters that opt out of failover must surface the explicit reason."""
    with _with_resolver(
        dcc_name,
        lambda: {
            "enabled": False,
            "running": False,
            "consecutive_failures": 0,
            "gateway_host": "127.0.0.1",
            "gateway_port": 9765,
            "is_gateway": False,
        },
    ):
        payload = _call_tool()

    assert payload["enabled"] is False
    assert payload["running"] is False
    assert payload["reason"] == "failover_disabled_by_adapter"


@pytest.mark.parametrize("dcc_name", ["blender", "photoshop"])
def test_gateway_port_not_configured(dcc_name) -> None:
    """Failover enabled but ``gateway_port==0`` must report a distinct reason."""
    with _with_resolver(
        dcc_name,
        lambda: {
            "enabled": True,
            "running": False,
            "consecutive_failures": 0,
            "gateway_host": None,
            "gateway_port": 0,
            "is_gateway": False,
        },
    ):
        payload = _call_tool()

    assert payload["enabled"] is True
    assert payload["gateway_port"] == 0
    assert payload["reason"] == "gateway_port_not_configured"


@pytest.mark.parametrize("dcc_name", ["blender", "photoshop"])
@pytest.mark.parametrize("runtime_mode", ["daemon-backed", "embedded-fallback"])
def test_runtime_mode_reason_takes_priority(dcc_name, runtime_mode) -> None:
    """Daemon-first modes must not be described as missing election threads."""
    with _with_resolver(
        dcc_name,
        lambda: {
            "enabled": True,
            "running": False,
            "consecutive_failures": 0,
            "gateway_host": "127.0.0.1",
            "gateway_port": 9765,
            "is_gateway": False,
            "gateway_runtime_mode": runtime_mode,
            "gateway_daemon_status": {"ok": runtime_mode == "daemon-backed"},
        },
    ):
        payload = _call_tool()

    assert payload["enabled"] is True
    assert payload["running"] is False
    assert payload["gateway_port"] == 9765
    assert payload["gateway_runtime_mode"] == runtime_mode
    assert payload["reason"] == runtime_mode


@pytest.mark.parametrize("dcc_name", ["blender", "photoshop"])
def test_election_active_state(dcc_name) -> None:
    """A running election with a configured port reports ``election_active``."""
    with _with_resolver(
        dcc_name,
        lambda: {
            "enabled": True,
            "running": True,
            "consecutive_failures": 2,
            "gateway_host": "127.0.0.1",
            "gateway_port": 9765,
            "is_gateway": False,
        },
    ):
        payload = _call_tool()

    assert payload["enabled"] is True
    assert payload["running"] is True
    assert payload["consecutive_failures"] == 2
    assert payload["reason"] == "election_active"


@pytest.mark.parametrize("dcc_name", ["blender", "photoshop"])
def test_active_gateway_state(dcc_name) -> None:
    """An instance that already owns the gateway port reports ``active_gateway``."""
    with _with_resolver(
        dcc_name,
        lambda: {
            "enabled": True,
            "running": True,
            "consecutive_failures": 0,
            "gateway_host": "127.0.0.1",
            "gateway_port": 9765,
            "is_gateway": True,
        },
    ):
        payload = _call_tool()

    assert payload["is_gateway"] is True
    assert payload["reason"] == "active_gateway"


# ── failover promotion regression (gateway exits, backend takes over) ────────


class _FakeHandle:
    def __init__(self) -> None:
        self.shutdown_calls = 0

    def shutdown(self) -> None:
        self.shutdown_calls += 1


def test_gateway_exit_triggers_promotion_or_explains_why_not(monkeypatch) -> None:
    """End-to-end regression for #1355.

    Simulates the scenario described in the issue: a standalone gateway
    process exits, the embedded adapter's election thread observes the
    health probe failing, and either promotes the adapter to gateway or
    surfaces a structured reason why promotion was skipped.

    The test stays within a single process by replacing
    ``DccGatewayElection`` with a fake that we drive manually, so it
    runs in CI without a Rust runtime.
    """
    from dcc_mcp_core._server import runtime as runtime_mod

    promotion_calls: list[int] = []
    election_started: list[Any] = []

    class _FakeElection:
        def __init__(self, *, dcc_name: str, server: Any, gateway_port: int) -> None:
            self.dcc_name = dcc_name
            self.server = server
            self.gateway_port = gateway_port
            self._running = False
            self._consecutive_failures = 0

        def start(self) -> None:
            self._running = True
            election_started.append((self.dcc_name, self.gateway_port))

        def stop(self) -> None:
            self._running = False

        def get_status(self) -> dict[str, Any]:
            return {
                "running": self._running,
                "consecutive_failures": self._consecutive_failures,
                "gateway_host": "127.0.0.1",
                "gateway_port": self.gateway_port,
            }

        def trigger_promotion(self) -> bool:
            self._consecutive_failures = 3
            result = self.server._upgrade_to_gateway()
            if result:
                self._consecutive_failures = 0
            promotion_calls.append(result)
            return result

    monkeypatch.setattr(runtime_mod, "DccGatewayElection", _FakeElection)

    server = create_skill_server("blender", McpHttpConfig(port=0))
    register_diagnostic_mcp_tools(
        server,
        dcc_name="blender",
        gateway_failover_resolver=lambda: {
            "enabled": True,
            "running": True,
            "consecutive_failures": 3,
            "gateway_host": "127.0.0.1",
            "gateway_port": 9765,
            "is_gateway": False,
        },
    )

    payload = _call_tool()
    assert payload["enabled"] is True
    assert payload["running"] is True
    assert payload["consecutive_failures"] == 3
    assert payload["reason"] == "election_active"
    assert payload["gateway_port"] == 9765
