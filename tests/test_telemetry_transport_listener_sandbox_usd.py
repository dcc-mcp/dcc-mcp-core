"""Tests for TelemetryConfig, TransportAddress, ListenerHandle, SandboxContext.execute_json.

And UsdStage.set/get_attribute deep coverage (iteration 84, +113 tests).
"""

from __future__ import annotations

import math

import pytest

import dcc_mcp_core as m


# ---------------------------------------------------------------------------
# TelemetryConfig
# ---------------------------------------------------------------------------
class TestTelemetryConfigBasic:
    """Basic attribute and construction tests for TelemetryConfig."""

    def test_service_name(self):
        tc = m.TelemetryConfig("my-service")
        assert tc.service_name == "my-service"

    def test_service_name_empty(self):
        tc = m.TelemetryConfig("")
        assert tc.service_name == ""

    def test_service_name_unicode(self):
        tc = m.TelemetryConfig("maya-2025-日本語")
        assert tc.service_name == "maya-2025-日本語"

    def test_enable_metrics_default_true(self):
        tc = m.TelemetryConfig("svc")
        assert tc.enable_metrics is True

    def test_enable_tracing_default_true(self):
        tc = m.TelemetryConfig("svc")
        assert tc.enable_tracing is True

    def test_repr_contains_service_name(self):
        tc = m.TelemetryConfig("my-service")
        assert "my-service" in repr(tc)

    def test_repr_is_string(self):
        tc = m.TelemetryConfig("svc")
        assert isinstance(repr(tc), str)


class TestTelemetryConfigSetters:
    """set_enable_metrics / set_enable_tracing tests."""

    def test_set_enable_metrics_false(self):
        tc = m.TelemetryConfig("svc")
        tc.set_enable_metrics(False)
        assert tc.enable_metrics is False

    def test_set_enable_metrics_true_again(self):
        tc = m.TelemetryConfig("svc")
        tc.set_enable_metrics(False)
        tc.set_enable_metrics(True)
        assert tc.enable_metrics is True

    def test_set_enable_tracing_false(self):
        tc = m.TelemetryConfig("svc")
        tc.set_enable_tracing(False)
        assert tc.enable_tracing is False

    def test_set_enable_tracing_true_again(self):
        tc = m.TelemetryConfig("svc")
        tc.set_enable_tracing(False)
        tc.set_enable_tracing(True)
        assert tc.enable_tracing is True


class TestTelemetryConfigBuilderMethods:
    """Builder-style methods return self for chaining."""

    def test_with_attribute_returns_self(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_attribute("env", "prod")
        assert result is tc

    def test_with_attribute_chain(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_attribute("k1", "v1").with_attribute("k2", "v2")
        assert result is tc

    def test_with_stdout_exporter_returns_self(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_stdout_exporter()
        assert result is tc

    def test_with_noop_exporter_returns_self(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_noop_exporter()
        assert result is tc

    def test_with_service_version_returns_self(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_service_version("1.2.3")
        assert result is tc

    def test_with_json_logs_returns_self(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_json_logs()
        assert result is tc

    def test_with_text_logs_returns_self(self):
        tc = m.TelemetryConfig("svc")
        result = tc.with_text_logs()
        assert result is tc

    def test_full_builder_chain(self):
        tc = m.TelemetryConfig("svc")
        result = (
            tc.with_noop_exporter().with_service_version("0.1.0").with_attribute("dcc.name", "maya").with_text_logs()
        )
        assert result is tc


class TestTelemetryConfigInitShutdown:
    """init() / shutdown() / is_telemetry_initialized() lifecycle tests.

    NOTE: TelemetryConfig.init() installs a *global* tracer/meter provider.
    The underlying tracing crate only allows one installation per process.
    These tests are designed to handle the case where a previous test in
    another module has already installed the global provider.
    """

    def _try_init(self, tc: m.TelemetryConfig) -> bool:
        """Attempt init; return True if succeeded, False if already installed."""
        try:
            tc.init()
            return True
        except RuntimeError:
            return False

    def test_init_or_already_initialized(self):
        """init() either succeeds or raises RuntimeError (already installed)."""
        tc = m.TelemetryConfig("lifecycle-svc")
        tc.with_noop_exporter()
        try:
            tc.init()
            tc.shutdown()  # clean up if we installed
        except RuntimeError:
            pass  # already installed, that's fine

    def test_shutdown_telemetry_no_exception_if_not_init(self):
        # shutdown_telemetry() is a top-level function; if nothing initialized, no crash
        if hasattr(m, "shutdown_telemetry"):
            m.shutdown_telemetry()  # must not raise

    def test_shutdown_returns_none(self):
        # shutdown_telemetry() is a module-level function
        if hasattr(m, "shutdown_telemetry"):
            result = m.shutdown_telemetry()
            assert result is None
        else:
            pytest.skip("shutdown_telemetry not available")

    def test_double_shutdown_no_exception(self):
        if hasattr(m, "shutdown_telemetry"):
            m.shutdown_telemetry()
            m.shutdown_telemetry()  # must not raise
        else:
            pytest.skip("shutdown_telemetry not available")

    def test_second_init_raises_runtime_error(self):
        """If we successfully install first, second install raises RuntimeError."""
        if m.is_telemetry_initialized():
            # Already initialized — verify second init raises
            tc2 = m.TelemetryConfig("second-svc")
            tc2.with_noop_exporter()
            with pytest.raises(RuntimeError):
                tc2.init()
        else:
            tc1 = m.TelemetryConfig("first-svc")
            tc1.with_noop_exporter()
            ok = self._try_init(tc1)
            if ok:
                tc2 = m.TelemetryConfig("second-svc")
                tc2.with_noop_exporter()
                with pytest.raises(RuntimeError):
                    tc2.init()
                # No shutdown method on TelemetryConfig; use module-level if available
                if hasattr(m, "shutdown_telemetry"):
                    m.shutdown_telemetry()
            else:
                pytest.skip("Could not install telemetry provider in this process")

    def test_is_telemetry_initialized_function_exists(self):
        assert hasattr(m, "is_telemetry_initialized")

    def test_is_telemetry_initialized_returns_bool(self):
        result = m.is_telemetry_initialized()
        assert isinstance(result, bool)


# ---------------------------------------------------------------------------
# TransportAddress
# ---------------------------------------------------------------------------
class TestTransportAddressTcp:
    """TCP-specific TransportAddress tests."""

    def test_tcp_scheme(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        assert ta.scheme == "tcp"

    def test_tcp_is_tcp(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        assert ta.is_tcp is True

    def test_tcp_is_local(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        assert ta.is_local is True

    def test_tcp_is_not_named_pipe(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        assert ta.is_named_pipe is False

    def test_tcp_is_not_unix_socket(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        assert ta.is_unix_socket is False

    def test_tcp_to_connection_string(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        s = ta.to_connection_string()
        assert "127.0.0.1" in s
        assert "9001" in s

    def test_tcp_repr(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        r = repr(ta)
        assert isinstance(r, str)
        assert "9001" in r

    def test_tcp_str(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        assert "9001" in str(ta)

    def test_tcp_hash_is_int(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        assert isinstance(hash(ta), int)

    def test_tcp_equality_same(self):
        ta1 = m.TransportAddress.tcp("127.0.0.1", 9001)
        ta2 = m.TransportAddress.tcp("127.0.0.1", 9001)
        assert ta1 == ta2

    def test_tcp_inequality_different_port(self):
        ta1 = m.TransportAddress.tcp("127.0.0.1", 9001)
        ta2 = m.TransportAddress.tcp("127.0.0.1", 9002)
        assert ta1 != ta2

    def test_tcp_parse_equality(self):
        ta1 = m.TransportAddress.tcp("127.0.0.1", 9001)
        ta2 = m.TransportAddress.parse("tcp://127.0.0.1:9001")
        assert ta1 == ta2

    def test_parse_tcp_scheme(self):
        ta = m.TransportAddress.parse("tcp://127.0.0.1:9001")
        assert ta.scheme == "tcp"

    def test_parse_tcp_is_tcp(self):
        ta = m.TransportAddress.parse("tcp://127.0.0.1:9001")
        assert ta.is_tcp is True


class TestTransportAddressNamedPipe:
    """Named Pipe TransportAddress tests."""

    def test_named_pipe_scheme(self):
        ta = m.TransportAddress.named_pipe("my-pipe")
        assert ta.scheme == "pipe"

    def test_named_pipe_is_named_pipe(self):
        ta = m.TransportAddress.named_pipe("my-pipe")
        assert ta.is_named_pipe is True

    def test_named_pipe_is_local(self):
        ta = m.TransportAddress.named_pipe("my-pipe")
        assert ta.is_local is True

    def test_named_pipe_is_not_tcp(self):
        ta = m.TransportAddress.named_pipe("my-pipe")
        assert ta.is_tcp is False

    def test_named_pipe_to_connection_string(self):
        ta = m.TransportAddress.named_pipe("my-pipe")
        s = ta.to_connection_string()
        assert isinstance(s, str)
        assert len(s) > 0

    def test_named_pipe_repr(self):
        ta = m.TransportAddress.named_pipe("my-pipe")
        r = repr(ta)
        assert isinstance(r, str)

    def test_parse_pipe_scheme(self):
        ta = m.TransportAddress.parse("pipe://my-pipe")
        assert ta.scheme == "pipe"

    def test_parse_pipe_is_named_pipe(self):
        ta = m.TransportAddress.parse("pipe://my-pipe")
        assert ta.is_named_pipe is True

    def test_named_pipe_equality(self):
        ta1 = m.TransportAddress.named_pipe("my-pipe")
        ta2 = m.TransportAddress.named_pipe("my-pipe")
        assert ta1 == ta2

    def test_named_pipe_hash(self):
        ta = m.TransportAddress.named_pipe("my-pipe")
        assert isinstance(hash(ta), int)


class TestTransportAddressDefaultLocal:
    """default_local, default_pipe_name, default_unix_socket tests."""

    def test_default_local_is_local(self):
        ta = m.TransportAddress.default_local("maya", 12345)
        assert ta.is_local is True

    def test_default_local_to_connection_string(self):
        ta = m.TransportAddress.default_local("maya", 12345)
        s = ta.to_connection_string()
        assert isinstance(s, str)
        assert len(s) > 0

    def test_default_local_different_dcc_types(self):
        ta_maya = m.TransportAddress.default_local("maya", 100)
        ta_blender = m.TransportAddress.default_local("blender", 100)
        # Different DCC names should produce different addresses
        assert ta_maya != ta_blender

    def test_default_pipe_name_is_transport_address(self):
        ta = m.TransportAddress.default_pipe_name("maya", 12345)
        assert isinstance(ta, m.TransportAddress)

    def test_default_pipe_name_is_named_pipe(self):
        ta = m.TransportAddress.default_pipe_name("maya", 12345)
        assert ta.is_named_pipe is True

    def test_default_pipe_name_conn_string_contains_dcc(self):
        ta = m.TransportAddress.default_pipe_name("maya", 12345)
        s = ta.to_connection_string()
        assert "maya" in s.lower() or "12345" in s

    def test_default_unix_socket_is_transport_address(self):
        ta = m.TransportAddress.default_unix_socket("maya", 12345)
        assert isinstance(ta, m.TransportAddress)


class TestTransportAddressMisc:
    """Miscellaneous TransportAddress tests."""

    def test_parse_invalid_raises(self):
        with pytest.raises((RuntimeError, ValueError)):
            m.TransportAddress.parse("not-valid-uri")

    def test_tcp_can_be_dict_key(self):
        ta = m.TransportAddress.tcp("127.0.0.1", 9001)
        d = {ta: "value"}
        assert d[ta] == "value"

    def test_tcp_in_set(self):
        ta1 = m.TransportAddress.tcp("127.0.0.1", 9001)
        ta2 = m.TransportAddress.tcp("127.0.0.1", 9001)
        s = {ta1, ta2}
        assert len(s) == 1


# ---------------------------------------------------------------------------
# ListenerHandle via IpcListener
# ---------------------------------------------------------------------------
class TestListenerHandle:
    """ListenerHandle attribute and lifecycle tests."""

    @pytest.fixture
    def handle_and_addr(self):
        """Create a ListenerHandle on a free TCP port."""
        for port in range(19300, 19350):
            try:
                addr = m.TransportAddress.tcp("127.0.0.1", port)
                listener = m.IpcListener.bind(addr)
                handle = listener.into_handle()
                yield handle, addr
                return
            except Exception:
                continue
        pytest.skip("No free port available for IpcListener")

    def test_accept_count_initial_zero(self, handle_and_addr):
        handle, _ = handle_and_addr
        assert handle.accept_count == 0

    def test_is_shutdown_initial_false(self, handle_and_addr):
        handle, _ = handle_and_addr
        assert handle.is_shutdown is False

    def test_transport_name_is_string(self, handle_and_addr):
        handle, _ = handle_and_addr
        assert isinstance(handle.transport_name, str)

    def test_transport_name_tcp(self, handle_and_addr):
        handle, _ = handle_and_addr
        assert handle.transport_name == "tcp"

    def test_local_address_is_transport_address(self, handle_and_addr):
        handle, _ = handle_and_addr
        la = handle.local_address()
        assert isinstance(la, m.TransportAddress)

    def test_local_address_is_tcp(self, handle_and_addr):
        handle, _ = handle_and_addr
        la = handle.local_address()
        assert la.is_tcp is True

    def test_repr_is_string(self, handle_and_addr):
        handle, _ = handle_and_addr
        r = repr(handle)
        assert isinstance(r, str)
        assert "tcp" in r

    def test_repr_contains_accept_count(self, handle_and_addr):
        handle, _ = handle_and_addr
        assert "accept_count" in repr(handle) or "0" in repr(handle)

    def test_shutdown_returns_none(self, handle_and_addr):
        handle, _ = handle_and_addr
        result = handle.shutdown()
        assert result is None

    def test_is_shutdown_true_after_shutdown(self, handle_and_addr):
        handle, _ = handle_and_addr
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_double_shutdown_no_exception(self, handle_and_addr):
        handle, _ = handle_and_addr
        handle.shutdown()
        handle.shutdown()  # must not raise

    def test_local_address_available_after_shutdown(self, handle_and_addr):
        handle, _ = handle_and_addr
        handle.shutdown()
        la = handle.local_address()
        assert isinstance(la, m.TransportAddress)

    def test_accept_count_unchanged_after_no_connections(self, handle_and_addr):
        handle, _ = handle_and_addr
        assert handle.accept_count == 0
        handle.shutdown()
        assert handle.accept_count == 0


class TestIpcListenerBind:
    """IpcListener.bind() and basic properties."""

    def test_bind_returns_ipc_listener(self):
        for port in range(19400, 19420):
            try:
                addr = m.TransportAddress.tcp("127.0.0.1", port)
                listener = m.IpcListener.bind(addr)
                assert type(listener).__name__ == "IpcListener"
                return
            except Exception:
                continue
        pytest.skip("No free port")

    def test_listener_transport_name_tcp(self):
        for port in range(19420, 19440):
            try:
                addr = m.TransportAddress.tcp("127.0.0.1", port)
                listener = m.IpcListener.bind(addr)
                assert listener.transport_name == "tcp"
                return
            except Exception:
                continue
        pytest.skip("No free port")

    def test_listener_local_address_matches_bind(self):
        for port in range(19440, 19460):
            try:
                addr = m.TransportAddress.tcp("127.0.0.1", port)
                listener = m.IpcListener.bind(addr)
                la = listener.local_address()
                assert isinstance(la, m.TransportAddress)
                assert la.is_tcp is True
                return
            except Exception:
                continue
        pytest.skip("No free port")

    def test_into_handle_returns_listener_handle(self):
        for port in range(19460, 19480):
            try:
                addr = m.TransportAddress.tcp("127.0.0.1", port)
                listener = m.IpcListener.bind(addr)
                handle = listener.into_handle()
                assert type(handle).__name__ == "ListenerHandle"
                handle.shutdown()
                return
            except Exception:
                continue
        pytest.skip("No free port")


# ---------------------------------------------------------------------------
# SandboxContext.execute_json complete scenarios
# ---------------------------------------------------------------------------
class TestSandboxContextExecuteJson:
    """SandboxContext.execute_json behavior with various policies."""

    def _make_ctx(self, allowed=None, max_actions=None):
        policy = m.SandboxPolicy()
        if allowed is not None:
            policy.allow_actions(allowed)
        if max_actions is not None:
            policy.set_max_actions(max_actions)
        return m.SandboxContext(policy)

    def test_execute_json_allowed_returns_string(self):
        ctx = self._make_ctx(allowed=["render"])
        result = ctx.execute_json("render", '{"frame": 1}')
        assert isinstance(result, str)

    def test_execute_json_allowed_returns_null_string(self):
        ctx = self._make_ctx(allowed=["render"])
        result = ctx.execute_json("render", '{"frame": 1}')
        assert result == "null"

    def test_execute_json_denied_raises_runtime_error(self):
        ctx = self._make_ctx(allowed=["render"])
        with pytest.raises(RuntimeError, match="not allowed"):
            ctx.execute_json("delete", '{"all": true}')

    def test_execute_json_no_whitelist_allows_any(self):
        ctx = self._make_ctx()  # no allow_actions
        result = ctx.execute_json("any_action", "{}")
        assert isinstance(result, str)

    def test_execute_json_empty_params_allowed(self):
        ctx = self._make_ctx(allowed=["render"])
        result = ctx.execute_json("render", "{}")
        assert isinstance(result, str)

    def test_execute_json_max_actions_respected(self):
        ctx = self._make_ctx(max_actions=2)
        ctx.execute_json("render", '{"frame": 1}')
        ctx.execute_json("render", '{"frame": 2}')
        with pytest.raises(RuntimeError, match="maximum action count"):
            ctx.execute_json("render", '{"frame": 3}')

    def test_execute_json_max_actions_exact_boundary(self):
        ctx = self._make_ctx(max_actions=3)
        for i in range(3):
            ctx.execute_json("render", f'{{"frame": {i}}}')
        with pytest.raises(RuntimeError):
            ctx.execute_json("render", '{"frame": 99}')

    def test_execute_json_invalid_json_raises(self):
        ctx = self._make_ctx(allowed=["render"])
        with pytest.raises(RuntimeError):
            ctx.execute_json("render", "not-json")

    def test_execute_json_invalid_json_not_counted_in_audit(self):
        import contextlib

        ctx = self._make_ctx(allowed=["render"])
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("render", "not-json")
        # Invalid JSON calls are not counted in audit entries
        assert len(ctx.audit_log.entries()) == 0

    def test_execute_json_audit_log_increments_on_success(self):
        ctx = self._make_ctx(allowed=["render"])
        ctx.execute_json("render", '{"frame": 1}')
        ctx.execute_json("render", '{"frame": 2}')
        assert len(ctx.audit_log.entries()) == 2

    def test_execute_json_audit_log_increments_on_denial(self):
        import contextlib

        ctx = self._make_ctx(allowed=["render"])
        ctx.execute_json("render", "{}")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("delete", "{}")
        assert len(ctx.audit_log.entries()) == 2

    def test_execute_json_audit_successes_count(self):
        import contextlib

        ctx = self._make_ctx(allowed=["render"])
        ctx.execute_json("render", "{}")
        ctx.execute_json("render", "{}")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("delete", "{}")
        assert len(ctx.audit_log.successes()) == 2

    def test_execute_json_audit_denials_count(self):
        import contextlib

        ctx = self._make_ctx(allowed=["render"])
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("delete", "{}")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("hack", "{}")
        assert len(ctx.audit_log.denials()) == 2

    def test_execute_json_audit_total_equals_success_plus_denials(self):
        import contextlib

        ctx = self._make_ctx(allowed=["render"])
        ctx.execute_json("render", "{}")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("delete", "{}")
        total = len(ctx.audit_log.entries())
        succ = len(ctx.audit_log.successes())
        deni = len(ctx.audit_log.denials())
        assert total == succ + deni

    def test_execute_json_multiple_allowed_actions(self):
        ctx = self._make_ctx(allowed=["render", "export", "snapshot"])
        ctx.execute_json("render", "{}")
        ctx.execute_json("export", "{}")
        ctx.execute_json("snapshot", "{}")
        assert len(ctx.audit_log.successes()) == 3

    def test_execute_json_after_max_limit_always_raises(self):
        ctx = self._make_ctx(max_actions=1)
        ctx.execute_json("render", "{}")
        for _ in range(3):
            with pytest.raises(RuntimeError):
                ctx.execute_json("render", "{}")


# ---------------------------------------------------------------------------
# UsdStage.set_attribute + get_attribute
# ---------------------------------------------------------------------------
class TestUsdStageSetGetAttribute:
    """UsdStage.set_attribute and get_attribute full type coverage."""

    @pytest.fixture
    def stage_with_world(self):
        stage = m.UsdStage("test-stage")
        stage.define_prim("/World", "Xform")
        return stage

    def test_set_get_int_type_name(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myInt", m.VtValue.from_int(42))
        v = stage.get_attribute("/World", "myInt")
        assert v.type_name == "int"

    def test_set_get_int_to_python(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myInt", m.VtValue.from_int(42))
        v = stage.get_attribute("/World", "myInt")
        assert v.to_python() == 42

    def test_set_get_float_type_name(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myFlt", m.VtValue.from_float(1.5))
        v = stage.get_attribute("/World", "myFlt")
        assert v.type_name == "float"

    def test_set_get_float_to_python_approx(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myFlt", m.VtValue.from_float(3.14))
        v = stage.get_attribute("/World", "myFlt")
        # float32 precision loss expected
        assert math.isclose(v.to_python(), 3.14, rel_tol=1e-5)

    def test_set_get_string_type_name(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myStr", m.VtValue.from_string("hello"))
        v = stage.get_attribute("/World", "myStr")
        assert v.type_name == "string"

    def test_set_get_string_to_python(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myStr", m.VtValue.from_string("hello"))
        v = stage.get_attribute("/World", "myStr")
        assert v.to_python() == "hello"

    def test_set_get_bool_true_type_name(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myBool", m.VtValue.from_bool(True))
        v = stage.get_attribute("/World", "myBool")
        assert v.type_name == "bool"

    def test_set_get_bool_true_to_python(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myBool", m.VtValue.from_bool(True))
        v = stage.get_attribute("/World", "myBool")
        assert v.to_python() is True

    def test_set_get_bool_false_to_python(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myBoolF", m.VtValue.from_bool(False))
        v = stage.get_attribute("/World", "myBoolF")
        assert v.to_python() is False

    def test_set_get_token_type_name(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myTok", m.VtValue.from_token("myToken"))
        v = stage.get_attribute("/World", "myTok")
        assert v.type_name == "token"

    def test_set_get_token_to_python(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "myTok", m.VtValue.from_token("myToken"))
        v = stage.get_attribute("/World", "myTok")
        assert v.to_python() == "myToken"

    def test_get_attribute_missing_returns_none(self, stage_with_world):
        stage = stage_with_world
        v = stage.get_attribute("/World", "noSuchAttr")
        assert v is None

    def test_get_attribute_returns_vtvalue_type(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "x", m.VtValue.from_int(1))
        v = stage.get_attribute("/World", "x")
        assert isinstance(v, m.VtValue)

    def test_overwrite_attribute_updates_value(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "num", m.VtValue.from_int(10))
        stage.set_attribute("/World", "num", m.VtValue.from_int(20))
        v = stage.get_attribute("/World", "num")
        assert v.to_python() == 20

    def test_multiple_attributes_independent(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "a", m.VtValue.from_int(1))
        stage.set_attribute("/World", "b", m.VtValue.from_int(2))
        assert stage.get_attribute("/World", "a").to_python() == 1
        assert stage.get_attribute("/World", "b").to_python() == 2

    def test_prim_get_attribute_consistent_with_stage(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "shared", m.VtValue.from_string("data"))
        prim = stage.get_prim("/World")
        via_prim = prim.get_attribute("shared")
        via_stage = stage.get_attribute("/World", "shared")
        assert via_prim.to_python() == via_stage.to_python()

    def test_prim_set_attribute_visible_via_prim(self, stage_with_world):
        stage = stage_with_world
        prim = stage.get_prim("/World")
        prim.set_attribute("pAttr", m.VtValue.from_string("via-prim"))
        v = prim.get_attribute("pAttr")
        assert v is not None
        assert v.to_python() == "via-prim"

    def test_prim_get_attribute_missing_returns_none(self, stage_with_world):
        stage = stage_with_world
        prim = stage.get_prim("/World")
        v = prim.get_attribute("nosuchattr")
        assert v is None

    def test_string_empty_value(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "empty", m.VtValue.from_string(""))
        v = stage.get_attribute("/World", "empty")
        assert v.to_python() == ""

    def test_int_zero(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "zero", m.VtValue.from_int(0))
        v = stage.get_attribute("/World", "zero")
        assert v.to_python() == 0

    def test_int_negative(self, stage_with_world):
        stage = stage_with_world
        stage.set_attribute("/World", "neg", m.VtValue.from_int(-99))
        v = stage.get_attribute("/World", "neg")
        assert v.to_python() == -99
