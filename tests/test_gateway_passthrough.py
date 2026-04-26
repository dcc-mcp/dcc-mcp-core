"""Gateway async-dispatch passthrough regression (issue #321).

The gateway must

1. Apply a longer per-request timeout when the client has opted into
   async dispatch (``_meta.dcc.async=true`` or ``_meta.progressToken``).
   This Python surface test pins the two new ``McpHttpConfig`` fields
   and confirms they round-trip through getters/setters and the
   constructor signature.
2. Support a wait-for-terminal mode where the gateway blocks the
   ``tools/call`` response until a terminal ``$/dcc.jobUpdated`` is
   observed. The full end-to-end dance requires a real backend SSE
   stream — covered by
   ``crates/dcc-mcp-http/tests/gateway_passthrough.rs``. Here we
   verify the Python API surface and that gateway startup does NOT
   regress when the defaults are bumped to the #321 values.
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import socket
import time

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── Config surface ────────────────────────────────────────────────────────


def test_mcp_http_config_defaults_match_issue_321():
    """Defaults: 60 s for async dispatch, 10 min for wait-for-terminal."""
    cfg = McpHttpConfig(port=8765)
    assert cfg.gateway_async_dispatch_timeout_ms == 60_000
    assert cfg.gateway_wait_terminal_timeout_ms == 600_000
    # Default raised from 10 s → 120 s to accommodate long DCC operations
    # (scene import, simulation bake, render). Regression guard for #314.
    assert cfg.backend_timeout_ms == 120_000


def test_mcp_http_config_accepts_new_fields_via_constructor():
    cfg = McpHttpConfig(
        port=8765,
        gateway_async_dispatch_timeout_ms=90_000,
        gateway_wait_terminal_timeout_ms=120_000,
    )
    assert cfg.gateway_async_dispatch_timeout_ms == 90_000
    assert cfg.gateway_wait_terminal_timeout_ms == 120_000


def test_mcp_http_config_setters_round_trip():
    cfg = McpHttpConfig(port=0)
    cfg.gateway_async_dispatch_timeout_ms = 45_000
    cfg.gateway_wait_terminal_timeout_ms = 30_000
    assert cfg.gateway_async_dispatch_timeout_ms == 45_000
    assert cfg.gateway_wait_terminal_timeout_ms == 30_000


# ── Gateway startup does not regress ──────────────────────────────────────


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


def test_gateway_starts_with_custom_passthrough_timeouts(tmp_path):
    """Regression: the new config fields don't break gateway election.

    The `McpServerHandle.is_gateway` path runs a self-probe (issue #303);
    if the new config fields were wired incorrectly (for example by
    dropping gateway supervisor tasks) this probe would fail.
    """
    registry_dir = tmp_path / "registry"
    registry_dir.mkdir()
    gw_port = _pick_free_port()

    reg = ToolRegistry()
    cfg = McpHttpConfig(
        port=0,
        server_name="gateway-passthrough-test",
        gateway_async_dispatch_timeout_ms=45_000,
        gateway_wait_terminal_timeout_ms=30_000,
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
        # Sanity: the config the server ran with reflects the overrides.
        assert cfg.gateway_async_dispatch_timeout_ms == 45_000
        assert cfg.gateway_wait_terminal_timeout_ms == 30_000
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()
