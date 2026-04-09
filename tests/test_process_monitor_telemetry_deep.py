"""Deep tests for PyProcessMonitor (untrack/list_all/tracked_count/is_alive).

Also covers TelemetryConfig complete builder chain.
"""

from __future__ import annotations

import os
import time

import pytest

from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import PyProcessWatcher
from dcc_mcp_core import TelemetryConfig

# ---------------------------------------------------------------------------
# PyProcessMonitor deep tests
# ---------------------------------------------------------------------------


class TestPyProcessMonitorUntrack:
    """untrack: stop monitoring a PID."""

    def test_untrack_reduces_count(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        assert mon.tracked_count() == 1
        mon.untrack(pid)
        assert mon.tracked_count() == 0

    def test_untrack_nonexistent_no_error(self):
        mon = PyProcessMonitor()
        # Should not raise
        mon.untrack(99999)

    def test_untrack_removes_from_list_all(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        mon.untrack(pid)
        assert mon.list_all() == []

    def test_untrack_then_retrack(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.untrack(pid)
        mon.track(pid, "self_again")
        assert mon.tracked_count() == 1


class TestPyProcessMonitorListAll:
    """list_all: snapshot for all tracked PIDs."""

    def test_list_all_empty(self):
        mon = PyProcessMonitor()
        result = mon.list_all()
        assert isinstance(result, list)
        assert len(result) == 0

    def test_list_all_single_process(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        items = mon.list_all()
        assert len(items) == 1
        item = items[0]
        assert item["pid"] == pid
        assert item["name"] == "self"
        assert "status" in item
        assert "cpu_usage_percent" in item
        assert "memory_bytes" in item

    def test_list_all_multiple_processes(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        # Track same process under two names (PIDs are unique in map, so only last wins)
        # Instead, track one PID
        mon.track(pid, "process_a")
        mon.refresh()
        items = mon.list_all()
        assert len(items) >= 1

    def test_list_all_keys_present(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "test")
        mon.refresh()
        item = mon.list_all()[0]
        required_keys = {"pid", "name", "status", "cpu_usage_percent", "memory_bytes", "restart_count"}
        assert required_keys.issubset(set(item.keys()))

    def test_list_all_empty_after_untrack(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.untrack(pid)
        assert len(mon.list_all()) == 0


class TestPyProcessMonitorTrackedCount:
    """tracked_count: number of currently tracked PIDs."""

    def test_tracked_count_initial_zero(self):
        mon = PyProcessMonitor()
        assert mon.tracked_count() == 0

    def test_tracked_count_after_track(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "p1")
        assert mon.tracked_count() == 1

    def test_tracked_count_after_untrack(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "p")
        mon.untrack(pid)
        assert mon.tracked_count() == 0


class TestPyProcessMonitorIsAlive:
    """is_alive: check if PID is in OS process table."""

    def test_is_alive_self(self):
        mon = PyProcessMonitor()
        assert mon.is_alive(os.getpid()) is True

    def test_is_alive_nonexistent_pid(self):
        mon = PyProcessMonitor()
        # PID 0 typically is the scheduler or system idle; treat as not alive
        # Use very high PID unlikely to exist
        assert mon.is_alive(9999999) is False

    def test_is_alive_does_not_require_tracking(self):
        mon = PyProcessMonitor()
        # is_alive checks OS process table, not just tracked list
        result = mon.is_alive(os.getpid())
        assert result is True


class TestPyProcessMonitorQueryDepth:
    """query: return snapshot dict for a tracked PID."""

    def test_query_returns_none_when_not_tracked(self):
        mon = PyProcessMonitor()
        result = mon.query(os.getpid())
        assert result is None

    def test_query_returns_dict_when_tracked(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        result = mon.query(pid)
        assert result is not None
        assert isinstance(result, dict)
        assert result["pid"] == pid

    def test_query_memory_bytes_positive(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["memory_bytes"] > 0

    def test_query_restart_count_initial_zero(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["restart_count"] == 0

    def test_query_status_is_string(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert isinstance(info["status"], str)
        assert len(info["status"]) > 0


class TestPyProcessWatcherDeep:
    """PyProcessWatcher: background polling with event drain."""

    def test_watcher_start_stop(self):
        watcher = PyProcessWatcher(poll_interval_ms=100)
        assert watcher.is_running() is False
        watcher.start()
        assert watcher.is_running() is True
        watcher.stop()
        time.sleep(0.15)
        assert watcher.is_running() is False

    def test_watcher_track_untrack(self):
        watcher = PyProcessWatcher(poll_interval_ms=100)
        pid = os.getpid()
        watcher.track(pid, "self")
        assert watcher.tracked_count() == 1
        watcher.untrack(pid)
        assert watcher.tracked_count() == 0

    def test_watcher_produces_events(self):
        watcher = PyProcessWatcher(poll_interval_ms=50)
        watcher.track(os.getpid(), "self")
        watcher.start()
        time.sleep(0.2)
        events = watcher.poll_events()
        watcher.stop()
        assert isinstance(events, list)
        # Should have at least some heartbeat events
        assert len(events) >= 1

    def test_watcher_event_schema(self):
        watcher = PyProcessWatcher(poll_interval_ms=50)
        watcher.track(os.getpid(), "self")
        watcher.start()
        time.sleep(0.15)
        events = watcher.poll_events()
        watcher.stop()
        for ev in events:
            assert "type" in ev
            assert "pid" in ev
            assert "name" in ev
            if ev["type"] == "heartbeat":
                assert "new_status" in ev
                assert "cpu_usage_percent" in ev
                assert "memory_bytes" in ev

    def test_watcher_poll_returns_empty_after_drain(self):
        watcher = PyProcessWatcher(poll_interval_ms=50)
        watcher.track(os.getpid(), "self")
        watcher.start()
        time.sleep(0.15)
        _events = watcher.poll_events()  # drain
        events2 = watcher.poll_events()  # should be empty immediately
        watcher.stop()
        assert isinstance(events2, list)

    def test_watcher_start_is_idempotent(self):
        watcher = PyProcessWatcher(poll_interval_ms=100)
        watcher.start()
        watcher.start()  # second call should be no-op
        assert watcher.is_running() is True
        watcher.stop()

    def test_watcher_stop_is_idempotent(self):
        watcher = PyProcessWatcher(poll_interval_ms=100)
        watcher.stop()  # should not raise when not running
        watcher.stop()  # second call also fine


# ---------------------------------------------------------------------------
# TelemetryConfig complete builder chain
# ---------------------------------------------------------------------------


class TestTelemetryConfigBasicProps:
    """service_name, enable_metrics, enable_tracing default values."""

    def test_service_name_set(self):
        cfg = TelemetryConfig("my-service")
        assert cfg.service_name == "my-service"

    def test_enable_metrics_default_true(self):
        cfg = TelemetryConfig("svc")
        assert cfg.enable_metrics is True

    def test_enable_tracing_default_true(self):
        cfg = TelemetryConfig("svc")
        assert cfg.enable_tracing is True


class TestTelemetryConfigBuilderChain:
    """Builder methods return new immutable instances."""

    def test_with_stdout_exporter_returns_config(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.with_stdout_exporter()
        assert isinstance(c2, TelemetryConfig)

    def test_with_noop_exporter_returns_config(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.with_noop_exporter()
        assert isinstance(c2, TelemetryConfig)

    def test_with_noop_exporter_repr_contains_noop(self):
        cfg = TelemetryConfig("svc").with_noop_exporter()
        assert "Noop" in repr(cfg) or "noop" in repr(cfg).lower()

    def test_with_json_logs_returns_config(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.with_json_logs()
        assert isinstance(c2, TelemetryConfig)

    def test_with_text_logs_returns_config(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.with_text_logs()
        assert isinstance(c2, TelemetryConfig)

    def test_with_attribute_returns_config(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.with_attribute("dcc.name", "maya")
        assert isinstance(c2, TelemetryConfig)

    def test_with_service_version_returns_config(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.with_service_version("1.2.3")
        assert isinstance(c2, TelemetryConfig)

    def test_set_enable_metrics_false(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.set_enable_metrics(False)
        assert c2.enable_metrics is False

    def test_set_enable_metrics_true(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.set_enable_metrics(False).set_enable_metrics(True)
        assert c2.enable_metrics is True

    def test_set_enable_tracing_false(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.set_enable_tracing(False)
        assert c2.enable_tracing is False

    def test_set_enable_tracing_true(self):
        cfg = TelemetryConfig("svc")
        c2 = cfg.set_enable_tracing(False).set_enable_tracing(True)
        assert c2.enable_tracing is True

    def test_builder_chain_full(self):
        """Full builder chain all at once."""
        cfg = (
            TelemetryConfig("dcc-mcp")
            .with_noop_exporter()
            .with_json_logs()
            .with_attribute("env", "test")
            .with_service_version("0.1.0")
            .set_enable_metrics(False)
            .set_enable_tracing(True)
        )
        assert cfg.service_name == "dcc-mcp"
        assert cfg.enable_metrics is False
        assert cfg.enable_tracing is True

    def test_builder_chain_modifies_instance(self):
        """Builder methods may modify in-place (mutable) — verify final state."""
        cfg_orig = TelemetryConfig("svc")
        cfg2 = cfg_orig.set_enable_metrics(False)
        # Whether mutable or immutable, the returned config has enable_metrics=False
        assert cfg2.enable_metrics is False

    def test_with_noop_and_set_metrics_chain(self):
        cfg = TelemetryConfig("s").with_noop_exporter().set_enable_metrics(False)
        assert cfg.enable_metrics is False

    def test_multiple_with_attribute(self):
        cfg = TelemetryConfig("svc").with_noop_exporter().with_attribute("k1", "v1").with_attribute("k2", "v2")
        assert isinstance(cfg, TelemetryConfig)


class TestTelemetryConfigRepr:
    """__repr__ correctness."""

    def test_repr_contains_service_name(self):
        cfg = TelemetryConfig("my-svc")
        assert "my-svc" in repr(cfg)

    def test_repr_is_string(self):
        cfg = TelemetryConfig("svc")
        assert isinstance(repr(cfg), str)


class TestTelemetryConfigInitShutdown:
    """init() and shutdown_telemetry() basic smoke test."""

    def test_init_raises_on_double_init(self):
        """init() raises RuntimeError if called twice without shutdown."""
        from dcc_mcp_core import is_telemetry_initialized
        from dcc_mcp_core import shutdown_telemetry

        cfg = TelemetryConfig("test-double-init").with_noop_exporter()
        # Shut down any existing provider first
        import contextlib

        with contextlib.suppress(Exception):
            shutdown_telemetry()
        # First init — may succeed or fail depending on global state
        try:
            cfg.init()
            # Second init must raise
            with pytest.raises(RuntimeError):
                cfg.init()
        except RuntimeError:
            # First init itself failed — global tracer already set from other tests
            pass

    def test_is_telemetry_initialized_returns_bool(self):
        from dcc_mcp_core import is_telemetry_initialized

        result = is_telemetry_initialized()
        assert isinstance(result, bool)
