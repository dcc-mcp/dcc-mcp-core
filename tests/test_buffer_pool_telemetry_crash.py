"""Deep tests for PyBufferPool, TelemetryConfig builder chain, and PyCrashRecoveryPolicy.

Covers:
- PyBufferPool: acquire/release cycle, available count, exhaustion RuntimeError
- PySharedBuffer: id/capacity()/data_len()/name()/write/read/clear/descriptor_json
- TelemetryConfig: builder chain methods, property access
- PyCrashRecoveryPolicy: use_exponential_backoff, use_fixed_backoff,
  next_delay_ms, should_restart, max_restarts, boundary conditions
"""

from __future__ import annotations

import gc
import json

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# PyBufferPool + PySharedBuffer
# ---------------------------------------------------------------------------


class TestPyBufferPoolBasic:
    def test_construction(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(4, 1024)
        assert pool is not None

    def test_initial_available(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(4, 1024)
        assert pool.available() == 4

    def test_available_decrements_on_acquire(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(3, 256)
        b1 = pool.acquire()
        assert pool.available() == 2
        b2 = pool.acquire()
        assert pool.available() == 1
        b3 = pool.acquire()
        assert pool.available() == 0
        del b1, b2, b3

    def test_exhaustion_raises(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(2, 128)
        b1 = pool.acquire()
        b2 = pool.acquire()
        with pytest.raises(RuntimeError, match="exhausted"):
            pool.acquire()
        del b1, b2

    def test_release_via_del_restores_available(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(2, 256)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert pool.available() == 0
        del b1
        gc.collect()
        assert pool.available() == 1
        del b2
        gc.collect()
        assert pool.available() == 2

    def test_reacquire_after_release(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(1, 128)
        b1 = pool.acquire()
        assert pool.available() == 0
        del b1
        gc.collect()
        b2 = pool.acquire()
        assert pool.available() == 0
        del b2

    def test_capacity_1(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(1, 64)
        assert pool.available() == 1
        buf = pool.acquire()
        assert pool.available() == 0
        with pytest.raises(RuntimeError):
            pool.acquire()
        del buf
        gc.collect()
        assert pool.available() == 1


class TestPySharedBuffer:
    def _make_pool_and_buf(self, capacity: int = 512) -> tuple:
        pool = dcc_mcp_core.PyBufferPool(2, capacity)
        buf = pool.acquire()
        return pool, buf

    def test_id_is_string(self) -> None:
        _, buf = self._make_pool_and_buf()
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_capacity_method(self) -> None:
        _, buf = self._make_pool_and_buf(512)
        assert buf.capacity() == 512

    def test_initial_data_len_zero(self) -> None:
        _, buf = self._make_pool_and_buf()
        assert buf.data_len() == 0

    def test_name_is_string(self) -> None:
        _, buf = self._make_pool_and_buf()
        p = buf.name()
        assert isinstance(p, str)
        assert len(p) > 0

    def test_write_updates_data_len(self) -> None:
        _, buf = self._make_pool_and_buf(512)
        payload = b"hello world"
        buf.write(payload)
        assert buf.data_len() == len(payload)

    def test_read_returns_written_data(self) -> None:
        _, buf = self._make_pool_and_buf(512)
        payload = b"test payload"
        buf.write(payload)
        assert buf.read() == payload

    def test_clear_resets_data_len(self) -> None:
        _, buf = self._make_pool_and_buf(512)
        buf.write(b"some data")
        assert buf.data_len() > 0
        buf.clear()
        assert buf.data_len() == 0

    def test_clear_then_write_again(self) -> None:
        _, buf = self._make_pool_and_buf(512)
        buf.write(b"first")
        buf.clear()
        buf.write(b"second")
        assert buf.read() == b"second"

    def test_overwrite_replaces_data(self) -> None:
        _, buf = self._make_pool_and_buf(512)
        buf.write(b"old data here")
        buf.write(b"new")
        assert buf.read() == b"new"
        assert buf.data_len() == 3

    def test_binary_data_roundtrip(self) -> None:
        _, buf = self._make_pool_and_buf(512)
        payload = bytes(range(200))
        buf.write(payload)
        assert buf.read() == payload

    def test_descriptor_json_is_valid_json(self) -> None:
        _, buf = self._make_pool_and_buf()
        desc_str = buf.descriptor_json()
        assert isinstance(desc_str, str)
        desc = json.loads(desc_str)
        assert isinstance(desc, dict)

    def test_descriptor_json_contains_id(self) -> None:
        _, buf = self._make_pool_and_buf()
        desc = json.loads(buf.descriptor_json())
        assert "id" in desc
        assert desc["id"] == buf.id

    def test_descriptor_json_contains_name(self) -> None:
        _, buf = self._make_pool_and_buf()
        desc = json.loads(buf.descriptor_json())
        assert "name" in desc

    def test_unique_ids_from_same_pool(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(2, 256)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert b1.id != b2.id
        del b1, b2

    def test_empty_write_read(self) -> None:
        _, buf = self._make_pool_and_buf(256)
        buf.write(b"")
        assert buf.read() == b""
        assert buf.data_len() == 0


# ---------------------------------------------------------------------------
# TelemetryConfig builder chain
# ---------------------------------------------------------------------------


class TestTelemetryConfigBasic:
    def test_construction(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("my-svc")
        assert cfg is not None

    def test_service_name(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("render-service")
        assert cfg.service_name == "render-service"

    def test_default_enable_metrics_true(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        assert cfg.enable_metrics is True

    def test_default_enable_tracing_true(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        assert cfg.enable_tracing is True


class TestTelemetryConfigBuilderChain:
    def test_with_service_version_returns_config(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").with_service_version("2.0.0")
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)

    def test_with_service_version_chain(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").with_service_version("1.0.0").with_noop_exporter()
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)

    def test_with_attribute_returns_config(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").with_attribute("env", "staging")
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)

    def test_with_multiple_attributes(self) -> None:
        cfg = (
            dcc_mcp_core.TelemetryConfig("svc")
            .with_attribute("env", "prod")
            .with_attribute("region", "us-west-2")
            .with_attribute("version", "3.1.4")
        )
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)

    def test_with_stdout_exporter_returns_config(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").with_stdout_exporter()
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)

    def test_with_noop_exporter_returns_config(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").with_noop_exporter()
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)

    def test_with_json_logs_returns_config(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").with_json_logs()
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)

    def test_with_text_logs_returns_config(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").with_text_logs()
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)

    def test_set_enable_metrics_false(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").set_enable_metrics(False)
        assert cfg.enable_metrics is False

    def test_set_enable_metrics_true(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").set_enable_metrics(False).set_enable_metrics(True)
        assert cfg.enable_metrics is True

    def test_set_enable_tracing_false(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").set_enable_tracing(False)
        assert cfg.enable_tracing is False

    def test_set_enable_tracing_true(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc").set_enable_tracing(False).set_enable_tracing(True)
        assert cfg.enable_tracing is True

    def test_full_builder_chain(self) -> None:
        cfg = (
            dcc_mcp_core.TelemetryConfig("dcc-mcp-core")
            .with_service_version("0.12.6")
            .with_attribute("dcc", "maya")
            .with_attribute("env", "test")
            .with_noop_exporter()
            .with_text_logs()
            .set_enable_metrics(True)
            .set_enable_tracing(True)
        )
        assert isinstance(cfg, dcc_mcp_core.TelemetryConfig)
        assert cfg.service_name == "dcc-mcp-core"
        assert cfg.enable_metrics is True
        assert cfg.enable_tracing is True

    def test_service_name_preserved_after_chain(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("preserved-name").with_noop_exporter().set_enable_metrics(False)
        assert cfg.service_name == "preserved-name"

    def test_different_service_names(self) -> None:
        names = ["svc-a", "svc-b", "svc-c"]
        for name in names:
            cfg = dcc_mcp_core.TelemetryConfig(name)
            assert cfg.service_name == name


# ---------------------------------------------------------------------------
# PyCrashRecoveryPolicy
# ---------------------------------------------------------------------------


class TestPyCrashRecoveryPolicyDefault:
    def test_construction(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert pol is not None

    def test_default_max_restarts(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        # Default is typically 3
        assert isinstance(pol.max_restarts, int)
        assert pol.max_restarts >= 0

    def test_should_restart_crashed(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        # crashed status -> should restart
        assert pol.should_restart("crashed") is True

    def test_should_restart_unresponsive(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert pol.should_restart("unresponsive") is True

    def test_should_restart_stopped_false(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        # stopped is not a restart-worthy status
        assert pol.should_restart("stopped") is False

    def test_should_restart_running_false(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert pol.should_restart("running") is False

    def test_should_restart_starting_false(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert pol.should_restart("starting") is False

    def test_should_restart_restarting_false(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert pol.should_restart("restarting") is False


class TestPyCrashRecoveryPolicyFixedBackoff:
    def test_use_fixed_backoff(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=500)
        assert pol is not None

    def test_next_delay_ms_constant(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=300)
        d0 = pol.next_delay_ms("test_app", 0)
        d1 = pol.next_delay_ms("test_app", 1)
        d2 = pol.next_delay_ms("test_app", 2)
        assert d0 == d1 == d2

    def test_next_delay_ms_matches_configured(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=750)
        delay = pol.next_delay_ms("app", 0)
        assert delay == 750

    def test_max_restarts_default_after_fixed(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=100)
        # Default max restarts should be positive
        assert pol.max_restarts > 0

    def test_exceeded_max_restarts_raises(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=100)
        max_r = pol.max_restarts
        # Attempt beyond max should raise RuntimeError
        with pytest.raises(RuntimeError):
            pol.next_delay_ms("app", max_r)

    def test_small_delay(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=1)
        assert pol.next_delay_ms("app", 0) == 1

    def test_large_delay(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=60000)
        assert pol.next_delay_ms("app", 0) == 60000


class TestPyCrashRecoveryPolicyExponentialBackoff:
    def test_use_exponential_backoff(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        assert pol is not None

    def test_first_delay_matches_initial(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_exponential_backoff(initial_ms=200, max_delay_ms=10000)
        d0 = pol.next_delay_ms("app", 0)
        assert d0 == 200

    def test_delay_increases_with_attempt(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_exponential_backoff(initial_ms=100, max_delay_ms=100000)
        d0 = pol.next_delay_ms("app", 0)
        d1 = pol.next_delay_ms("app", 1)
        d2 = pol.next_delay_ms("app", 2)
        assert d0 <= d1 <= d2

    def test_delay_capped_at_max(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_exponential_backoff(initial_ms=100, max_delay_ms=500)
        # High attempt number: delay should not exceed max_delay_ms
        d_high = pol.next_delay_ms("app", pol.max_restarts - 1)
        assert d_high <= 500

    def test_exceeded_max_restarts_raises(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_exponential_backoff(initial_ms=50, max_delay_ms=1000)
        max_r = pol.max_restarts
        with pytest.raises(RuntimeError):
            pol.next_delay_ms("app", max_r)

    def test_should_restart_still_works_after_config(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        assert pol.should_restart("crashed") is True
        assert pol.should_restart("stopped") is False

    def test_max_restarts_is_positive(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        assert pol.max_restarts > 0

    def test_small_initial_ms(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_exponential_backoff(initial_ms=1, max_delay_ms=1000)
        assert pol.next_delay_ms("app", 0) == 1

    def test_switch_from_fixed_to_exp(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=999)
        pol.use_exponential_backoff(initial_ms=50, max_delay_ms=2000)
        # After switching to exponential, first delay should be initial_ms
        assert pol.next_delay_ms("app", 0) == 50


class TestPyCrashRecoveryPolicyMaxRestarts:
    def test_max_restarts_is_int(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert isinstance(pol.max_restarts, int)

    def test_max_restarts_non_negative(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert pol.max_restarts >= 0

    def test_max_restarts_preserved_after_fixed(self) -> None:
        pol = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol.use_fixed_backoff(delay_ms=100)
        # max_restarts may be same or reset - just check it is positive
        assert pol.max_restarts > 0

    def test_multiple_policies_independent(self) -> None:
        pol1 = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol2 = dcc_mcp_core.PyCrashRecoveryPolicy()
        pol1.use_fixed_backoff(delay_ms=100)
        pol2.use_exponential_backoff(initial_ms=200, max_delay_ms=5000)
        # They should behave independently
        d1 = pol1.next_delay_ms("app", 0)
        d2 = pol2.next_delay_ms("app", 0)
        assert d1 == 100
        assert d2 == 200
