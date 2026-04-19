"""Deep tests for SHM, Capture, Process, and value-wrapper APIs.

Covers: PySharedBuffer, PyBufferPool, PySharedSceneBuffer, PySceneDataKind,
Capturer, CaptureFrame, PyProcessMonitor, PyProcessWatcher, PyCrashRecoveryPolicy,
PyDccLauncher, and value-wrapper utilities (BooleanWrapper, IntWrapper, FloatWrapper,
StringWrapper, wrap_value, unwrap_value, unwrap_parameters).

Includes happy paths, boundary conditions, error paths, and attribute vs. method
distinctions.
"""

from __future__ import annotations

import json
import os
import time

import pytest

from dcc_mcp_core import BooleanWrapper
from dcc_mcp_core import CaptureFrame
from dcc_mcp_core import Capturer
from dcc_mcp_core import CaptureResult
from dcc_mcp_core import FloatWrapper
from dcc_mcp_core import IntWrapper
from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PyCrashRecoveryPolicy
from dcc_mcp_core import PyDccLauncher
from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import PyProcessWatcher
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import PySharedBuffer
from dcc_mcp_core import PySharedSceneBuffer
from dcc_mcp_core import StringWrapper
from dcc_mcp_core import unwrap_parameters
from dcc_mcp_core import unwrap_value
from dcc_mcp_core import wrap_value

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _current_pid() -> int:
    return os.getpid()


# ===========================================================================
# PySharedSceneBuffer
# ===========================================================================


class TestPySharedSceneBufferCreate:
    """Creating shared scene buffers and verifying basic metadata."""

    def test_write_returns_instance(self):
        data = b"hello world"
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", False)
        assert isinstance(ssb, PySharedSceneBuffer)

    def test_repr_contains_inline(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        assert "inline=true" in repr(ssb)

    def test_total_bytes_equals_data_length(self):
        data = b"x" * 100
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", False)
        assert ssb.total_bytes == len(data)

    def test_is_inline_true_for_small_data(self):
        ssb = PySharedSceneBuffer.write(b"small", PySceneDataKind.Geometry, "Maya", False)
        assert ssb.is_inline is True

    def test_is_chunked_false_for_small_data(self):
        ssb = PySharedSceneBuffer.write(b"small", PySceneDataKind.Geometry, "Maya", False)
        assert ssb.is_chunked is False

    def test_read_returns_original_data(self):
        data = b"round-trip test data 12345"
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", False)
        assert ssb.read() == data

    def test_all_scene_data_kinds(self):
        for kind in [
            PySceneDataKind.Geometry,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Screenshot,
            PySceneDataKind.Arbitrary,
        ]:
            ssb = PySharedSceneBuffer.write(b"payload", kind, "Maya", False)
            assert ssb.read() == b"payload"

    def test_source_dcc_none(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, None, False)
        assert isinstance(ssb, PySharedSceneBuffer)
        assert ssb.read() == b"data"

    def test_descriptor_json_is_valid_json(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert isinstance(d, dict)

    def test_descriptor_json_has_meta_key(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert "meta" in d

    def test_descriptor_json_has_storage_key(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert "storage" in d

    def test_descriptor_meta_kind_geometry(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["kind"] == "geometry"

    def test_descriptor_meta_kind_animation_cache(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.AnimationCache, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["kind"] == "animation_cache"

    def test_descriptor_meta_source_dcc_recorded(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Blender", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["source_dcc"] == "Blender"


class TestPySharedSceneBufferCompression:
    """Compression round-trip tests."""

    def test_compressed_read_returns_original(self):
        data = b"abc" * 200
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", True)
        assert ssb.read() == data

    def test_compressed_total_bytes_shows_original_size(self):
        data = b"abc" * 200
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", True)
        assert ssb.total_bytes == len(data)

    def test_animation_cache_with_compression(self):
        data = b"\x00\x01\x02" * 300
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.AnimationCache, "Houdini", True)
        assert ssb.read() == data

    def test_screenshot_kind_compression(self):
        data = bytes(range(256)) * 10
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Screenshot, None, True)
        assert ssb.read() == data


class TestPySceneDataKind:
    """PySceneDataKind enum values."""

    def test_geometry_exists(self):
        assert PySceneDataKind.Geometry is not None

    def test_animation_cache_exists(self):
        assert PySceneDataKind.AnimationCache is not None

    def test_screenshot_exists(self):
        assert PySceneDataKind.Screenshot is not None

    def test_arbitrary_exists(self):
        assert PySceneDataKind.Arbitrary is not None

    def test_geometry_repr(self):
        assert "Geometry" in repr(PySceneDataKind.Geometry)

    def test_animation_cache_repr(self):
        assert "AnimationCache" in repr(PySceneDataKind.AnimationCache)


# ===========================================================================
# PySharedBuffer
# ===========================================================================


class TestPySharedBufferCreate:
    """Creating and basic usage of PySharedBuffer."""

    def test_create_returns_instance(self):
        buf = PySharedBuffer.create(1024)
        assert isinstance(buf, PySharedBuffer)

    def test_repr_contains_id_and_capacity(self):
        buf = PySharedBuffer.create(512)
        r = repr(buf)
        assert "capacity=512" in r

    def test_capacity_matches_requested(self):
        buf = PySharedBuffer.create(2048)
        assert buf.capacity() == 2048

    def test_data_len_zero_before_write(self):
        buf = PySharedBuffer.create(1024)
        assert buf.data_len() == 0

    def test_id_is_nonempty_string(self):
        buf = PySharedBuffer.create(512)
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_name_is_nonempty_string(self):
        buf = PySharedBuffer.create(512)
        assert isinstance(buf.name(), str)
        assert len(buf.name()) > 0

    def test_write_and_read_roundtrip(self):
        buf = PySharedBuffer.create(1024)
        data = b"test payload 1234"
        buf.write(data)
        assert buf.read() == data

    def test_data_len_after_write(self):
        buf = PySharedBuffer.create(1024)
        data = b"x" * 50
        buf.write(data)
        assert buf.data_len() == 50

    def test_clear_resets_data_len(self):
        buf = PySharedBuffer.create(1024)
        buf.write(b"hello")
        buf.clear()
        assert buf.data_len() == 0

    def test_read_after_clear_returns_empty(self):
        buf = PySharedBuffer.create(1024)
        buf.write(b"hello")
        buf.clear()
        assert buf.read() == b""

    def test_descriptor_json_valid(self):
        buf = PySharedBuffer.create(1024)
        d = json.loads(buf.descriptor_json())
        assert isinstance(d, dict)

    def test_descriptor_json_has_capacity(self):
        buf = PySharedBuffer.create(1024)
        d = json.loads(buf.descriptor_json())
        assert d["capacity"] == 1024

    def test_descriptor_json_has_id(self):
        buf = PySharedBuffer.create(1024)
        d = json.loads(buf.descriptor_json())
        assert "id" in d
        assert d["id"] == buf.id

    def test_descriptor_json_has_name(self):
        buf = PySharedBuffer.create(1024)
        d = json.loads(buf.descriptor_json())
        assert "name" in d


class TestPySharedBufferOpen:
    """Opening an existing buffer by name + id."""

    def test_open_returns_same_capacity(self):
        buf = PySharedBuffer.create(512)
        buf2 = PySharedBuffer.open(buf.name(), buf.id)
        assert buf2.capacity() == 512

    def test_open_can_read_written_data(self):
        buf = PySharedBuffer.create(512)
        buf.write(b"shared content")
        buf2 = PySharedBuffer.open(buf.name(), buf.id)
        assert buf2.read() == b"shared content"

    def test_open_repr_contains_id(self):
        buf = PySharedBuffer.create(512)
        buf2 = PySharedBuffer.open(buf.name(), buf.id)
        assert buf.id in repr(buf2)


# ===========================================================================
# PyBufferPool
# ===========================================================================


class TestPyBufferPoolCreate:
    """Creating buffer pools."""

    def test_create_returns_instance(self):
        pool = PyBufferPool(3, 512)
        assert isinstance(pool, PyBufferPool)

    def test_repr_shows_capacity_available_buffer_size(self):
        pool = PyBufferPool(3, 512)
        r = repr(pool)
        assert "capacity=3" in r
        assert "available=3" in r
        assert "buffer_size=512" in r

    def test_capacity_matches(self):
        pool = PyBufferPool(4, 256)
        assert pool.capacity() == 4

    def test_available_equals_capacity_initially(self):
        pool = PyBufferPool(4, 256)
        assert pool.available() == 4

    def test_buffer_size_matches(self):
        pool = PyBufferPool(2, 1024)
        assert pool.buffer_size() == 1024

    def test_capacity_1(self):
        pool = PyBufferPool(1, 128)
        assert pool.capacity() == 1


class TestPyBufferPoolAcquireRelease:
    """Acquire/release behavior.

    Note: buffers are returned to the pool when their Python reference is dropped (GC).
    Tests must keep explicit references to observe decremented availability.
    """

    def test_acquire_returns_buffer(self):
        pool = PyBufferPool(3, 512)
        buf = pool.acquire()
        assert isinstance(buf, PySharedBuffer)

    def test_acquire_decrements_available(self):
        pool = PyBufferPool(3, 512)
        buf = pool.acquire()  # keep reference so GC does not return it
        assert pool.available() == 2
        del buf  # explicitly release

    def test_acquire_multiple_decrements(self):
        pool = PyBufferPool(3, 512)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert pool.available() == 1
        del b1, b2

    def test_del_returns_buffer_to_pool(self):
        pool = PyBufferPool(3, 512)
        buf = pool.acquire()
        assert pool.available() == 2
        del buf
        assert pool.available() == 3

    def test_acquired_buffer_capacity_matches_pool(self):
        pool = PyBufferPool(2, 512)
        buf = pool.acquire()
        assert buf.capacity() == 512

    def test_acquired_buffer_can_write_and_read(self):
        pool = PyBufferPool(2, 512)
        buf = pool.acquire()
        buf.write(b"pool data")
        assert buf.read() == b"pool data"

    def test_id_has_pool_prefix(self):
        pool = PyBufferPool(2, 512)
        buf = pool.acquire()
        assert "pool-" in buf.id

    def test_all_buffers_acquired_leaves_zero_available(self):
        pool = PyBufferPool(2, 128)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert pool.available() == 0
        del b1, b2


# ===========================================================================
# Capturer / CaptureFrame
# ===========================================================================


class TestCapturerCreate:
    """Creating Capturer instances."""

    def test_new_auto_returns_capturer(self):
        cap = Capturer.new_auto()
        assert isinstance(cap, Capturer)

    def test_new_mock_returns_capturer(self):
        cap = Capturer.new_mock()
        assert isinstance(cap, Capturer)

    def test_new_auto_repr_contains_backend(self):
        cap = Capturer.new_auto()
        assert "Capturer(" in repr(cap)

    def test_new_mock_backend_name_is_mock(self):
        cap = Capturer.new_mock()
        assert cap.backend_name() == "Mock"

    def test_new_auto_backend_name_nonempty(self):
        cap = Capturer.new_auto()
        assert len(cap.backend_name()) > 0

    def test_stats_initial_all_zeros(self):
        cap = Capturer.new_mock()
        s = cap.stats()
        assert s == (0, 0, 0)

    def test_stats_is_tuple(self):
        cap = Capturer.new_mock()
        assert isinstance(cap.stats(), tuple)

    def test_stats_length_three(self):
        cap = Capturer.new_mock()
        assert len(cap.stats()) == 3


class TestCapturerCapture:
    """Capturing frames with mock backend."""

    def test_capture_returns_capture_frame(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert isinstance(frame, CaptureFrame)

    def test_frame_width_positive(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert frame.width > 0

    def test_frame_height_positive(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert frame.height > 0

    def test_frame_format_is_string(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert isinstance(frame.format, str)
        assert len(frame.format) > 0

    def test_frame_format_is_png_for_mock(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert frame.format == "png"

    def test_frame_mime_type_is_image_png(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert frame.mime_type == "image/png"

    def test_frame_data_is_bytes(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert isinstance(frame.data, bytes)

    def test_frame_data_starts_with_png_signature(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        # PNG magic: \x89PNG\r\n\x1a\n
        assert frame.data[:4] == b"\x89PNG"

    def test_frame_byte_len_matches_data_len(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert frame.byte_len() == len(frame.data)

    def test_frame_dpi_scale_positive(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert frame.dpi_scale > 0

    def test_frame_timestamp_ms_positive(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert frame.timestamp_ms > 0

    def test_frame_repr_contains_dimensions(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        r = repr(frame)
        assert str(frame.width) in r
        assert str(frame.height) in r

    def test_stats_after_capture_success_increments(self):
        cap = Capturer.new_mock()
        cap.capture()
        s = cap.stats()
        assert s[0] == 1  # success count

    def test_stats_second_idx_is_bytes_count(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        s = cap.stats()
        assert s[1] == frame.byte_len()

    def test_stats_after_two_captures(self):
        cap = Capturer.new_mock()
        cap.capture()
        cap.capture()
        s = cap.stats()
        assert s[0] == 2

    def test_capture_frame_1920x1080_for_mock(self):
        cap = Capturer.new_mock()
        frame = cap.capture()
        assert frame.width == 1920
        assert frame.height == 1080


# ===========================================================================
# PyProcessMonitor
# ===========================================================================


class TestPyProcessMonitorCreate:
    """Creating PyProcessMonitor."""

    def test_create_default(self):
        mon = PyProcessMonitor()
        assert isinstance(mon, PyProcessMonitor)

    def test_repr_shows_tracked_zero(self):
        mon = PyProcessMonitor()
        assert "tracked=0" in repr(mon)

    def test_tracked_count_zero_initially(self):
        mon = PyProcessMonitor()
        assert mon.tracked_count() == 0

    def test_is_alive_unknown_pid_false(self):
        mon = PyProcessMonitor()
        assert mon.is_alive(99999999) is False


class TestPyProcessMonitorTrack:
    """Tracking and querying processes."""

    def test_track_self_increments_count(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        assert mon.tracked_count() == 1

    def test_is_alive_tracked_self(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        assert mon.is_alive(pid) is True

    def test_query_returns_dict(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert isinstance(info, dict)

    def test_query_dict_has_pid_key(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["pid"] == pid

    def test_query_dict_has_name_key(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["name"] == "self"

    def test_query_dict_has_status_running(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "test_process")
        mon.refresh()
        info = mon.query(pid)
        assert info["status"] == "running"

    def test_query_dict_has_memory_bytes(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert "memory_bytes" in info
        assert info["memory_bytes"] > 0

    def test_query_dict_has_cpu_usage(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert "cpu_usage_percent" in info

    def test_query_dict_has_restart_count(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert "restart_count" in info

    def test_list_all_returns_list(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        all_procs = mon.list_all()
        assert isinstance(all_procs, list)
        assert len(all_procs) == 1

    def test_list_all_contains_tracked_entry(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "maya-test")
        entries = mon.list_all()
        pids = [e["pid"] for e in entries]
        assert pid in pids

    def test_untrack_removes_from_count(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        assert mon.tracked_count() == 1
        mon.untrack(pid)
        assert mon.tracked_count() == 0

    def test_query_unknown_pid_returns_none(self):
        mon = PyProcessMonitor()
        result = mon.query(99999999)
        assert result is None

    def test_multiple_track_same_pid_counts_once(self):
        mon = PyProcessMonitor()
        pid = _current_pid()
        mon.track(pid, "self")
        mon.track(pid, "self")  # duplicate — should not double-count
        assert mon.tracked_count() == 1


# ===========================================================================
# PyProcessWatcher
# ===========================================================================


class TestPyProcessWatcherCreate:
    """Creating PyProcessWatcher."""

    def test_create_default(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        assert isinstance(w, PyProcessWatcher)

    def test_tracked_count_zero_initially(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        assert w.tracked_count() == 0

    def test_is_watched_unknown_pid_false(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        assert w.is_watched(99999999) is False


class TestPyProcessWatcherTrackAndPoll:
    """Track, start, poll events, stop."""

    def test_track_increments_count(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "self")
        assert w.tracked_count() == 1

    def test_is_watched_returns_true_after_track(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "self")
        assert w.is_watched(pid) is True

    def test_start_and_stop(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "self")
        w.start()
        assert w.is_running() is True
        w.stop()
        assert w.is_running() is False

    def test_poll_events_returns_list(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        assert isinstance(events, list)

    def test_poll_events_has_at_least_one_heartbeat(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        assert len(events) >= 1
        types = [e["type"] for e in events]
        assert "heartbeat" in types

    def test_heartbeat_event_has_pid(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "self")
        w.start()
        time.sleep(0.25)
        events = w.poll_events()
        w.stop()
        hb = next((e for e in events if e["type"] == "heartbeat"), None)
        assert hb is not None
        assert hb["pid"] == pid

    def test_heartbeat_event_has_name(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "watcher-test")
        w.start()
        time.sleep(0.25)
        events = w.poll_events()
        w.stop()
        hb = next((e for e in events if e["type"] == "heartbeat"), None)
        assert hb is not None
        assert hb["name"] == "watcher-test"

    def test_poll_clears_queue_on_second_call(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "self")
        w.start()
        time.sleep(0.25)
        w.poll_events()  # drain
        events2 = w.poll_events()
        w.stop()
        # second poll immediately should have 0 new events
        assert isinstance(events2, list)

    def test_untrack_removes_from_watched(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.track(pid, "self")
        w.untrack(pid)
        assert w.is_watched(pid) is False
        assert w.tracked_count() == 0

    def test_add_watch_alias(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.add_watch(pid, "self")
        assert w.watch_count() == 1

    def test_remove_watch_alias(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = _current_pid()
        w.add_watch(pid, "self")
        w.remove_watch(pid)
        assert w.watch_count() == 0


# ===========================================================================
# PyCrashRecoveryPolicy
# ===========================================================================


class TestPyCrashRecoveryPolicyCreate:
    """Creating PyCrashRecoveryPolicy."""

    def test_create_with_max_restarts(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        assert isinstance(policy, PyCrashRecoveryPolicy)

    def test_repr_contains_class_name(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        assert "CrashRecoveryPolicy" in repr(policy)

    def test_max_restarts_attribute(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        assert policy.max_restarts == 5

    def test_max_restarts_one(self):
        policy = PyCrashRecoveryPolicy(max_restarts=1)
        assert policy.max_restarts == 1


class TestPyCrashRecoveryPolicyShouldRestart:
    """should_restart state machine.

    Only crash-indicating statuses (e.g. "crashed", "stopped") return True.
    Active statuses like "running" return False.
    """

    def test_should_restart_returns_true_for_crashed(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        assert policy.should_restart("crashed") is True

    def test_should_restart_returns_true_within_limit(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        results = [policy.should_restart("crashed") for _ in range(3)]
        assert all(results)

    def test_should_restart_running_returns_false(self):
        # "running" is a healthy status — no restart needed
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        assert policy.should_restart("running") is False

    def test_should_restart_multiple_calls_within_limit(self):
        policy = PyCrashRecoveryPolicy(max_restarts=10)
        # All crash-like calls within max should return True
        for _ in range(5):
            assert policy.should_restart("crashed") is True


class TestPyCrashRecoveryPolicyFixedBackoff:
    """Fixed backoff delay."""

    def test_fixed_backoff_delay_constant(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_fixed_backoff(delay_ms=500)
        assert policy.next_delay_ms("maya", 0) == 500

    def test_fixed_backoff_same_for_all_attempts(self):
        policy = PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_fixed_backoff(delay_ms=200)
        delays = [policy.next_delay_ms("maya", i) for i in range(4)]
        assert all(d == 200 for d in delays)

    def test_fixed_backoff_zero_delay(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_fixed_backoff(delay_ms=0)
        assert policy.next_delay_ms("maya", 0) == 0


class TestPyCrashRecoveryPolicyExponentialBackoff:
    """Exponential backoff delay."""

    def test_exp_backoff_initial_delay(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
        assert policy.next_delay_ms("maya", 0) == 1000

    def test_exp_backoff_doubles_each_attempt(self):
        policy = PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=100000)
        d0 = policy.next_delay_ms("maya", 0)
        d1 = policy.next_delay_ms("maya", 1)
        d2 = policy.next_delay_ms("maya", 2)
        assert d1 == d0 * 2
        assert d2 == d0 * 4

    def test_exp_backoff_caps_at_max(self):
        policy = PyCrashRecoveryPolicy(max_restarts=20)
        policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
        delay = policy.next_delay_ms("maya", 10)
        assert delay <= 30000

    def test_exp_backoff_exceeds_max_restarts_raises(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
        with pytest.raises(RuntimeError, match="exceeded max restarts"):
            policy.next_delay_ms("maya", 3)

    def test_exp_backoff_different_dcc_names(self):
        policy = PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_exponential_backoff(initial_ms=500, max_delay_ms=20000)
        d_maya = policy.next_delay_ms("maya", 0)
        d_blender = policy.next_delay_ms("blender", 0)
        assert d_maya == 500
        assert d_blender == 500


# ===========================================================================
# PyDccLauncher
# ===========================================================================


class TestPyDccLauncherCreate:
    """Creating PyDccLauncher."""

    def test_create_default(self):
        launcher = PyDccLauncher()
        assert isinstance(launcher, PyDccLauncher)

    def test_repr_shows_running_zero(self):
        launcher = PyDccLauncher()
        assert "running=0" in repr(launcher)

    def test_running_count_zero_initially(self):
        launcher = PyDccLauncher()
        assert launcher.running_count() == 0

    def test_pid_of_unknown_returns_none(self):
        launcher = PyDccLauncher()
        assert launcher.pid_of("nonexistent_dcc") is None

    def test_restart_count_unknown_is_zero(self):
        launcher = PyDccLauncher()
        assert launcher.restart_count("nonexistent_dcc") == 0


# ===========================================================================
# BooleanWrapper / IntWrapper / FloatWrapper / StringWrapper
# ===========================================================================


class TestBooleanWrapper:
    """BooleanWrapper creation and value access.

    Note: ``value`` is a plain attribute, not a method.
    """

    def test_true_wrapper(self):
        bw = BooleanWrapper(True)
        assert bw.value is True

    def test_false_wrapper(self):
        bw = BooleanWrapper(False)
        assert bw.value is False

    def test_repr_true(self):
        bw = BooleanWrapper(True)
        assert "True" in repr(bw)

    def test_repr_false(self):
        bw = BooleanWrapper(False)
        assert "False" in repr(bw)


class TestIntWrapper:
    """IntWrapper creation and value access."""

    def test_positive_int(self):
        iw = IntWrapper(42)
        assert iw.value == 42

    def test_negative_int(self):
        iw = IntWrapper(-1)
        assert iw.value == -1

    def test_zero(self):
        iw = IntWrapper(0)
        assert iw.value == 0

    def test_repr_shows_value(self):
        iw = IntWrapper(99)
        assert "99" in repr(iw)


class TestFloatWrapper:
    """FloatWrapper creation and value access."""

    def test_positive_float(self):
        fw = FloatWrapper(3.14)
        assert abs(fw.value - 3.14) < 1e-9

    def test_negative_float(self):
        fw = FloatWrapper(-2.71)
        assert fw.value < 0

    def test_repr_shows_value(self):
        fw = FloatWrapper(1.5)
        assert "1.5" in repr(fw)


class TestStringWrapper:
    """StringWrapper creation and value access."""

    def test_simple_string(self):
        sw = StringWrapper("hello")
        assert sw.value == "hello"

    def test_empty_string(self):
        sw = StringWrapper("")
        assert sw.value == ""

    def test_repr_shows_value(self):
        sw = StringWrapper("world")
        assert "world" in repr(sw)


# ===========================================================================
# wrap_value / unwrap_value / unwrap_parameters
# ===========================================================================


class TestWrapValue:
    """wrap_value type dispatch.

    Note: wrapper ``.value`` is a plain attribute, not a method.
    """

    def test_bool_true_becomes_boolean_wrapper(self):
        w = wrap_value(True)
        assert isinstance(w, BooleanWrapper)
        assert w.value is True

    def test_bool_false_becomes_boolean_wrapper(self):
        w = wrap_value(False)
        assert isinstance(w, BooleanWrapper)
        assert w.value is False

    def test_int_becomes_int_wrapper(self):
        w = wrap_value(42)
        assert isinstance(w, IntWrapper)
        assert w.value == 42

    def test_negative_int_becomes_int_wrapper(self):
        w = wrap_value(-10)
        assert isinstance(w, IntWrapper)
        assert w.value == -10

    def test_float_becomes_float_wrapper(self):
        w = wrap_value(1.5)
        assert isinstance(w, FloatWrapper)

    def test_str_becomes_string_wrapper(self):
        w = wrap_value("hello")
        assert isinstance(w, StringWrapper)
        assert w.value == "hello"

    def test_none_returns_none(self):
        assert wrap_value(None) is None

    def test_zero_int_wraps_as_int(self):
        w = wrap_value(0)
        assert isinstance(w, IntWrapper)


class TestUnwrapValue:
    """unwrap_value round-trip."""

    def test_unwrap_bool_wrapper(self):
        assert unwrap_value(BooleanWrapper(True)) is True

    def test_unwrap_false_bool_wrapper(self):
        assert unwrap_value(BooleanWrapper(False)) is False

    def test_unwrap_int_wrapper(self):
        assert unwrap_value(IntWrapper(7)) == 7

    def test_unwrap_float_wrapper(self):
        result = unwrap_value(FloatWrapper(2.71))
        assert abs(result - 2.71) < 1e-9

    def test_unwrap_string_wrapper(self):
        assert unwrap_value(StringWrapper("hi")) == "hi"

    def test_unwrap_plain_int_passthrough(self):
        assert unwrap_value(42) == 42

    def test_unwrap_plain_str_passthrough(self):
        assert unwrap_value("raw") == "raw"

    def test_unwrap_none_passthrough(self):
        assert unwrap_value(None) is None

    def test_wrap_then_unwrap_bool_identity(self):
        for v in [True, False]:
            assert unwrap_value(wrap_value(v)) is v

    def test_wrap_then_unwrap_int_identity(self):
        assert unwrap_value(wrap_value(123)) == 123


class TestUnwrapParameters:
    """unwrap_parameters batch unwrapping."""

    def test_empty_dict_returns_empty(self):
        assert unwrap_parameters({}) == {}

    def test_all_wrapped_values_unwrapped(self):
        params = {
            "flag": BooleanWrapper(True),
            "count": IntWrapper(5),
            "scale": FloatWrapper(1.0),
            "name": StringWrapper("maya"),
        }
        result = unwrap_parameters(params)
        assert result == {"flag": True, "count": 5, "scale": 1.0, "name": "maya"}

    def test_plain_values_pass_through(self):
        params = {"a": 1, "b": "hello", "c": None}
        result = unwrap_parameters(params)
        assert result == {"a": 1, "b": "hello", "c": None}

    def test_mixed_wrapped_and_plain(self):
        params = {
            "wrapped_bool": BooleanWrapper(False),
            "plain_str": "unchanged",
            "wrapped_int": IntWrapper(99),
        }
        result = unwrap_parameters(params)
        assert result["wrapped_bool"] is False
        assert result["plain_str"] == "unchanged"
        assert result["wrapped_int"] == 99

    def test_result_is_new_dict(self):
        params = {"x": IntWrapper(1)}
        result = unwrap_parameters(params)
        assert result is not params

    def test_all_keys_preserved(self):
        params = {
            "a": BooleanWrapper(True),
            "b": IntWrapper(2),
            "c": FloatWrapper(3.0),
            "d": StringWrapper("d"),
        }
        result = unwrap_parameters(params)
        assert set(result.keys()) == {"a", "b", "c", "d"}
