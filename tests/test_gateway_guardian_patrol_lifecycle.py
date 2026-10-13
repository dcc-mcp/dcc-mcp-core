"""Deterministic ordering regressions for the guardian's threaded test probes."""

from __future__ import annotations

import json
import linecache
import sys
import threading
import types

import pytest
from scripts.ci import guardian_patrol_diagnostics as diagnostics
from scripts.ci.guardian_patrol_diagnostics import guardian_snapshot

import test_gateway_guardian as cases

pytestmark = pytest.mark.skipif(
    sys.platform == "win32" and sys.version_info >= (3, 14),
    reason="Preserve the existing Windows py3.14 guardian threading exclusion (PIP-1610)",
)


def _patrol_threads():
    return {
        thread
        for thread in threading.enumerate()
        if thread.name.startswith("dcc-mcp-gateway-guardian-") or thread.name == "stuck-guardian"
    }


def _inject_foreign_probe(monkeypatch, method_name="start", name="foreign-guardian-probe"):
    original = getattr(cases.gg.GatewayDaemonGuardian, method_name)
    attempts = []
    failures = []

    def start(guardian, *args, **kwargs):
        if method_name == "__init__":
            original(guardian, *args, **kwargs)

        def probe():
            attempts.append(threading.current_thread().name)
            try:
                cases.gg._is_application_ready(guardian.gateway_host, guardian.gateway_port, timeout=0.05)
            except Exception as error:
                failures.append(type(error).__name__)

        foreign = threading.Thread(target=probe, name=name, daemon=True)
        foreign.start()
        foreign.join(timeout=2.0)
        assert not foreign.is_alive(), "controlled foreign probe must finish before the target starts"
        if method_name == "start":
            return original(guardian)

    monkeypatch.setattr(cases.gg.GatewayDaemonGuardian, method_name, start)
    return attempts, failures


@pytest.mark.parametrize(
    ("method_name", "name"),
    [
        ("start", "foreign-guardian-probe"),
        ("start", "dcc-mcp-gateway-guardian-resilient-test"),
        ("__init__", "foreign-guardian-probe"),
    ],
)
def test_resilience_probe_keeps_its_first_exception_for_the_owned_thread(monkeypatch, method_name, name):
    """A foreign caller must not consume the target test's synthetic exception."""
    attempts, failures = _inject_foreign_probe(monkeypatch, method_name, name)
    cases.test_guardian_run_continues_after_exception(monkeypatch)
    assert attempts == [name]
    assert failures == []


@pytest.mark.parametrize("preceding", ["crash", "healthy", "timed_out_stop"])
def test_resilience_probe_after_preceding_guardian_case(monkeypatch, preceding):
    baseline = _patrol_threads()
    with monkeypatch.context() as previous:
        if preceding == "crash":
            cases.test_guardian_run_catches_crash_and_increments_crash_count(previous)
        elif preceding == "healthy":
            cases.test_gateway_daemon_guardian_resets_failures_on_health(previous)
        else:
            cases.test_guardian_stop_keeps_a_thread_that_refuses_to_exit_visible()
    assert _patrol_threads() <= baseline, "preceding guardian case leaked a patrol thread"
    attempts, failures = _inject_foreign_probe(monkeypatch)
    cases.test_guardian_run_continues_after_exception(monkeypatch)
    assert attempts == ["foreign-guardian-probe"]
    assert failures == []
    assert _patrol_threads() <= baseline, "resilience case leaked a patrol thread"


def test_failure_snapshot_does_not_wait_for_a_busy_status_lock(monkeypatch):
    guardian = cases.gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1", gateway_port=9765, registry_dir=None, dcc_type="snapshot-test"
    )
    started = threading.Event()
    release = threading.Event()

    def blocked():
        started.set()
        release.wait(5.0)

    thread = threading.Thread(target=blocked, name="dcc-mcp-gateway-guardian-snapshot-test", daemon=True)
    guardian._thread = thread
    thread.start()
    assert started.wait(2.0)
    guardian._lock.acquire()
    lookups = []
    owner = threading.current_thread()
    original_getline, original_checkcache = linecache.getline, linecache.checkcache

    def getline(*args, **kwargs):
        if threading.current_thread() is owner:
            lookups.append("getline")
            return ""
        return original_getline(*args, **kwargs)

    def checkcache(*args, **kwargs):
        if threading.current_thread() is owner:
            lookups.append("checkcache")
            return
        return original_checkcache(*args, **kwargs)

    monkeypatch.setattr(linecache, "getline", getline)
    monkeypatch.setattr(linecache, "checkcache", checkcache)
    try:
        snapshot = json.loads(guardian_snapshot(guardian, [1, 1], [("foreign-probe", 42)], cases.gg))
        assert snapshot["status_lock"] == "busy"
        assert snapshot["probe_calls"] == 2
        assert snapshot["foreign_threads"] == [{"name": "foreign-probe", "ident": 42}]
        assert snapshot["mock_is_loaded_module"]
        assert snapshot["mock_is_package_module"]
        owned = next(row for row in snapshot["threads"] if row["owned"])
        assert owned["alive"]
        assert owned["stack"]
        assert len(owned["stack"]) <= 8
        assert all("/" not in row["file"] and "\\" not in row["file"] for row in owned["stack"])
        assert lookups == [], "failure stacks must not read or stat source files"
    finally:
        guardian._lock.release()
        release.set()
        thread.join(timeout=2.0)
    assert not thread.is_alive()


def test_failure_snapshot_records_a_dead_owner_and_missing_package(monkeypatch):
    guardian = cases.gg.GatewayDaemonGuardian(
        gateway_host="127.0.0.1", gateway_port=9765, registry_dir=None, dcc_type="finished-test"
    )
    thread = threading.Thread(target=lambda: None, name="dcc-mcp-gateway-guardian-finished-test")
    guardian._thread = thread
    thread.start()
    thread.join(timeout=2.0)
    assert not thread.is_alive()
    monkeypatch.setattr(
        diagnostics,
        "sys",
        types.SimpleNamespace(
            modules={cases.gg.__name__: cases.gg},
            _current_frames=sys._current_frames,
        ),
    )
    snapshot = json.loads(guardian_snapshot(guardian, [], [], cases.gg))
    assert snapshot["status_lock"] == "available"
    assert not snapshot["mock_is_package_module"]
    assert not snapshot["stop_requested"]
    owned = next(row for row in snapshot["threads"] if row["owned"])
    assert not owned["alive"]
    assert owned["stack"] == []
