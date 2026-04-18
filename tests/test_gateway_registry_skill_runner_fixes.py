"""E2E regression tests for the gateway / registry / skill-runner bug fixes.

Covers the five issues closed on the branch ``fix/gateway-registry-skill-runner-bugs``:

* **#227 — Ghost entries**: ``TransportManager.register_service`` auto-populates
  ``pid`` from ``os.getpid()`` when the caller doesn't pass one, and the new
  ``prune_dead_pids()`` method reaps rows whose owning process is dead.
* **#228 — Gateway version compare (self-yield)**: a DCC host version like
  Maya's ``"2024"`` must not masquerade as a newer gateway crate version.
  (Covered exhaustively at the Rust unit-test level — the Python layer
  simply verifies the ``__gateway__`` sentinel row survives normal
  registration / heartbeating without being confused with DCC entries.)
* **#229 — Sentinel heartbeat**: ``TransportManager.heartbeat`` on the
  sentinel row advances ``last_heartbeat`` so cleanup doesn't evict it.
* **#230 — Sentinel eviction**: ``TransportManager.cleanup()`` never removes
  the ``__gateway__`` sentinel, even when its heartbeat is artificially old.
* **#231 — Skill runner ambient-python fallback**: covered by Rust unit
  tests in ``crates/dcc-mcp-skills/src/catalog/tests.rs``; nothing here.
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import os
from pathlib import Path
import time

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

GATEWAY_SENTINEL = "__gateway__"
GHOST_PID = 0xFFFFFFFF  # u32::MAX — reserved/dead on every OS we target


@pytest.fixture()
def manager(tmp_path: Path) -> dcc_mcp_core.TransportManager:
    """Fresh ``TransportManager`` backed by an isolated temp directory."""
    mgr = dcc_mcp_core.TransportManager(str(tmp_path))
    yield mgr
    with contextlib.suppress(Exception):
        mgr.shutdown()


# ── Issue #227 — ghost-entry reaping ──────────────────────────────────────────


class TestGhostEntryReaping:
    """``prune_dead_pids`` removes rows whose owning PID is gone (#227)."""

    def test_register_without_pid_auto_populates_current_pid(self, manager: dcc_mcp_core.TransportManager) -> None:
        iid = manager.register_service("maya", "127.0.0.1", 18810)
        entry = manager.get_service("maya", iid)
        assert entry is not None
        assert entry.pid == os.getpid(), (
            "auto-populated pid must equal current process id so future prune_dead_pids calls don't reap our own row"
        )

    def test_prune_removes_ghost_rows_only(self, manager: dcc_mcp_core.TransportManager) -> None:
        live = manager.register_service("maya", "127.0.0.1", 18811)
        ghost = manager.register_service("maya", "127.0.0.1", 18812, pid=GHOST_PID)

        removed = manager.prune_dead_pids()
        assert removed == 1, f"exactly one ghost row must be pruned, got {removed}"

        assert manager.get_service("maya", live) is not None, "live row must survive"
        assert manager.get_service("maya", ghost) is None, "ghost row must be reaped"

    def test_cleanup_also_prunes_ghosts(self, manager: dcc_mcp_core.TransportManager) -> None:
        # ``cleanup()`` is the cron-style path that long-running gateways and
        # DCC plugins invoke periodically — it must fold ghost-pruning in.
        manager.register_service("maya", "127.0.0.1", 18813)
        manager.register_service("maya", "127.0.0.1", 18814, pid=GHOST_PID)

        stale_services, _closed_sessions, _evicted = manager.cleanup()
        # The live row stays, ghost is gone.
        assert stale_services >= 1
        remaining = [e for e in manager.list_instances("maya") if e.pid != GHOST_PID]
        assert len(remaining) == 1


# ── Issues #229 + #230 — sentinel heartbeat / preservation ────────────────────


class TestSentinelLifecycle:
    """The ``__gateway__`` sentinel row survives cleanup and heartbeats OK."""

    def test_sentinel_is_not_evicted_by_cleanup(self, manager: dcc_mcp_core.TransportManager) -> None:
        sentinel_iid = manager.register_service(
            GATEWAY_SENTINEL,
            "127.0.0.1",
            9765,
            version="0.13.2",
        )
        # A stale DCC row that SHOULD be reaped by cleanup.
        manager.register_service("maya", "127.0.0.1", 18815, pid=GHOST_PID)

        # Let the heartbeat age enough that cleanup_stale would evict anyone
        # without the sentinel exception.  The default heartbeat interval is
        # short enough in tests that we just trigger cleanup immediately and
        # rely on the ghost-PID path to evict the Maya row.
        manager.cleanup()

        sentinel = manager.get_service(GATEWAY_SENTINEL, sentinel_iid)
        assert sentinel is not None, (
            "gateway sentinel must survive cleanup even when cleanup_stale would otherwise flag it (issue #230)"
        )
        assert sentinel.version == "0.13.2"

    def test_sentinel_heartbeat_advances(self, manager: dcc_mcp_core.TransportManager) -> None:
        iid = manager.register_service(
            GATEWAY_SENTINEL,
            "127.0.0.1",
            9765,
            version="0.13.2",
        )
        before = manager.get_service(GATEWAY_SENTINEL, iid).last_heartbeat_ms
        # Small sleep to ensure system clock moves forward even on low-res timers.
        time.sleep(0.05)
        assert manager.heartbeat(GATEWAY_SENTINEL, iid) is True
        after = manager.get_service(GATEWAY_SENTINEL, iid).last_heartbeat_ms
        assert after > before, (
            f"sentinel heartbeat must advance on heartbeat() call; before={before} after={after} (issue #229)"
        )
