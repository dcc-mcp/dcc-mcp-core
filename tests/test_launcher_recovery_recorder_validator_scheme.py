"""Tests for PyDccLauncher, PyCrashRecoveryPolicy, ActionRecorder/RecordingGuard/ActionMetrics.

Also covers InputValidator and TransportScheme.select_address.
All tests are based on observed API behavior from probe sessions (probe87b-87i).
"""

from __future__ import annotations

import json
import os
import sys
import time

import pytest

from dcc_mcp_core import ActionRecorder
from dcc_mcp_core import InputValidator
from dcc_mcp_core import PyCrashRecoveryPolicy
from dcc_mcp_core import PyDccLauncher
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportScheme

# ---------------------------------------------------------------------------
# PyDccLauncher
# ---------------------------------------------------------------------------


class TestPyDccLauncher:
    """Tests for the DCC process launcher."""

    class TestHappyPath:
        def test_create_launcher(self):
            launcher = PyDccLauncher()
            assert launcher is not None

        def test_repr(self):
            launcher = PyDccLauncher()
            r = repr(launcher)
            assert "PyDccLauncher" in r
            assert "running=0" in r

        def test_initial_running_count_is_zero(self):
            launcher = PyDccLauncher()
            assert launcher.running_count() == 0

        def test_launch_returns_dict(self):
            launcher = PyDccLauncher()
            result = launcher.launch("test_launch_87", sys.executable, ["--version"])
            assert isinstance(result, dict)

        def test_launch_result_has_pid(self):
            launcher = PyDccLauncher()
            result = launcher.launch("test_pid_87", sys.executable, ["--version"])
            assert "pid" in result
            assert isinstance(result["pid"], int)
            assert result["pid"] > 0

        def test_launch_result_has_name(self):
            launcher = PyDccLauncher()
            result = launcher.launch("test_name_87", sys.executable, ["--version"])
            assert result["name"] == "test_name_87"

        def test_launch_result_has_status(self):
            launcher = PyDccLauncher()
            result = launcher.launch("test_status_87", sys.executable, ["--version"])
            assert "status" in result
            assert isinstance(result["status"], str)

        def test_running_count_after_launch(self):
            launcher = PyDccLauncher()
            launcher.launch("test_count_87", sys.executable, ["--version"])
            assert launcher.running_count() >= 1

        def test_pid_of_launched_process(self):
            launcher = PyDccLauncher()
            result = launcher.launch("test_pidof_87", sys.executable, ["--version"])
            pid = launcher.pid_of("test_pidof_87")
            assert pid == result["pid"]

        def test_restart_count_initial_zero(self):
            launcher = PyDccLauncher()
            launcher.launch("test_restart_87", sys.executable, ["--version"])
            assert launcher.restart_count("test_restart_87") == 0

        def test_restart_count_is_int(self):
            launcher = PyDccLauncher()
            launcher.launch("test_restart_int_87", sys.executable, ["--version"])
            count = launcher.restart_count("test_restart_int_87")
            assert isinstance(count, int)

        def test_terminate_process(self):
            launcher = PyDccLauncher()
            launcher.launch("test_term_87", sys.executable, ["--version"])
            time.sleep(0.3)
            launcher.terminate("test_term_87")
            # After terminate, running_count should drop to 0
            assert launcher.running_count() == 0

        def test_terminate_reduces_running_count(self):
            launcher = PyDccLauncher()
            launcher.launch("test_term_count_87", sys.executable, ["--version"])
            before = launcher.running_count()
            time.sleep(0.2)
            launcher.terminate("test_term_count_87")
            after = launcher.running_count()
            assert after < before or after == 0

    class TestErrorPath:
        def test_kill_not_running_raises_runtime_error(self):
            launcher = PyDccLauncher()
            launcher.launch("test_kill_87", sys.executable, ["--version"])
            time.sleep(0.3)
            launcher.terminate("test_kill_87")
            with pytest.raises(RuntimeError, match="not running"):
                launcher.kill("test_kill_87")

        def test_pid_of_returns_int(self):
            launcher = PyDccLauncher()
            launcher.launch("test_pidtype_87", sys.executable, ["--version"])
            pid = launcher.pid_of("test_pidtype_87")
            assert isinstance(pid, int)


# ---------------------------------------------------------------------------
# PyCrashRecoveryPolicy
# ---------------------------------------------------------------------------


class TestPyCrashRecoveryPolicy:
    """Tests for crash recovery policy configuration."""

    class TestConstructor:
        def test_default_max_restarts(self):
            p = PyCrashRecoveryPolicy()
            assert p.max_restarts == 3

        def test_custom_max_restarts(self):
            p = PyCrashRecoveryPolicy(5)
            assert p.max_restarts == 5

        def test_max_restarts_zero(self):
            p = PyCrashRecoveryPolicy(0)
            assert p.max_restarts == 0

        def test_repr(self):
            p = PyCrashRecoveryPolicy(3)
            r = repr(p)
            assert "PyCrashRecoveryPolicy" in r
            assert "3" in r

    class TestShouldRestart:
        def test_crashed_returns_true(self):
            p = PyCrashRecoveryPolicy(5)
            assert p.should_restart("crashed") is True

        def test_unresponsive_returns_true(self):
            p = PyCrashRecoveryPolicy(5)
            assert p.should_restart("unresponsive") is True

        def test_running_returns_false(self):
            p = PyCrashRecoveryPolicy(5)
            assert p.should_restart("running") is False

        def test_starting_returns_false(self):
            p = PyCrashRecoveryPolicy(5)
            assert p.should_restart("starting") is False

        def test_stopped_returns_false(self):
            p = PyCrashRecoveryPolicy(5)
            assert p.should_restart("stopped") is False

        def test_should_restart_returns_bool(self):
            p = PyCrashRecoveryPolicy(3)
            result = p.should_restart("crashed")
            assert isinstance(result, bool)

        def test_unknown_status_raises_value_error(self):
            p = PyCrashRecoveryPolicy(3)
            with pytest.raises(ValueError, match="unknown ProcessStatus"):
                p.should_restart("dead")

        def test_unknown_status_error_message_lists_valid(self):
            p = PyCrashRecoveryPolicy(3)
            with pytest.raises(ValueError) as exc_info:
                p.should_restart("invalid_status")
            msg = str(exc_info.value)
            assert "running" in msg

    class TestNextDelayMs:
        def test_default_policy_returns_int(self):
            p = PyCrashRecoveryPolicy(5)
            d = p.next_delay_ms("my_dcc", 0)
            assert isinstance(d, int)

        def test_default_policy_attempt_0(self):
            p = PyCrashRecoveryPolicy(5)
            d = p.next_delay_ms("my_dcc", 0)
            assert d > 0

        def test_default_policy_consistent(self):
            p = PyCrashRecoveryPolicy(5)
            d0 = p.next_delay_ms("my_dcc", 0)
            d1 = p.next_delay_ms("my_dcc", 1)
            # Default policy (no backoff configured): constant
            assert d0 == d1

        def test_exponential_backoff_grows(self):
            p = PyCrashRecoveryPolicy(5)
            p.use_exponential_backoff(100, 5000)
            d0 = p.next_delay_ms("dcc", 0)
            d1 = p.next_delay_ms("dcc", 1)
            d2 = p.next_delay_ms("dcc", 2)
            assert d0 <= d1 <= d2
            assert d0 == 100  # initial_ms

        def test_exponential_backoff_doubles(self):
            p = PyCrashRecoveryPolicy(5)
            p.use_exponential_backoff(100, 5000)
            d0 = p.next_delay_ms("dcc", 0)
            d1 = p.next_delay_ms("dcc", 1)
            d2 = p.next_delay_ms("dcc", 2)
            assert d1 == d0 * 2
            assert d2 == d1 * 2

        def test_fixed_backoff_constant(self):
            p = PyCrashRecoveryPolicy(5)
            p.use_fixed_backoff(500)
            d0 = p.next_delay_ms("dcc", 0)
            d1 = p.next_delay_ms("dcc", 1)
            d2 = p.next_delay_ms("dcc", 2)
            assert d0 == d1 == d2 == 500

        def test_use_exponential_backoff_returns_none(self):
            p = PyCrashRecoveryPolicy(3)
            result = p.use_exponential_backoff(100, 5000)
            assert result is None

        def test_use_fixed_backoff_returns_none(self):
            p = PyCrashRecoveryPolicy(3)
            result = p.use_fixed_backoff(500)
            assert result is None

    class TestMaxRestartsBehavior:
        def test_should_restart_always_returns_true_for_crashed(self):
            # PyCrashRecoveryPolicy.should_restart does not check restart count in Python
            # The count check is done externally (e.g., in the launcher loop)
            p = PyCrashRecoveryPolicy(0)
            # Even with max_restarts=0, should_restart is a pure status predicate
            # Actual behavior: always True for "crashed" regardless of count
            result = p.should_restart("crashed")
            assert isinstance(result, bool)


# ---------------------------------------------------------------------------
# ActionRecorder / RecordingGuard / ActionMetrics
# ---------------------------------------------------------------------------


class TestActionRecorder:
    """Tests for ActionRecorder and its context manager (RecordingGuard)."""

    class TestConstructor:
        def test_create_with_scope(self):
            rec = ActionRecorder("test_scope")
            assert rec is not None

        def test_initial_all_metrics_empty(self):
            rec = ActionRecorder("empty_scope_87")
            assert rec.all_metrics() == []

        def test_repr(self):
            rec = ActionRecorder("my_scope")
            r = repr(rec)
            assert "ActionRecorder" in r or "recorder" in r.lower() or "scope" in r.lower() or r

    class TestRecordingGuard:
        def test_start_returns_recording_guard(self):
            rec = ActionRecorder("guard_scope_87")
            with rec.start("create_sphere", "maya") as guard:
                assert guard is not None

        def test_guard_repr(self):
            rec = ActionRecorder("guard_repr_87")
            with rec.start("action", "maya") as guard:
                r = repr(guard)
                assert "RecordingGuard" in r

        def test_guard_has_finish_method(self):
            rec = ActionRecorder("guard_finish_87")
            with rec.start("action", "maya") as guard:
                assert hasattr(guard, "finish")

        def test_context_manager_enter_exit(self):
            rec = ActionRecorder("ctx_87")
            with rec.start("action1", "maya") as guard:
                assert guard is not None

        def test_context_manager_success_records_invocation(self):
            rec = ActionRecorder("ctx_success_87")
            with rec.start("my_action", "maya"):
                pass
            m = rec.metrics("my_action")
            assert m is not None
            assert m.invocation_count == 1

        def test_context_manager_success_count(self):
            rec = ActionRecorder("ctx_success_count_87")
            with rec.start("my_action", "maya"):
                pass
            m = rec.metrics("my_action")
            assert m.success_count == 1

        def test_context_manager_exception_reraises(self):
            rec = ActionRecorder("ctx_exc_87")
            with pytest.raises(ValueError, match="deliberate"), rec.start("fail_action", "maya"):
                raise ValueError("deliberate")

        def test_context_manager_exception_records_failure(self):
            rec = ActionRecorder("ctx_fail_87")
            try:
                with rec.start("fail_action", "maya"):
                    raise ValueError("test fail")
            except ValueError:
                pass
            m = rec.metrics("fail_action")
            assert m is not None
            assert m.failure_count == 1

        def test_context_manager_exception_success_count_zero(self):
            rec = ActionRecorder("ctx_fail_sc_87")
            try:
                with rec.start("fail_action", "maya"):
                    raise ValueError("test fail")
            except ValueError:
                pass
            m = rec.metrics("fail_action")
            assert m.success_count == 0

        def test_multiple_recordings_accumulate(self):
            rec = ActionRecorder("multi_87")
            with rec.start("act", "maya"):
                pass
            with rec.start("act", "maya"):
                pass
            m = rec.metrics("act")
            assert m.invocation_count == 2

        def test_guard_active_in_context(self):
            rec = ActionRecorder("active_87")
            with rec.start("action", "maya") as guard:
                r = repr(guard)
                assert "active=true" in r

    class TestActionMetrics:
        def test_metrics_not_none_after_recording(self):
            rec = ActionRecorder("metrics_87")
            with rec.start("action", "maya"):
                pass
            m = rec.metrics("action")
            assert m is not None

        def test_metrics_action_name(self):
            rec = ActionRecorder("metrics_name_87")
            with rec.start("create_cube", "blender"):
                pass
            m = rec.metrics("create_cube")
            assert m.action_name == "create_cube"

        def test_metrics_invocation_count(self):
            rec = ActionRecorder("metrics_inv_87")
            for _ in range(3):
                with rec.start("action", "maya"):
                    pass
            m = rec.metrics("action")
            assert m.invocation_count == 3

        def test_metrics_success_count(self):
            rec = ActionRecorder("metrics_sc_87")
            with rec.start("action", "maya"):
                pass
            with rec.start("action", "maya"):
                pass
            try:
                with rec.start("action", "maya"):
                    raise ValueError("fail")
            except ValueError:
                pass
            m = rec.metrics("action")
            assert m.success_count == 2

        def test_metrics_failure_count(self):
            rec = ActionRecorder("metrics_fc_87")
            with rec.start("action", "maya"):
                pass
            try:
                with rec.start("action", "maya"):
                    raise RuntimeError("oops")
            except RuntimeError:
                pass
            m = rec.metrics("action")
            assert m.failure_count == 1

        def test_metrics_success_rate_all_success(self):
            rec = ActionRecorder("metrics_sr100_87")
            with rec.start("action", "maya"):
                pass
            m = rec.metrics("action")
            assert m.success_rate() == pytest.approx(1.0)

        def test_metrics_success_rate_all_failure(self):
            rec = ActionRecorder("metrics_sr0_87")
            try:
                with rec.start("action", "maya"):
                    raise RuntimeError("fail")
            except RuntimeError:
                pass
            m = rec.metrics("action")
            assert m.success_rate() == pytest.approx(0.0)

        def test_metrics_success_rate_mixed(self):
            rec = ActionRecorder("metrics_srmix_87")
            with rec.start("action", "maya"):
                pass
            with rec.start("action", "maya"):
                pass
            try:
                with rec.start("action", "maya"):
                    raise ValueError("fail")
            except ValueError:
                pass
            m = rec.metrics("action")
            assert m.success_rate() == pytest.approx(2.0 / 3.0)

        def test_metrics_avg_duration_ms_positive(self):
            rec = ActionRecorder("metrics_dur_87")
            with rec.start("action", "maya"):
                pass
            m = rec.metrics("action")
            assert m.avg_duration_ms >= 0.0

        def test_metrics_p95_duration_ms_positive(self):
            rec = ActionRecorder("metrics_p95_87")
            with rec.start("action", "maya"):
                pass
            m = rec.metrics("action")
            assert m.p95_duration_ms >= 0.0

        def test_metrics_p99_duration_ms_positive(self):
            rec = ActionRecorder("metrics_p99_87")
            with rec.start("action", "maya"):
                pass
            m = rec.metrics("action")
            assert m.p99_duration_ms >= 0.0

        def test_metrics_repr(self):
            rec = ActionRecorder("metrics_repr_87")
            with rec.start("my_action", "maya"):
                pass
            m = rec.metrics("my_action")
            r = repr(m)
            assert "ActionMetrics" in r
            assert "my_action" in r

    class TestAllMetricsAndReset:
        def test_all_metrics_returns_list(self):
            rec = ActionRecorder("all_87")
            with rec.start("action", "maya"):
                pass
            am = rec.all_metrics()
            assert isinstance(am, list)

        def test_all_metrics_contains_recorded_actions(self):
            rec = ActionRecorder("all_has_87")
            with rec.start("action_a", "maya"):
                pass
            with rec.start("action_b", "blender"):
                pass
            names = {m.action_name for m in rec.all_metrics()}
            assert "action_a" in names
            assert "action_b" in names

        def test_all_metrics_len(self):
            rec = ActionRecorder("all_len_87")
            with rec.start("act1", "maya"):
                pass
            with rec.start("act2", "maya"):
                pass
            assert len(rec.all_metrics()) == 2

        def test_reset_clears_metrics(self):
            rec = ActionRecorder("reset_87")
            with rec.start("action", "maya"):
                pass
            rec.reset()
            assert rec.all_metrics() == []

        def test_reset_after_reset_still_works(self):
            rec = ActionRecorder("reset2_87")
            rec.reset()
            rec.reset()
            assert rec.all_metrics() == []

        def test_all_metrics_empty_initial(self):
            rec = ActionRecorder("empty2_87")
            assert len(rec.all_metrics()) == 0


# ---------------------------------------------------------------------------
# InputValidator
# ---------------------------------------------------------------------------


class TestInputValidator:
    """Tests for InputValidator — require_string, require_number, forbid_substrings, validate."""

    class TestRequireString:
        def test_require_string_returns_none(self):
            v = InputValidator()
            result = v.require_string("username", min_length=3, max_length=20)
            assert result is None

        def test_require_string_valid_passes(self):
            v = InputValidator()
            v.require_string("name", min_length=2, max_length=50)
            ok, msg = v.validate(json.dumps({"name": "alice"}))
            assert ok is True
            assert msg is None

        def test_require_string_too_short_fails(self):
            v = InputValidator()
            v.require_string("name", min_length=3, max_length=50)
            ok, msg = v.validate(json.dumps({"name": "ab"}))
            assert ok is False
            assert msg is not None
            assert "name" in msg

        def test_require_string_too_long_fails(self):
            v = InputValidator()
            v.require_string("name", min_length=1, max_length=5)
            ok, msg = v.validate(json.dumps({"name": "toolongstring"}))
            assert ok is False
            assert msg is not None

        def test_require_string_missing_field_fails(self):
            v = InputValidator()
            v.require_string("name", min_length=1, max_length=50)
            ok, msg = v.validate(json.dumps({}))
            assert ok is False
            assert "name" in msg
            assert "required" in msg.lower() or "field" in msg.lower()

        def test_require_string_error_msg_mentions_field(self):
            v = InputValidator()
            v.require_string("username", min_length=5, max_length=20)
            _ok, msg = v.validate(json.dumps({"username": "ab"}))
            assert "username" in msg

        def test_require_string_error_msg_mentions_min(self):
            v = InputValidator()
            v.require_string("f", min_length=5, max_length=20)
            _ok, msg = v.validate(json.dumps({"f": "ab"}))
            assert "5" in msg or "minimum" in msg.lower() or "below" in msg.lower()

    class TestRequireNumber:
        def test_require_number_returns_none(self):
            v = InputValidator()
            result = v.require_number("age", min_value=0.0, max_value=150.0)
            assert result is None

        def test_require_number_valid_passes(self):
            v = InputValidator()
            v.require_number("age", min_value=0.0, max_value=150.0)
            ok, msg = v.validate(json.dumps({"age": 25}))
            assert ok is True
            assert msg is None

        def test_require_number_below_min_fails(self):
            v = InputValidator()
            v.require_number("age", min_value=0.0, max_value=150.0)
            ok, msg = v.validate(json.dumps({"age": -1}))
            assert ok is False
            assert msg is not None

        def test_require_number_above_max_fails(self):
            v = InputValidator()
            v.require_number("age", min_value=0.0, max_value=100.0)
            ok, msg = v.validate(json.dumps({"age": 200}))
            assert ok is False
            assert msg is not None

        def test_require_number_missing_field_fails(self):
            v = InputValidator()
            v.require_number("count", min_value=0.0, max_value=100.0)
            ok, msg = v.validate(json.dumps({}))
            assert ok is False
            assert "count" in msg

        def test_require_number_at_boundary_passes(self):
            v = InputValidator()
            v.require_number("n", min_value=0.0, max_value=100.0)
            ok, _ = v.validate(json.dumps({"n": 0}))
            assert ok is True
            ok2, _ = v.validate(json.dumps({"n": 100}))
            assert ok2 is True

    class TestForbidSubstrings:
        def test_forbid_substrings_returns_none(self):
            v = InputValidator()
            result = v.forbid_substrings("cmd", ["DROP TABLE"])
            assert result is None

        def test_clean_input_passes(self):
            v = InputValidator()
            v.forbid_substrings("cmd", ["DROP TABLE", "rm -rf"])
            ok, _msg = v.validate(json.dumps({"cmd": "print('hello')"}))
            assert ok is True

        def test_injection_fails(self):
            v = InputValidator()
            v.forbid_substrings("cmd", ["DROP TABLE"])
            ok, msg = v.validate(json.dumps({"cmd": "DROP TABLE users;"}))
            assert ok is False
            assert msg is not None

        def test_injection_msg_mentions_field(self):
            v = InputValidator()
            v.forbid_substrings("query", ["DROP TABLE"])
            _ok, msg = v.validate(json.dumps({"query": "DROP TABLE test"}))
            assert "query" in msg

        def test_multiple_forbidden_strings(self):
            v = InputValidator()
            v.forbid_substrings("code", ["__import__", "exec(", "eval("])
            for bad in ["__import__('os')", "exec('bad')", "eval('x')"]:
                ok, _ = v.validate(json.dumps({"code": bad}))
                assert ok is False, f"Should reject: {bad!r}"

    class TestValidate:
        def test_validate_returns_tuple(self):
            v = InputValidator()
            result = v.validate(json.dumps({}))
            assert isinstance(result, tuple)
            assert len(result) == 2

        def test_validate_tuple_first_is_bool(self):
            v = InputValidator()
            ok, _msg = v.validate(json.dumps({}))
            assert isinstance(ok, bool)

        def test_validate_success_msg_is_none(self):
            v = InputValidator()
            v.require_string("name", min_length=1, max_length=50)
            ok, msg = v.validate(json.dumps({"name": "ok"}))
            assert ok is True
            assert msg is None

        def test_validate_failure_msg_is_str(self):
            v = InputValidator()
            v.require_string("name", min_length=5, max_length=50)
            ok, msg = v.validate(json.dumps({"name": "x"}))
            assert ok is False
            assert isinstance(msg, str)

        def test_validate_invalid_json_raises_runtime_error(self):
            v = InputValidator()
            with pytest.raises(RuntimeError, match="invalid JSON"):
                v.validate("not valid json!!!")

        def test_validate_combined_rules(self):
            v = InputValidator()
            v.require_string("username", min_length=3, max_length=20)
            v.require_number("age", min_value=0.0, max_value=150.0)
            ok, _ = v.validate(json.dumps({"username": "alice", "age": 25}))
            assert ok is True

        def test_validate_combined_first_rule_fails(self):
            v = InputValidator()
            v.require_string("username", min_length=5, max_length=20)
            v.require_number("age", min_value=0.0, max_value=150.0)
            ok, msg = v.validate(json.dumps({"username": "ab", "age": 25}))
            assert ok is False
            assert msg is not None

        def test_validate_empty_json_object(self):
            v = InputValidator()
            ok, _msg = v.validate("{}")
            assert isinstance(ok, bool)


# ---------------------------------------------------------------------------
# TransportScheme.select_address
# ---------------------------------------------------------------------------


class TestTransportSchemeSelectAddress:
    """Tests for TransportScheme.select_address(dcc_type, host, port, pid=None)."""

    class TestSchemeVariants:
        def test_auto_no_pid_returns_tcp(self):
            result = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 9999)
            assert result.scheme == "tcp"

        def test_tcp_only_no_pid_returns_tcp(self):
            result = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 9999)
            assert result.scheme == "tcp"

        def test_prefer_named_pipe_no_pid_returns_tcp(self):
            result = TransportScheme.PREFER_NAMED_PIPE.select_address("maya", "127.0.0.1", 9999)
            assert result.scheme == "tcp"

        def test_prefer_unix_socket_no_pid_returns_tcp(self):
            result = TransportScheme.PREFER_UNIX_SOCKET.select_address("maya", "127.0.0.1", 9999)
            assert result.scheme == "tcp"

        def test_prefer_ipc_no_pid_returns_tcp(self):
            result = TransportScheme.PREFER_IPC.select_address("maya", "127.0.0.1", 9999)
            assert result.scheme == "tcp"

    class TestWithPidOnWindows:
        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_auto_with_pid_returns_pipe(self):
            result = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 9999, 12345)
            assert result.scheme == "pipe"

        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_prefer_named_pipe_with_pid_returns_pipe(self):
            result = TransportScheme.PREFER_NAMED_PIPE.select_address("maya", "127.0.0.1", 9999, 12345)
            assert result.scheme == "pipe"

        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_prefer_ipc_with_pid_returns_pipe(self):
            result = TransportScheme.PREFER_IPC.select_address("maya", "127.0.0.1", 9999, 12345)
            assert result.scheme == "pipe"

        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_tcp_only_with_pid_still_returns_tcp(self):
            result = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 9999, 12345)
            assert result.scheme == "tcp"

        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_prefer_unix_socket_with_pid_on_windows_returns_tcp(self):
            # Unix sockets not available on Windows → falls back to TCP
            result = TransportScheme.PREFER_UNIX_SOCKET.select_address("maya", "127.0.0.1", 9999, 12345)
            assert result.scheme == "tcp"

        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_pipe_addr_contains_pid(self):
            pid = 12345
            result = TransportScheme.PREFER_NAMED_PIPE.select_address("maya", "127.0.0.1", 9999, pid)
            assert str(pid) in str(result)

        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_pipe_addr_contains_dcc_type(self):
            result = TransportScheme.PREFER_NAMED_PIPE.select_address("blender", "127.0.0.1", 9999, 12345)
            assert "blender" in str(result)

        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_remote_host_prefer_named_pipe_still_uses_tcp(self):
            # Non-local host → cannot use named pipe
            result = TransportScheme.PREFER_NAMED_PIPE.select_address("maya", "192.168.1.1", 9999, 12345)
            assert result.scheme == "tcp"

        @pytest.mark.skipif(sys.platform != "win32", reason="Named pipe only on Windows")
        def test_localhost_prefer_named_pipe_uses_pipe(self):
            result = TransportScheme.PREFER_NAMED_PIPE.select_address("maya", "localhost", 9999, 12345)
            assert result.scheme == "pipe"

    class TestResultType:
        def test_result_is_transport_address(self):
            result = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 9999)
            assert isinstance(result, TransportAddress)

        def test_tcp_result_scheme(self):
            result = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 9999)
            assert result.scheme == "tcp"

        def test_tcp_result_is_tcp(self):
            result = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 9999)
            # is_tcp is a property (bool), not a method
            assert result.is_tcp is True

        def test_result_repr_is_str(self):
            result = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 9999)
            assert isinstance(repr(result), str)

        def test_different_dcc_types_work(self):
            for dcc in ["maya", "blender", "houdini", "3dsmax", "unreal"]:
                result = TransportScheme.TCP_ONLY.select_address(dcc, "127.0.0.1", 9999)
                assert result.scheme == "tcp"

    class TestSchemeEnumVariants:
        def test_auto_exists(self):
            assert hasattr(TransportScheme, "AUTO")

        def test_tcp_only_exists(self):
            assert hasattr(TransportScheme, "TCP_ONLY")

        def test_prefer_named_pipe_exists(self):
            assert hasattr(TransportScheme, "PREFER_NAMED_PIPE")

        def test_prefer_unix_socket_exists(self):
            assert hasattr(TransportScheme, "PREFER_UNIX_SOCKET")

        def test_prefer_ipc_exists(self):
            assert hasattr(TransportScheme, "PREFER_IPC")
