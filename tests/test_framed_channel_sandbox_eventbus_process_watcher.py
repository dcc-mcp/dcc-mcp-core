"""Behavioral tests for FramedChannel, SandboxContext/Policy, AuditLog, InputValidator, EventBus, and PyProcessWatcher.

Covers happy path, edge cases, and error paths for each class.
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import json
import os
import uuid

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import AuditLog
from dcc_mcp_core import EventBus
from dcc_mcp_core import InputValidator
from dcc_mcp_core import IpcListener
from dcc_mcp_core import PyProcessWatcher
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import connect_ipc

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _bind_and_connect():
    """Bind a TCP listener, convert to handle, connect a client.

    Uses ListenerHandle so no accept() thread is needed.
    Returns (handle, client_channel).
    """
    from dcc_mcp_core import ListenerHandle

    addr = TransportAddress.tcp("127.0.0.1", 0)
    listener = IpcListener.bind(addr)
    local = listener.local_address()
    handle = listener.into_handle()
    client = connect_ipc(local)
    return handle, client


# ===========================================================================
# FramedChannel tests
# ===========================================================================


class TestFramedChannelLifecycle:
    """Tests for FramedChannel basic lifecycle (single-endpoint, no accept needed)."""

    def test_is_running_after_connect(self):
        _handle, client = _bind_and_connect()
        try:
            assert client.is_running is True
        finally:
            client.shutdown()

    def test_is_running_after_shutdown(self):
        _handle, client = _bind_and_connect()
        client.shutdown()
        assert client.is_running is False

    def test_shutdown_is_idempotent(self):
        _handle, client = _bind_and_connect()
        client.shutdown()
        client.shutdown()  # second call must not raise
        assert client.is_running is False

    def test_bool_true_when_running(self):
        _handle, client = _bind_and_connect()
        try:
            assert bool(client) is True
        finally:
            client.shutdown()

    def test_bool_false_after_shutdown(self):
        _handle, client = _bind_and_connect()
        client.shutdown()
        assert bool(client) is False

    def test_repr_is_string(self):
        _handle, client = _bind_and_connect()
        try:
            r = repr(client)
            assert isinstance(r, str)
        finally:
            client.shutdown()


class TestFramedChannelSendOnly:
    """Tests for FramedChannel send operations (single-endpoint, no server recv needed)."""

    def test_send_request_returns_uuid_string(self):
        _handle, client = _bind_and_connect()
        try:
            req_id = client.send_request("test_method")
            assert isinstance(req_id, str)
            assert len(req_id) == 36  # UUID: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
        finally:
            client.shutdown()

    def test_send_request_with_params_bytes(self):
        _handle, client = _bind_and_connect()
        try:
            req_id = client.send_request("execute_python", b'print("hello")')
            assert isinstance(req_id, str)
            assert len(req_id) == 36
        finally:
            client.shutdown()

    def test_send_request_no_params(self):
        _handle, client = _bind_and_connect()
        try:
            req_id = client.send_request("no_params_method")
            assert isinstance(req_id, str)
        finally:
            client.shutdown()

    def test_send_request_different_ids(self):
        _handle, client = _bind_and_connect()
        try:
            id1 = client.send_request("method_a")
            id2 = client.send_request("method_b")
            assert id1 != id2
        finally:
            client.shutdown()

    def test_send_notify_does_not_raise(self):
        _handle, client = _bind_and_connect()
        try:
            client.send_notify("scene_changed", b"data")  # must not raise
        finally:
            client.shutdown()

    def test_send_notify_no_payload(self):
        _handle, client = _bind_and_connect()
        try:
            client.send_notify("event_no_data")  # must not raise
        finally:
            client.shutdown()

    def test_try_recv_returns_none_when_empty(self):
        _handle, client = _bind_and_connect()
        try:
            result = client.try_recv()
            assert result is None
        finally:
            client.shutdown()

    def test_send_response_with_valid_uuid(self):
        _handle, client = _bind_and_connect()
        try:
            valid_uuid = str(uuid.uuid4())
            client.send_response(valid_uuid, True, b"data")  # must not raise
        finally:
            client.shutdown()

    def test_send_response_failure_flag(self):
        _handle, client = _bind_and_connect()
        try:
            valid_uuid = str(uuid.uuid4())
            client.send_response(valid_uuid, False, error="err msg")  # must not raise
        finally:
            client.shutdown()

    def test_send_response_invalid_uuid_raises(self):
        _handle, client = _bind_and_connect()
        try:
            with pytest.raises((ValueError, RuntimeError)):
                client.send_response("not-a-uuid", True, b"data")
        finally:
            client.shutdown()

    def test_recv_timeout_returns_none(self):
        _handle, client = _bind_and_connect()
        try:
            result = client.recv(timeout_ms=50)
            # No message sent → should return None
            assert result is None
        finally:
            client.shutdown()


# ===========================================================================
# SandboxPolicy tests
# ===========================================================================


class TestSandboxPolicy:
    """Tests for SandboxPolicy configuration."""

    def test_default_not_read_only(self):
        p = SandboxPolicy()
        assert p.is_read_only is False

    def test_set_read_only(self):
        p = SandboxPolicy()
        p.set_read_only(True)
        assert p.is_read_only is True

    def test_set_read_only_false(self):
        p = SandboxPolicy()
        p.set_read_only(True)
        p.set_read_only(False)
        assert p.is_read_only is False

    def test_allow_actions_list(self):
        p = SandboxPolicy()
        p.allow_actions(["create_sphere", "delete_mesh"])  # must not raise

    def test_deny_actions_list(self):
        p = SandboxPolicy()
        p.deny_actions(["exec_script", "eval_code"])  # must not raise

    def test_allow_paths_list(self):
        p = SandboxPolicy()
        p.allow_paths(["/project", "/assets"])  # must not raise

    def test_set_timeout_ms(self):
        p = SandboxPolicy()
        p.set_timeout_ms(5000)  # must not raise

    def test_set_max_actions(self):
        p = SandboxPolicy()
        p.set_max_actions(50)  # must not raise

    def test_empty_allow_list(self):
        p = SandboxPolicy()
        p.allow_actions([])  # empty list OK

    def test_empty_deny_list(self):
        p = SandboxPolicy()
        p.deny_actions([])


# ===========================================================================
# SandboxContext tests
# ===========================================================================


class TestSandboxContext:
    """Tests for SandboxContext action execution and audit."""

    def _make_ctx(self, allow=None, deny=None, read_only=False):
        p = SandboxPolicy()
        if allow:
            p.allow_actions(allow)
        if deny:
            p.deny_actions(deny)
        if read_only:
            p.set_read_only(True)
        return SandboxContext(p)

    def test_action_count_starts_at_zero(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        assert ctx.action_count == 0

    def test_is_allowed_for_allowed_action(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        assert ctx.is_allowed("create_sphere") is True

    def test_is_not_allowed_for_not_in_whitelist(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        assert ctx.is_allowed("exec_script") is False

    def test_is_not_allowed_for_denied_action(self):
        p = SandboxPolicy()
        p.allow_actions(["create_sphere", "exec_script"])
        p.deny_actions(["exec_script"])
        ctx = SandboxContext(p)
        assert ctx.is_allowed("exec_script") is False

    def test_is_not_allowed_unknown_when_whitelist_set(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        assert ctx.is_allowed("unknown_action") is False

    def test_is_path_allowed_returns_bool(self):
        ctx = self._make_ctx(allow=["x"])
        result = ctx.is_path_allowed("/project")
        assert isinstance(result, bool)

    def test_set_actor_does_not_raise(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        ctx.set_actor("agent_1")  # must not raise

    def test_audit_log_is_audit_log_instance(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        log = ctx.audit_log
        assert isinstance(log, AuditLog)

    def test_execute_allowed_action_returns_json(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        result = ctx.execute_json("create_sphere", json.dumps({"radius": 1.0}))
        assert isinstance(result, str)

    def test_execute_increments_action_count(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        ctx.execute_json("create_sphere", json.dumps({}))
        assert ctx.action_count == 1

    def test_execute_multiple_increments_count(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        ctx.execute_json("create_sphere", json.dumps({}))
        ctx.execute_json("create_sphere", json.dumps({}))
        assert ctx.action_count == 2

    def test_execute_denied_raises_runtime_error(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        with pytest.raises(RuntimeError, match="not allowed"):
            ctx.execute_json("exec_script", json.dumps({}))

    def test_execute_denied_still_records_in_audit_log(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        ctx.set_actor("tester")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("exec_script", json.dumps({}))
        log = ctx.audit_log
        denials = log.denials()
        assert len(denials) >= 1
        assert denials[0].action == "exec_script"

    def test_execute_success_recorded_in_audit_log(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        ctx.set_actor("agent_x")
        ctx.execute_json("create_sphere", json.dumps({"radius": 2.0}))
        log = ctx.audit_log
        successes = log.successes()
        assert len(successes) == 1
        assert successes[0].action == "create_sphere"
        assert successes[0].actor == "agent_x"

    def test_audit_log_len_matches_total_entries(self):
        ctx = self._make_ctx(allow=["create_sphere"])
        ctx.execute_json("create_sphere", json.dumps({}))
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("denied_action", json.dumps({}))
        log = ctx.audit_log
        assert len(log) == 2


# ===========================================================================
# AuditLog / AuditEntry tests
# ===========================================================================


class TestAuditLogEntries:
    """Tests for AuditLog entries, filtering, and export."""

    def _make_ctx_with_log(self):
        p = SandboxPolicy()
        p.allow_actions(["act_a", "act_b"])
        ctx = SandboxContext(p)
        ctx.set_actor("tester")
        ctx.execute_json("act_a", json.dumps({"x": 1}))
        ctx.execute_json("act_b", json.dumps({"y": 2}))
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("denied_act", json.dumps({}))
        return ctx.audit_log

    def test_entries_returns_list(self):
        log = self._make_ctx_with_log()
        assert isinstance(log.entries(), list)

    def test_entries_count(self):
        log = self._make_ctx_with_log()
        assert len(log.entries()) == 3

    def test_successes_returns_only_successful_entries(self):
        log = self._make_ctx_with_log()
        s = log.successes()
        assert len(s) == 2
        actions = {e.action for e in s}
        assert actions == {"act_a", "act_b"}

    def test_denials_returns_only_denied_entries(self):
        log = self._make_ctx_with_log()
        d = log.denials()
        assert len(d) == 1
        assert d[0].action == "denied_act"

    def test_entries_for_action_filters(self):
        log = self._make_ctx_with_log()
        act_a_entries = log.entries_for_action("act_a")
        assert len(act_a_entries) == 1
        assert act_a_entries[0].action == "act_a"

    def test_entries_for_action_empty_when_not_found(self):
        log = self._make_ctx_with_log()
        results = log.entries_for_action("nonexistent")
        assert results == []

    def test_to_json_returns_string(self):
        log = self._make_ctx_with_log()
        j = log.to_json()
        assert isinstance(j, str)

    def test_to_json_is_valid_json(self):
        log = self._make_ctx_with_log()
        parsed = json.loads(log.to_json())
        assert isinstance(parsed, list)
        assert len(parsed) == 3

    def test_len_matches_entries_count(self):
        log = self._make_ctx_with_log()
        assert len(log) == len(log.entries())

    def test_entry_action_field(self):
        log = self._make_ctx_with_log()
        entry = log.entries()[0]
        assert isinstance(entry.action, str)
        assert len(entry.action) > 0

    def test_entry_actor_field(self):
        log = self._make_ctx_with_log()
        entry = log.entries()[0]
        assert entry.actor == "tester"

    def test_entry_outcome_success_string(self):
        log = self._make_ctx_with_log()
        success_entry = log.successes()[0]
        assert success_entry.outcome == "success"

    def test_entry_outcome_denied_string(self):
        log = self._make_ctx_with_log()
        denied_entry = log.denials()[0]
        assert denied_entry.outcome == "denied"

    def test_entry_timestamp_ms_is_positive_int(self):
        log = self._make_ctx_with_log()
        entry = log.entries()[0]
        assert isinstance(entry.timestamp_ms, int)
        assert entry.timestamp_ms > 0

    def test_entry_duration_ms_is_non_negative_int(self):
        log = self._make_ctx_with_log()
        entry = log.entries()[0]
        assert isinstance(entry.duration_ms, int)
        assert entry.duration_ms >= 0

    def test_entry_params_json_is_string(self):
        log = self._make_ctx_with_log()
        entry = log.entries()[0]
        assert isinstance(entry.params_json, str)

    def test_entry_outcome_detail_none_for_success(self):
        log = self._make_ctx_with_log()
        success_entry = log.successes()[0]
        assert success_entry.outcome_detail is None

    def test_entry_outcome_detail_set_for_denied(self):
        log = self._make_ctx_with_log()
        denied_entry = log.denials()[0]
        assert denied_entry.outcome_detail is not None
        assert isinstance(denied_entry.outcome_detail, str)


# ===========================================================================
# InputValidator tests
# ===========================================================================


class TestInputValidatorHappyPath:
    """Happy-path tests for InputValidator."""

    def test_valid_input_returns_true_none(self):
        v = InputValidator()
        v.require_string("name", 100, 1)
        v.require_number("radius", 0.0, 1000.0)  # min=0, max=1000
        ok, err = v.validate(json.dumps({"name": "sphere1", "radius": 2.5}))
        assert ok is True
        assert err is None

    def test_string_at_min_length(self):
        v = InputValidator()
        v.require_string("label", 100, 1)
        ok, _err = v.validate(json.dumps({"label": "x"}))
        assert ok is True

    def test_string_at_max_length(self):
        v = InputValidator()
        v.require_string("label", 5, 1)
        ok, _err = v.validate(json.dumps({"label": "hello"}))
        assert ok is True

    def test_number_at_min(self):
        v = InputValidator()
        v.require_number("count", 0.0, 100.0)  # min=0, max=100
        ok, _err = v.validate(json.dumps({"count": 0.0}))
        assert ok is True

    def test_number_at_max(self):
        v = InputValidator()
        v.require_number("count", 0.0, 100.0)  # min=0, max=100
        ok, _err = v.validate(json.dumps({"count": 100.0}))
        assert ok is True

    def test_no_forbidden_substring(self):
        v = InputValidator()
        v.require_string("path", 200, 1)
        v.forbid_substrings("path", ["../", "..\\"])
        ok, _err = v.validate(json.dumps({"path": "/project/scene.usd"}))
        assert ok is True

    def test_multiple_fields_all_valid(self):
        v = InputValidator()
        v.require_string("name", 50, 1)
        v.require_number("x", -100.0, 100.0)  # min=-100, max=100
        v.require_number("y", -100.0, 100.0)
        ok, _err = v.validate(json.dumps({"name": "pt", "x": 1.0, "y": -1.0}))
        assert ok is True


class TestInputValidatorErrorPath:
    """Error-path tests for InputValidator."""

    def test_forbidden_substring_returns_false(self):
        v = InputValidator()
        v.require_string("path", 200, 1)
        v.forbid_substrings("path", ["../"])
        ok, err = v.validate(json.dumps({"path": "../etc/passwd"}))
        assert ok is False
        assert err is not None

    def test_missing_required_string_returns_false(self):
        v = InputValidator()
        v.require_string("name", 100, 1)
        ok, err = v.validate(json.dumps({"radius": 1.0}))
        assert ok is False
        assert err is not None

    def test_missing_required_number_returns_false(self):
        v = InputValidator()
        v.require_number("radius", 0.0, 1000.0)
        ok, err = v.validate(json.dumps({"name": "x"}))
        assert ok is False
        assert err is not None

    def test_string_too_short_returns_false(self):
        v = InputValidator()
        v.require_string("name", 100, 3)
        ok, _err = v.validate(json.dumps({"name": "ab"}))
        assert ok is False

    def test_string_too_long_returns_false(self):
        v = InputValidator()
        v.require_string("name", 5, 1)
        ok, _err = v.validate(json.dumps({"name": "toolongname"}))
        assert ok is False

    def test_number_below_min_returns_false(self):
        v = InputValidator()
        v.require_number("val", 10.0, 100.0)  # min=10, max=100
        ok, _err = v.validate(json.dumps({"val": 5.0}))
        assert ok is False

    def test_number_above_max_returns_false(self):
        v = InputValidator()
        v.require_number("val", 0.0, 100.0)  # min=0, max=100
        ok, _err = v.validate(json.dumps({"val": 200.0}))
        assert ok is False

    def test_wrong_type_for_string_returns_false(self):
        v = InputValidator()
        v.require_string("name", 100, 1)
        ok, _err = v.validate(json.dumps({"name": 42}))
        assert ok is False

    def test_wrong_type_for_number_returns_false(self):
        v = InputValidator()
        v.require_number("radius", 0.0, 100.0)
        ok, _err = v.validate(json.dumps({"radius": "not_a_number"}))
        assert ok is False

    def test_invalid_json_raises_runtime_error(self):
        v = InputValidator()
        with pytest.raises(RuntimeError):
            v.validate("not valid json")

    def test_multiple_forbidden_substrings(self):
        v = InputValidator()
        v.require_string("cmd", 200, 1)
        v.forbid_substrings("cmd", ["rm -rf", "sudo", ";"])
        ok1, _ = v.validate(json.dumps({"cmd": "rm -rf /"}))
        ok2, _ = v.validate(json.dumps({"cmd": "echo hello"}))
        assert ok1 is False
        assert ok2 is True

    def test_empty_json_object_missing_fields(self):
        v = InputValidator()
        v.require_string("name", 100, 1)
        ok, _err = v.validate(json.dumps({}))
        assert ok is False


# ===========================================================================
# EventBus tests
# ===========================================================================


class TestEventBusSubscribePublish:
    """Tests for EventBus subscribe/publish/unsubscribe."""

    def test_subscribe_returns_integer_id(self):
        bus = EventBus()
        sub_id = bus.subscribe("evt", lambda **kw: None)
        assert isinstance(sub_id, int)

    def test_subscribe_different_ids_per_subscription(self):
        bus = EventBus()
        id1 = bus.subscribe("evt", lambda **kw: None)
        id2 = bus.subscribe("evt", lambda **kw: None)
        assert id1 != id2

    def test_publish_calls_subscriber(self):
        bus = EventBus()
        received = []
        bus.subscribe("scene_changed", lambda **kw: received.append(kw))
        bus.publish("scene_changed", frame=42)
        assert received == [{"frame": 42}]

    def test_publish_passes_kwargs_to_callback(self):
        bus = EventBus()
        received = []
        bus.subscribe("evt", lambda **kw: received.append(kw))
        bus.publish("evt", a=1, b="hello", c=True)
        assert received == [{"a": 1, "b": "hello", "c": True}]

    def test_publish_no_subscriber_does_not_raise(self):
        bus = EventBus()
        bus.publish("unknown_event", data="x")  # must not raise

    def test_multiple_subscribers_all_called(self):
        bus = EventBus()
        log1, log2 = [], []
        bus.subscribe("evt", lambda **kw: log1.append(kw))
        bus.subscribe("evt", lambda **kw: log2.append(kw))
        bus.publish("evt", msg="hello")
        assert log1 == [{"msg": "hello"}]
        assert log2 == [{"msg": "hello"}]

    def test_unsubscribe_removes_subscriber(self):
        bus = EventBus()
        received = []
        sub_id = bus.subscribe("evt", lambda **kw: received.append(kw))
        bus.unsubscribe("evt", sub_id)
        bus.publish("evt", data="x")
        assert received == []

    def test_unsubscribe_one_of_two_subscribers(self):
        bus = EventBus()
        log1, log2 = [], []
        id1 = bus.subscribe("evt", lambda **kw: log1.append(kw))
        bus.subscribe("evt", lambda **kw: log2.append(kw))
        bus.unsubscribe("evt", id1)
        bus.publish("evt", x=99)
        assert log1 == []
        assert log2 == [{"x": 99}]

    def test_publish_different_events_independent(self):
        bus = EventBus()
        log_a, log_b = [], []
        bus.subscribe("evt_a", lambda **kw: log_a.append(kw))
        bus.subscribe("evt_b", lambda **kw: log_b.append(kw))
        bus.publish("evt_a", val=1)
        bus.publish("evt_b", val=2)
        assert log_a == [{"val": 1}]
        assert log_b == [{"val": 2}]

    def test_publish_twice_calls_subscriber_twice(self):
        bus = EventBus()
        count = []
        bus.subscribe("evt", lambda **kw: count.append(1))
        bus.publish("evt")
        bus.publish("evt")
        assert len(count) == 2

    def test_subscribe_to_multiple_events(self):
        bus = EventBus()
        received = []
        bus.subscribe("evt_a", lambda **kw: received.append(("a", kw)))
        bus.subscribe("evt_b", lambda **kw: received.append(("b", kw)))
        bus.publish("evt_a", x=1)
        bus.publish("evt_b", y=2)
        assert ("a", {"x": 1}) in received
        assert ("b", {"y": 2}) in received

    def test_unsubscribe_nonexistent_does_not_raise(self):
        bus = EventBus()
        bus.unsubscribe("evt", 9999)  # must not raise

    def test_publish_no_kwargs(self):
        bus = EventBus()
        received = []
        bus.subscribe("evt", lambda **kw: received.append(kw))
        bus.publish("evt")
        assert received == [{}]


# ===========================================================================
# PyProcessWatcher tests
# ===========================================================================


class TestPyProcessWatcherLifecycle:
    """Tests for PyProcessWatcher lifecycle methods."""

    def test_is_running_method_returns_bool(self):
        w = PyProcessWatcher()
        result = w.is_running()
        assert isinstance(result, bool)

    def test_not_running_before_start(self):
        w = PyProcessWatcher()
        assert w.is_running() is False

    def test_running_after_start(self):
        w = PyProcessWatcher()
        w.start()
        try:
            assert w.is_running() is True
        finally:
            w.stop()

    def test_not_running_after_stop(self):
        w = PyProcessWatcher()
        w.start()
        w.stop()
        assert w.is_running() is False

    def test_stop_without_start_does_not_raise(self):
        w = PyProcessWatcher()
        w.stop()  # must not raise

    def test_start_twice_does_not_raise(self):
        w = PyProcessWatcher()
        w.start()
        try:
            w.start()  # second start must not raise
        finally:
            w.stop()


class TestPyProcessWatcherTracking:
    """Tests for PyProcessWatcher track/untrack/watch."""

    def test_watch_count_method_returns_int(self):
        w = PyProcessWatcher()
        assert isinstance(w.watch_count(), int)

    def test_tracked_count_method_returns_int(self):
        w = PyProcessWatcher()
        assert isinstance(w.tracked_count(), int)

    def test_tracked_count_starts_at_zero(self):
        w = PyProcessWatcher()
        assert w.tracked_count() == 0

    def test_watch_count_starts_at_zero(self):
        w = PyProcessWatcher()
        assert w.watch_count() == 0

    def test_is_watched_before_track_returns_false(self):
        w = PyProcessWatcher()
        assert w.is_watched(os.getpid()) is False

    def test_track_own_pid(self):
        w = PyProcessWatcher()
        w.track(os.getpid(), "self")
        assert w.is_watched(os.getpid()) is True
        w.untrack(os.getpid())

    def test_tracked_count_after_track(self):
        w = PyProcessWatcher()
        w.track(os.getpid(), "self")
        assert w.tracked_count() >= 1
        w.untrack(os.getpid())

    def test_add_watch_is_alias_for_track(self):
        w = PyProcessWatcher()
        w.add_watch(os.getpid(), "self_alias")
        assert w.is_watched(os.getpid()) is True
        w.remove_watch(os.getpid())

    def test_untrack_removes_pid(self):
        w = PyProcessWatcher()
        w.track(os.getpid(), "proc")
        w.untrack(os.getpid())
        assert w.is_watched(os.getpid()) is False

    def test_remove_watch_removes_pid(self):
        w = PyProcessWatcher()
        w.add_watch(os.getpid(), "proc")
        w.remove_watch(os.getpid())
        assert w.is_watched(os.getpid()) is False

    def test_untrack_idempotent(self):
        w = PyProcessWatcher()
        w.track(os.getpid(), "proc")
        w.untrack(os.getpid())
        w.untrack(os.getpid())  # second call must not raise

    def test_remove_watch_idempotent(self):
        w = PyProcessWatcher()
        w.add_watch(os.getpid(), "p")
        w.remove_watch(os.getpid())
        w.remove_watch(os.getpid())  # second call must not raise


class TestPyProcessWatcherPollEvents:
    """Tests for PyProcessWatcher.poll_events."""

    def test_poll_events_returns_list(self):
        w = PyProcessWatcher()
        w.start()
        try:
            evts = w.poll_events()
            assert isinstance(evts, list)
        finally:
            w.stop()

    def test_poll_events_empty_initially(self):
        w = PyProcessWatcher()
        w.start()
        try:
            evts = w.poll_events()
            # No processes tracked yet → empty list
            assert evts == []
        finally:
            w.stop()

    def test_poll_events_without_start_returns_list(self):
        w = PyProcessWatcher()
        evts = w.poll_events()
        assert isinstance(evts, list)
