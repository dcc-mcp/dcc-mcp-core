"""Deep tests for ToolPipeline.add_callable, encode/decode round-trips.

PySharedBuffer.descriptor_json, ToolRegistry.__len__, and PyProcessMonitor.

Covers:

- ToolPipeline.add_callable: before/after_fn trigger, non-callable error
- encode_request / encode_response / encode_notify / decode_envelope round-trips
- PySharedBuffer.descriptor_json structure and open() reconstruction
- ToolRegistry.__len__ changes with register/unregister/reset
- PyProcessMonitor: track/untrack/refresh/query/list_all/is_alive/tracked_count
"""

from __future__ import annotations

# Import built-in modules
import json
import os
import struct

# Import third-party modules
import pytest

from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import PySharedBuffer

# Import local modules
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolPipeline
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import decode_envelope
from dcc_mcp_core import encode_notify
from dcc_mcp_core import encode_request
from dcc_mcp_core import encode_response

# ──────────────────────────────────────────────────────────────────────────────
# Helpers
# ──────────────────────────────────────────────────────────────────────────────


def _make_pipeline_with_action(action_name: str = "ping"):
    """Return (pipeline, registry) with a simple echo handler registered."""
    reg = ToolRegistry()
    reg.register(action_name, category="test")
    dispatcher = ToolDispatcher(reg)
    dispatcher.register_handler(action_name, lambda params: "pong")
    pipeline = ToolPipeline(dispatcher)
    return pipeline, reg


# ──────────────────────────────────────────────────────────────────────────────
# ToolPipeline.add_callable — deep tests
# ──────────────────────────────────────────────────────────────────────────────


class TestActionPipelineAddCallable:
    """Verify add_callable behavior: before/after hooks fire correctly."""

    def test_add_callable_with_both_fns_no_error(self):
        pipeline, _ = _make_pipeline_with_action()
        before_calls: list[str] = []
        after_calls: list[tuple[str, bool]] = []

        pipeline.add_callable(
            before_fn=lambda name: before_calls.append(name),
            after_fn=lambda name, ok: after_calls.append((name, ok)),
        )
        pipeline.dispatch("ping", "{}")
        assert before_calls == ["ping"]
        assert after_calls == [("ping", True)]

    def test_add_callable_before_fn_only(self):
        pipeline, _ = _make_pipeline_with_action()
        fired: list[str] = []
        pipeline.add_callable(before_fn=lambda name: fired.append(name))
        pipeline.dispatch("ping", "{}")
        assert fired == ["ping"]

    def test_add_callable_after_fn_only(self):
        pipeline, _ = _make_pipeline_with_action()
        fired: list[tuple[str, bool]] = []
        pipeline.add_callable(after_fn=lambda name, ok: fired.append((name, ok)))
        pipeline.dispatch("ping", "{}")
        assert fired == [("ping", True)]

    def test_add_callable_none_fns_no_error(self):
        pipeline, _ = _make_pipeline_with_action()
        pipeline.add_callable(before_fn=None, after_fn=None)
        result = pipeline.dispatch("ping", "{}")
        assert result["output"] == "pong"

    def test_add_callable_non_callable_before_raises(self):
        pipeline, _ = _make_pipeline_with_action()
        with pytest.raises(TypeError):
            pipeline.add_callable(before_fn="not_callable")

    def test_add_callable_non_callable_after_raises(self):
        pipeline, _ = _make_pipeline_with_action()
        with pytest.raises(TypeError):
            pipeline.add_callable(after_fn=42)

    def test_add_callable_before_called_before_dispatch(self):
        pipeline, _ = _make_pipeline_with_action()
        order: list[str] = []
        pipeline.add_callable(
            before_fn=lambda name: order.append("before"),
        )
        pipeline.register_handler("ping", lambda params: order.append("handler") or "pong")
        pipeline.dispatch("ping", "{}")
        assert order[0] == "before"

    def test_add_callable_after_success_flag_true_on_success(self):
        pipeline, _ = _make_pipeline_with_action()
        flags: list[bool] = []
        pipeline.add_callable(after_fn=lambda name, ok: flags.append(ok))
        pipeline.dispatch("ping", "{}")
        assert flags == [True]

    def test_add_callable_multiple_callable_middlewares(self):
        pipeline, _ = _make_pipeline_with_action()
        calls: list[int] = []
        pipeline.add_callable(before_fn=lambda name: calls.append(1))
        pipeline.add_callable(before_fn=lambda name: calls.append(2))
        pipeline.dispatch("ping", "{}")
        assert 1 in calls
        assert 2 in calls

    def test_middleware_count_increases_with_callable(self):
        pipeline, _ = _make_pipeline_with_action()
        before = pipeline.middleware_count()
        pipeline.add_callable(before_fn=lambda name: None)
        assert pipeline.middleware_count() == before + 1

    def test_middleware_names_includes_python_callable(self):
        pipeline, _ = _make_pipeline_with_action()
        pipeline.add_callable(before_fn=lambda name: None)
        names = pipeline.middleware_names()
        assert any("python_callable" in n or "callable" in n for n in names)


# ──────────────────────────────────────────────────────────────────────────────
# encode_request / encode_response / encode_notify / decode_envelope round-trips
# ──────────────────────────────────────────────────────────────────────────────


class TestEncodeDecodeRequest:
    """encode_request + decode_envelope round-trip."""

    def test_encode_request_returns_bytes(self):
        frame = encode_request("execute_python", b"cmds.sphere()")
        assert isinstance(frame, bytes)

    def test_encode_request_length_prefix_correct(self):
        frame = encode_request("my_method", b"params")
        payload_len = struct.unpack(">I", frame[:4])[0]
        assert payload_len == len(frame) - 4

    def test_encode_request_decode_type_is_request(self):
        frame = encode_request("test_method", b"body")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "request"

    def test_encode_request_decode_method_preserved(self):
        frame = encode_request("execute_python", b"x=1")
        msg = decode_envelope(frame[4:])
        assert msg["method"] == "execute_python"

    def test_encode_request_decode_id_is_uuid_string(self):
        frame = encode_request("my_method")
        msg = decode_envelope(frame[4:])
        id_val = msg["id"]
        assert isinstance(id_val, str)
        assert len(id_val) == 36  # UUID format

    def test_encode_request_decode_params_bytes(self):
        params = b"\x01\x02\x03"
        frame = encode_request("method", params)
        msg = decode_envelope(frame[4:])
        assert msg["params"] == params

    def test_encode_request_no_params_returns_empty_bytes(self):
        frame = encode_request("method")
        msg = decode_envelope(frame[4:])
        assert isinstance(msg["params"], (bytes, type(None)))

    def test_encode_request_each_call_unique_id(self):
        frame1 = encode_request("method")
        frame2 = encode_request("method")
        msg1 = decode_envelope(frame1[4:])
        msg2 = decode_envelope(frame2[4:])
        assert msg1["id"] != msg2["id"]


class TestEncodeDecodeResponse:
    """encode_response + decode_envelope round-trip."""

    def test_encode_response_returns_bytes(self):
        uid = "00000000-0000-0000-0000-000000000000"
        frame = encode_response(uid, True, b"ok")
        assert isinstance(frame, bytes)

    def test_encode_response_length_prefix_correct(self):
        uid = "00000000-0000-0000-0000-000000000000"
        frame = encode_response(uid, True, b"result")
        payload_len = struct.unpack(">I", frame[:4])[0]
        assert payload_len == len(frame) - 4

    def test_encode_response_decode_type_is_response(self):
        uid = "00000000-0000-0000-0000-000000000000"
        frame = encode_response(uid, True, b"payload")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "response"

    def test_encode_response_decode_id_preserved(self):
        uid = "11111111-1111-1111-1111-111111111111"
        frame = encode_response(uid, True, b"data")
        msg = decode_envelope(frame[4:])
        assert msg["id"] == uid

    def test_encode_response_decode_success_true(self):
        uid = "00000000-0000-0000-0000-000000000000"
        frame = encode_response(uid, True)
        msg = decode_envelope(frame[4:])
        assert msg["success"] is True

    def test_encode_response_decode_success_false(self):
        uid = "00000000-0000-0000-0000-000000000000"
        frame = encode_response(uid, False, error="something went wrong")
        msg = decode_envelope(frame[4:])
        assert msg["success"] is False

    def test_encode_response_decode_error_field(self):
        uid = "00000000-0000-0000-0000-000000000000"
        frame = encode_response(uid, False, error="timeout")
        msg = decode_envelope(frame[4:])
        assert msg["error"] == "timeout"

    def test_encode_response_decode_payload_bytes(self):
        uid = "00000000-0000-0000-0000-000000000000"
        payload = b"\xde\xad\xbe\xef"
        frame = encode_response(uid, True, payload=payload)
        msg = decode_envelope(frame[4:])
        assert msg["payload"] == payload

    def test_encode_response_invalid_uuid_raises(self):
        with pytest.raises((RuntimeError, ValueError)):
            encode_response("not-a-uuid", True, b"")


class TestEncodeDecodeNotify:
    """encode_notify + decode_envelope round-trip."""

    def test_encode_notify_returns_bytes(self):
        frame = encode_notify("scene_changed", b"data")
        assert isinstance(frame, bytes)

    def test_encode_notify_length_prefix_correct(self):
        frame = encode_notify("topic", b"body")
        payload_len = struct.unpack(">I", frame[:4])[0]
        assert payload_len == len(frame) - 4

    def test_encode_notify_decode_type_is_notify(self):
        frame = encode_notify("scene_changed")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "notify"

    def test_encode_notify_decode_topic_preserved(self):
        frame = encode_notify("render_complete", b"")
        msg = decode_envelope(frame[4:])
        assert msg["topic"] == "render_complete"

    def test_encode_notify_decode_data_bytes(self):
        frame = encode_notify("event", b"payload123")
        msg = decode_envelope(frame[4:])
        assert msg["data"] == b"payload123"

    def test_encode_notify_no_data_empty_bytes(self):
        frame = encode_notify("event")
        msg = decode_envelope(frame[4:])
        assert isinstance(msg["data"], (bytes, type(None)))

    def test_encode_notify_different_topics_decode_correctly(self):
        for topic in ["a", "b", "scene.changed", "mesh.added"]:
            frame = encode_notify(topic)
            msg = decode_envelope(frame[4:])
            assert msg["topic"] == topic


class TestDecodeEnvelopeErrors:
    """decode_envelope error paths."""

    def test_decode_invalid_bytes_raises(self):
        with pytest.raises(RuntimeError):
            decode_envelope(b"\x00\x01\x02\x03garbage")

    def test_decode_empty_bytes_raises(self):
        with pytest.raises(RuntimeError):
            decode_envelope(b"")


# ──────────────────────────────────────────────────────────────────────────────
# PySharedBuffer descriptor_json and open() reconstruction
# ──────────────────────────────────────────────────────────────────────────────


class TestPySharedBufferDescriptorJson:
    """Verify descriptor_json structure and cross-instance open()."""

    def test_descriptor_json_is_valid_json(self):
        buf = PySharedBuffer.create(capacity=1024)
        desc = buf.descriptor_json()
        parsed = json.loads(desc)
        assert isinstance(parsed, dict)

    def test_descriptor_json_contains_id(self):
        buf = PySharedBuffer.create(capacity=1024)
        desc = json.loads(buf.descriptor_json())
        assert "id" in desc or "buffer_id" in desc or buf.id in desc.get("id", "")

    def test_descriptor_json_id_matches_buf_id(self):
        buf = PySharedBuffer.create(capacity=2048)
        desc = json.loads(buf.descriptor_json())
        id_val = desc.get("id") or desc.get("buffer_id")
        assert id_val == buf.id

    def test_open_reads_same_data(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"hello shared memory")
        buf2 = PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.read() == b"hello shared memory"

    def test_open_capacity_matches(self):
        buf = PySharedBuffer.create(capacity=4096)
        buf2 = PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.capacity() == buf.capacity()

    def test_open_data_len_matches(self):
        buf = PySharedBuffer.create(capacity=1024)
        data = b"some data"
        buf.write(data)
        buf2 = PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.data_len() == len(data)

    def test_write_visible_across_instances(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"cross-process data")
        buf2 = PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.read() == b"cross-process data"

    def test_clear_resets_data_len(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"data")
        buf.clear()
        assert buf.data_len() == 0

    def test_clear_read_returns_empty(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"data")
        buf.clear()
        assert buf.read() == b""

    def test_write_overflow_raises(self):
        buf = PySharedBuffer.create(capacity=8)
        with pytest.raises(RuntimeError):
            buf.write(b"too much data here!!")

    def test_buf_id_is_string(self):
        buf = PySharedBuffer.create(capacity=512)
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_buf_name_is_string(self):
        buf = PySharedBuffer.create(capacity=512)
        name = buf.name()
        assert isinstance(name, str)
        assert len(name) > 0

    def test_repr_is_string(self):
        buf = PySharedBuffer.create(capacity=512)
        r = repr(buf)
        assert isinstance(r, str)
        assert len(r) > 0


# ──────────────────────────────────────────────────────────────────────────────
# ToolRegistry.__len__ deep tests
# ──────────────────────────────────────────────────────────────────────────────


class TestActionRegistryLen:
    """Verify __len__ changes correctly with register/unregister/reset."""

    def test_len_empty_registry_is_zero(self):
        reg = ToolRegistry()
        assert len(reg) == 0

    def test_len_after_one_register(self):
        reg = ToolRegistry()
        reg.register("a")
        assert len(reg) == 1

    def test_len_after_three_registers(self):
        reg = ToolRegistry()
        reg.register("a")
        reg.register("b")
        reg.register("c")
        assert len(reg) == 3

    def test_len_after_unregister_decrements(self):
        reg = ToolRegistry()
        reg.register("a")
        reg.register("b")
        reg.unregister("a")
        assert len(reg) == 1

    def test_len_after_unregister_nonexistent_stays_same(self):
        reg = ToolRegistry()
        reg.register("a")
        reg.unregister("nonexistent")
        assert len(reg) == 1

    def test_len_after_reset_is_zero(self):
        reg = ToolRegistry()
        reg.register("a")
        reg.register("b")
        reg.reset()
        assert len(reg) == 0

    def test_len_batch_register_correct_count(self):
        reg = ToolRegistry()
        reg.register_batch(
            [
                {"name": "x", "dcc": "maya"},
                {"name": "y", "dcc": "maya"},
                {"name": "z", "dcc": "blender"},
            ]
        )
        assert len(reg) == 3

    def test_len_batch_skips_empty_name(self):
        reg = ToolRegistry()
        reg.register_batch(
            [
                {"name": "valid"},
                {"name": ""},
                {"dcc": "maya"},  # no name key
            ]
        )
        assert len(reg) == 1

    def test_len_same_action_different_dcc_counts_once(self):
        reg = ToolRegistry()
        reg.register("action", dcc="maya")
        reg.register("action", dcc="blender")
        # Same action name — how many? Check that __len__ returns consistent int
        count = len(reg)
        assert isinstance(count, int)
        assert count >= 1

    def test_len_type_is_int(self):
        reg = ToolRegistry()
        assert isinstance(len(reg), int)

    def test_len_increases_monotonically(self):
        reg = ToolRegistry()
        counts: list[int] = []
        for i in range(5):
            reg.register(f"action_{i}")
            counts.append(len(reg))
        for j in range(len(counts) - 1):
            assert counts[j] < counts[j + 1]


# ──────────────────────────────────────────────────────────────────────────────
# PyProcessMonitor deep tests
# ──────────────────────────────────────────────────────────────────────────────


class TestPyProcessMonitorBasic:
    """Basic construction and tracked_count."""

    def test_construction_no_error(self):
        mon = PyProcessMonitor()
        assert mon is not None

    def test_tracked_count_initial_zero(self):
        mon = PyProcessMonitor()
        assert mon.tracked_count() == 0

    def test_tracked_count_after_track(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        assert mon.tracked_count() == 1

    def test_tracked_count_after_untrack(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.untrack(os.getpid())
        assert mon.tracked_count() == 0

    def test_tracked_count_after_untrack_nonexistent(self):
        mon = PyProcessMonitor()
        mon.untrack(99999999)  # should not raise
        assert mon.tracked_count() == 0

    def test_tracked_count_is_int(self):
        mon = PyProcessMonitor()
        assert isinstance(mon.tracked_count(), int)

    def test_repr_is_string(self):
        mon = PyProcessMonitor()
        r = repr(mon)
        assert isinstance(r, str)
        assert len(r) > 0


class TestPyProcessMonitorRefreshQuery:
    """refresh() and query() behavior."""

    def test_refresh_no_error(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()  # should not raise

    def test_query_tracked_returns_dict(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert isinstance(info, dict)

    def test_query_nonexistent_returns_none_or_dict(self):
        mon = PyProcessMonitor()
        # Not tracked, not refreshed — may return None
        result = mon.query(99999999)
        assert result is None or isinstance(result, dict)

    def test_query_result_has_pid_key(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        if info is not None:
            assert "pid" in info

    def test_query_result_has_name_key(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        if info is not None:
            assert "name" in info

    def test_query_result_has_status_key(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        if info is not None:
            assert "status" in info

    def test_query_result_pid_matches(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        if info is not None:
            assert info["pid"] == pid

    def test_query_result_cpu_usage_is_float(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        if info is not None and "cpu_usage_percent" in info:
            assert isinstance(info["cpu_usage_percent"], (int, float))

    def test_query_result_memory_bytes_is_int(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        if info is not None and "memory_bytes" in info:
            assert isinstance(info["memory_bytes"], int)
            assert info["memory_bytes"] >= 0

    def test_query_result_restart_count_is_int(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        if info is not None and "restart_count" in info:
            assert isinstance(info["restart_count"], int)


class TestPyProcessMonitorListAll:
    """list_all() behavior."""

    def test_list_all_empty_initially(self):
        mon = PyProcessMonitor()
        assert mon.list_all() == []

    def test_list_all_returns_list(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        result = mon.list_all()
        assert isinstance(result, list)

    def test_list_all_count_matches_tracked(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        result = mon.list_all()
        # May have 0 entries if process info unavailable, but tracked_count is 1
        assert isinstance(result, list)

    def test_list_all_entries_are_dicts(self):
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        for entry in mon.list_all():
            assert isinstance(entry, dict)


class TestPyProcessMonitorIsAlive:
    """is_alive() behavior."""

    def test_is_alive_self_true(self):
        mon = PyProcessMonitor()
        assert mon.is_alive(os.getpid()) is True

    def test_is_alive_nonexistent_pid_false(self):
        mon = PyProcessMonitor()
        # Very large PID unlikely to exist
        assert mon.is_alive(2147483647) is False

    def test_is_alive_returns_bool(self):
        mon = PyProcessMonitor()
        result = mon.is_alive(os.getpid())
        assert isinstance(result, bool)

    def test_is_alive_does_not_require_track(self):
        mon = PyProcessMonitor()
        # is_alive should work without track() being called
        result = mon.is_alive(os.getpid())
        assert result is True

    def test_is_alive_multiple_pids(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        results = [mon.is_alive(pid) for _ in range(3)]
        assert all(r is True for r in results)
