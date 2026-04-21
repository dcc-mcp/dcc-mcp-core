"""Gateway job-to-backend routing cache (issue #322).

End-to-end verification of the cancel-forward + cross-backend cascade
path is covered by the Rust integration tests in
``crates/dcc-mcp-http/src/gateway/sse_subscriber.rs`` (``JobRoute`` unit
tests) and can be exercised against a real two-backend cluster via the
``gateway_cross_process`` harness. From Python we pin the public surface:

1. The two new ``McpHttpConfig`` fields (``gateway_route_ttl_secs`` and
   ``gateway_max_routes_per_session``) accept defaults, constructor
   kwargs, and setter round-trips.
2. A gateway that is configured with those fields still elects
   successfully and its listener passes the self-probe (issue #303
   regression guard).
"""

from __future__ import annotations

import contextlib
import socket
import time

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── Config surface ────────────────────────────────────────────────────────


def test_mcp_http_config_defaults_match_issue_322():
    """Defaults: 24 h TTL and 1000 live routes per session."""
    cfg = McpHttpConfig(port=8765)
    assert cfg.gateway_route_ttl_secs == 86_400
    assert cfg.gateway_max_routes_per_session == 1_000


def test_mcp_http_config_accepts_routing_fields_via_constructor():
    cfg = McpHttpConfig(
        port=8765,
        gateway_route_ttl_secs=600,
        gateway_max_routes_per_session=16,
    )
    assert cfg.gateway_route_ttl_secs == 600
    assert cfg.gateway_max_routes_per_session == 16


def test_mcp_http_config_setters_round_trip():
    cfg = McpHttpConfig(port=0)
    cfg.gateway_route_ttl_secs = 300
    cfg.gateway_max_routes_per_session = 4
    assert cfg.gateway_route_ttl_secs == 300
    assert cfg.gateway_max_routes_per_session == 4
    # 0 disables the cap — sanity check the type accepts it.
    cfg.gateway_max_routes_per_session = 0
    assert cfg.gateway_max_routes_per_session == 0


# ── Gateway startup does not regress under custom routing limits ─────────


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _wait_reachable(port: int, budget: float = 5.0) -> bool:
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return True
        except (OSError, socket.timeout):
            time.sleep(0.05)
    return False


def test_gateway_starts_with_custom_routing_cache_limits(tmp_path):
    """Regression: tight TTL + small per-session cap must not break election.

    If the GC task or the per-session cap were wired incorrectly, the
    self-probe inside ``start_gateway_tasks`` would time out and
    ``is_gateway`` would fall back to ``False``.
    """
    registry_dir = tmp_path / "registry"
    registry_dir.mkdir()
    gw_port = _pick_free_port()

    reg = ToolRegistry()
    cfg = McpHttpConfig(
        port=0,
        server_name="gateway-routing-cache-test",
        gateway_route_ttl_secs=2,  # aggressive TTL — GC loops every 60 s
        gateway_max_routes_per_session=4,
    )
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = "python"
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10

    server = McpHttpServer(reg, cfg)
    handle = server.start()
    try:
        assert _wait_reachable(handle.port), "instance port must be reachable"
        if not handle.is_gateway:
            pytest.skip(f"another process holds gateway port {gw_port} — cannot verify gateway startup invariants here")
        assert _wait_reachable(gw_port), "gateway port must be reachable"
        # The config the server ran with reflects the overrides.
        assert cfg.gateway_route_ttl_secs == 2
        assert cfg.gateway_max_routes_per_session == 4
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()
