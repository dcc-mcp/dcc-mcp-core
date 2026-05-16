"""Regression tests for gateway failover (RFC #998 follow-up, 2026-05-16).

Failure mode observed by the user
=================================

1. Maya A wins the gateway election (binds 9765) and registers a
   ``__gateway__`` sentinel in the FileRegistry.
2. Maya A crashes uncleanly. The kernel keeps the socket in
   ``TIME_WAIT`` for ~2 minutes (Windows default
   ``TcpTimedWaitDelay``). The sentinel stays in the registry because
   no clean shutdown happened.
3. Maya B / C are non-gateway peers running ``DccGatewayElection``.
   Their ``GET /health`` probes against 9765 fail (no listener).
4. After two consecutive failures, the election attempts to take
   over. Previously, this had two bugs:

   a. ``_is_port_free`` used ``SO_REUSEADDR=1`` which silently
      bypasses ``TIME_WAIT`` on Windows. The probe reported "free"
      but the real Rust ``GatewayRunner`` bind uses
      ``SO_REUSEADDR=false`` and rejected the address. Every Python
      election iteration burned a new ``McpHttpServer`` handle for
      no gain — for the full TIME_WAIT window.
   b. The Rust ``run_election`` ``None`` branch read the registry
      WITHOUT calling ``prune_dead_entries``, so the dead gateway's
      sentinel kept claiming ownership. With the same crate version
      on both sides, ``is_newer_election`` would refuse to promote
      anyone — even after TIME_WAIT cleared. The instance stayed
      "plain" forever (or until restarted).

The fixes
=========

* Python ``_is_port_free`` now uses ``SO_REUSEADDR=0`` so the probe
  mirrors what Rust would observe. Honest probes mean ``_attempt_election``
  no longer burns handles during TIME_WAIT.
* Rust ``run_election`` calls ``prune_dead_entries`` before reading
  the resident sentinel. Stale entries from dead gateways are evicted,
  so peers can correctly detect "no live gateway" and either win the
  bind immediately or spawn the challenger loop to keep polling.
* The challenger loop is now spawned in two cases instead of one:
  classic version-rank takeover, AND the "bind failed but no resident
  sentinel after pruning" case (TIME_WAIT recovery).

This test file covers the Python half of the contract via mocks. The
Rust half is covered by ``crates/dcc-mcp-gateway/src/gateway/tests.rs``.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import socket
from unittest.mock import MagicMock

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.gateway_election import DccGatewayElection


@pytest.fixture
def free_port() -> int:
    """Pick a free localhost port without leaving it open."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


@pytest.fixture
def fake_server() -> MagicMock:
    """Minimal server stub matching the ``DccGatewayElection`` contract."""
    server = MagicMock()
    server.is_gateway = False
    server.is_running = True
    return server


class TestIsPortFreeMatchesRustBindSemantics:
    """_is_port_free must answer "would Rust's bind succeed right now?".

    The Rust gateway runner uses ``socket.set_reuse_address(false)``
    (see ``crates/dcc-mcp-gateway/src/gateway/bind.rs::try_bind_port``).
    The Python probe must mirror this so the election doesn't lie about
    port availability and waste a server restart per iteration during
    TIME_WAIT.
    """

    def test_free_port_returns_true(self, fake_server, free_port):
        election = DccGatewayElection(
            dcc_name="test",
            server=fake_server,
            gateway_port=free_port,
        )
        assert election._is_port_free() is True

    def test_busy_port_returns_false(self, fake_server):
        """Hold a real listener on a port; the probe must report False."""
        holder = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        # Match the Rust bind: no SO_REUSEADDR.
        holder.bind(("127.0.0.1", 0))
        holder.listen(1)
        held_port = holder.getsockname()[1]

        try:
            election = DccGatewayElection(
                dcc_name="test",
                server=fake_server,
                gateway_port=held_port,
            )
            assert election._is_port_free() is False
        finally:
            holder.close()

    def test_probe_does_not_use_so_reuseaddr(self, fake_server, free_port, monkeypatch):
        """Regression guard: the probe must NOT enable SO_REUSEADDR.

        With ``SO_REUSEADDR=1`` Windows silently lets the bind through
        even when the kernel is holding the port in TIME_WAIT. That
        causes the false-positive that burns handles during failover
        (the original bug).
        """
        captured_optnames: list[int] = []
        captured_optvals: list[int] = []

        original_setsockopt = socket.socket.setsockopt

        def capturing_setsockopt(self, level, optname, value, *args, **kw):
            if level == socket.SOL_SOCKET:
                captured_optnames.append(optname)
                if isinstance(value, int):
                    captured_optvals.append(value)
            return original_setsockopt(self, level, optname, value, *args, **kw)

        monkeypatch.setattr(socket.socket, "setsockopt", capturing_setsockopt)

        election = DccGatewayElection(
            dcc_name="test",
            server=fake_server,
            gateway_port=free_port,
        )
        election._is_port_free()

        # SO_REUSEADDR must have been set, and the value must be 0.
        reuse_calls = [v for opt, v in zip(captured_optnames, captured_optvals) if opt == socket.SO_REUSEADDR]
        assert reuse_calls, "_is_port_free must explicitly clear SO_REUSEADDR for Rust parity"
        assert 1 not in reuse_calls, (
            "SO_REUSEADDR=1 in _is_port_free re-introduces the Windows TIME_WAIT "
            "false-positive that this fix exists to prevent."
        )


class TestElectionDoesNotBurnHandlesDuringTimeWait:
    """Verify the high-level contract: when port is busy, no promotion attempt.

    ``_attempt_election`` MUST short-circuit on ``_is_port_free() == False``
    and MUST NOT call ``_upgrade_to_gateway()``. Otherwise it tears down
    the working random-port handle every iteration during TIME_WAIT
    recovery.
    """

    def test_busy_port_skips_upgrade_call(self, fake_server):
        holder = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        holder.bind(("127.0.0.1", 0))
        holder.listen(1)
        held_port = holder.getsockname()[1]

        try:
            election = DccGatewayElection(
                dcc_name="test",
                server=fake_server,
                gateway_port=held_port,
            )
            # Spy on _upgrade_to_gateway so we can assert it's NOT called.
            election._upgrade_to_gateway = MagicMock(return_value=True)

            assert election._attempt_election() is False
            election._upgrade_to_gateway.assert_not_called()
        finally:
            holder.close()

    def test_free_port_triggers_upgrade_call(self, fake_server, free_port):
        election = DccGatewayElection(
            dcc_name="test",
            server=fake_server,
            gateway_port=free_port,
        )
        election._upgrade_to_gateway = MagicMock(return_value=True)

        assert election._attempt_election() is True
        election._upgrade_to_gateway.assert_called_once()
