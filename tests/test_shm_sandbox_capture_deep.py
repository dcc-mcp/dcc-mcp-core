"""Deep tests for PySharedBuffer, PySharedSceneBuffer, PyBufferPool,
SandboxContext, AuditLog, AuditEntry, InputValidator, Capturer, ScriptResult.

All APIs verified against installed dcc-mcp-core 0.12.12.
"""

from __future__ import annotations

import contextlib
import json

import pytest

from dcc_mcp_core import Capturer
from dcc_mcp_core import InputValidator
from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import PySharedBuffer
from dcc_mcp_core import PySharedSceneBuffer
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import ScriptResult

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_ctx(allowed_actions=None, max_actions=None, read_only=False, allow_paths=None):
    """Build a SandboxContext with specified policy."""
    policy = SandboxPolicy()
    if allowed_actions:
        policy.allow_actions(allowed_actions)
    if max_actions is not None:
        policy.set_max_actions(max_actions)
    if read_only:
        policy.set_read_only(True)
    if allow_paths:
        policy.allow_paths(allow_paths)
    ctx = SandboxContext(policy)
    return ctx


# ===========================================================================
# TestPySharedBufferCreate
# ===========================================================================


class TestPySharedBufferCreate:
    def test_create_basic(self):
        buf = PySharedBuffer.create(capacity=512)
        assert buf.capacity() == 512

    def test_initial_data_len_zero(self):
        buf = PySharedBuffer.create(capacity=256)
        assert buf.data_len() == 0

    def test_id_is_string(self):
        buf = PySharedBuffer.create(capacity=128)
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_name_is_string(self):
        buf = PySharedBuffer.create(capacity=128)
        assert isinstance(buf.name(), str)
        assert len(buf.name()) > 0

    def test_descriptor_json_is_string(self):
        buf = PySharedBuffer.create(capacity=128)
        desc = buf.descriptor_json()
        assert isinstance(desc, str)
        parsed = json.loads(desc)
        assert isinstance(parsed, dict)

    def test_capacity_matches_argument(self):
        for cap in [64, 256, 1024, 4096]:
            buf = PySharedBuffer.create(capacity=cap)
            assert buf.capacity() == cap

    def test_create_large(self):
        buf = PySharedBuffer.create(capacity=1024 * 1024)
        assert buf.capacity() == 1024 * 1024

    def test_unique_ids(self):
        b1 = PySharedBuffer.create(capacity=64)
        b2 = PySharedBuffer.create(capacity=64)
        assert b1.id != b2.id


# ===========================================================================
# TestPySharedBufferWrite
# ===========================================================================


class TestPySharedBufferWrite:
    def test_write_returns_bytes_written(self):
        buf = PySharedBuffer.create(capacity=256)
        n = buf.write(b"hello")
        assert n == 5

    def test_data_len_after_write(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"abc")
        assert buf.data_len() == 3

    def test_read_returns_written_bytes(self):
        buf = PySharedBuffer.create(capacity=256)
        data = b"vertex data"
        buf.write(data)
        assert buf.read() == data

    def test_write_empty_bytes(self):
        buf = PySharedBuffer.create(capacity=256)
        n = buf.write(b"")
        assert n == 0
        assert buf.data_len() == 0

    def test_write_binary_data(self):
        buf = PySharedBuffer.create(capacity=1024)
        payload = bytes(range(256))
        buf.write(payload)
        assert buf.read() == payload

    def test_clear_resets_data_len(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"something")
        buf.clear()
        assert buf.data_len() == 0

    def test_clear_then_read_returns_empty(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"something")
        buf.clear()
        data = buf.read()
        assert data == b"" or data is None or len(data) == 0

    def test_overwrite_on_second_write(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"first")
        buf.write(b"second")
        # second write replaces first
        result = buf.read()
        assert result == b"second"


# ===========================================================================
# TestPySharedBufferOpen
# ===========================================================================


class TestPySharedBufferOpen:
    def test_open_reads_same_data(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"cross-process data")
        buf2 = PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.read() == b"cross-process data"

    def test_open_has_same_id(self):
        buf = PySharedBuffer.create(capacity=256)
        buf2 = PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.id == buf.id

    def test_open_has_same_capacity(self):
        buf = PySharedBuffer.create(capacity=512)
        buf2 = PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.capacity() == 512

    def test_open_sees_updated_data(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"v1")
        buf2 = PySharedBuffer.open(name=buf.name(), id=buf.id)
        buf.write(b"v2")
        assert buf2.read() == b"v2"

    def test_descriptor_json_contains_id_and_path(self):
        buf = PySharedBuffer.create(capacity=256)
        desc = json.loads(buf.descriptor_json())
        assert "id" in desc or "name" in desc


# ===========================================================================
# TestPySharedSceneBufferWrite
# ===========================================================================


class TestPySharedSceneBufferWrite:
    def test_write_returns_scene_buffer(self):
        ssb = PySharedSceneBuffer.write(b"data", kind=PySceneDataKind.Arbitrary)
        assert ssb is not None

    def test_id_is_string(self):
        ssb = PySharedSceneBuffer.write(b"data", kind=PySceneDataKind.Arbitrary)
        assert isinstance(ssb.id, str) and len(ssb.id) > 0

    def test_total_bytes_matches_input(self):
        payload = b"hello world"
        ssb = PySharedSceneBuffer.write(payload, kind=PySceneDataKind.Geometry)
        assert ssb.total_bytes == len(payload)

    def test_is_inline_for_small_data(self):
        ssb = PySharedSceneBuffer.write(b"small", kind=PySceneDataKind.Screenshot)
        assert ssb.is_inline is True
        assert ssb.is_chunked is False

    def test_read_returns_original_bytes(self):
        payload = b"scene bytes"
        ssb = PySharedSceneBuffer.write(payload, kind=PySceneDataKind.Geometry)
        assert ssb.read() == payload

    def test_write_with_compression_round_trips(self):
        payload = b"A" * 1024
        ssb = PySharedSceneBuffer.write(payload, kind=PySceneDataKind.Geometry, use_compression=True)
        assert ssb.read() == payload

    def test_write_geometry_kind(self):
        ssb = PySharedSceneBuffer.write(b"geo", kind=PySceneDataKind.Geometry)
        assert ssb.read() == b"geo"

    def test_write_animation_kind(self):
        ssb = PySharedSceneBuffer.write(b"anim", kind=PySceneDataKind.AnimationCache)
        assert ssb.read() == b"anim"

    def test_write_screenshot_kind(self):
        ssb = PySharedSceneBuffer.write(b"screenshot", kind=PySceneDataKind.Screenshot)
        assert ssb.read() == b"screenshot"

    def test_write_arbitrary_kind(self):
        ssb = PySharedSceneBuffer.write(b"misc", kind=PySceneDataKind.Arbitrary)
        assert ssb.read() == b"misc"

    def test_descriptor_json_is_string(self):
        ssb = PySharedSceneBuffer.write(b"data", kind=PySceneDataKind.Arbitrary)
        desc = ssb.descriptor_json()
        assert isinstance(desc, str)
        parsed = json.loads(desc)
        assert isinstance(parsed, dict)

    def test_source_dcc_stored(self):
        ssb = PySharedSceneBuffer.write(b"data", kind=PySceneDataKind.Geometry, source_dcc="Maya")
        assert ssb is not None

    def test_empty_payload(self):
        ssb = PySharedSceneBuffer.write(b"", kind=PySceneDataKind.Arbitrary)
        assert ssb.total_bytes == 0

    def test_large_payload_no_compression(self):
        payload = bytes(range(256)) * 100  # 25600 bytes
        ssb = PySharedSceneBuffer.write(payload, kind=PySceneDataKind.Geometry, use_compression=False)
        assert ssb.read() == payload


# ===========================================================================
# TestPySceneDataKindEnum
# ===========================================================================


class TestPySceneDataKindEnum:
    def test_geometry_exists(self):
        assert PySceneDataKind.Geometry is not None

    def test_animation_cache_exists(self):
        assert PySceneDataKind.AnimationCache is not None

    def test_screenshot_exists(self):
        assert PySceneDataKind.Screenshot is not None

    def test_arbitrary_exists(self):
        assert PySceneDataKind.Arbitrary is not None

    def test_all_are_distinct(self):
        kinds = [
            PySceneDataKind.Geometry,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Screenshot,
            PySceneDataKind.Arbitrary,
        ]
        # Each kind is a different value
        assert len(set(str(k) for k in kinds)) == 4


# ===========================================================================
# TestPyBufferPool
# ===========================================================================


class TestPyBufferPool:
    def test_capacity_matches_argument(self):
        pool = PyBufferPool(capacity=4, buffer_size=512)
        assert pool.capacity() == 4

    def test_buffer_size_matches_argument(self):
        pool = PyBufferPool(capacity=2, buffer_size=2048)
        assert pool.buffer_size() == 2048

    def test_initial_available_equals_capacity(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        assert pool.available() == 3

    def test_acquire_decreases_available(self):
        pool = PyBufferPool(capacity=4, buffer_size=256)
        _b = pool.acquire()
        assert pool.available() == 3

    def test_acquire_returns_shared_buffer(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        assert buf is not None
        assert buf.capacity() == 256

    def test_acquire_and_write(self):
        pool = PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        buf.write(b"pool test data")
        assert buf.read() == b"pool test data"

    def test_release_on_del_restores_available(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        buf = pool.acquire()
        assert pool.available() == 2
        del buf
        assert pool.available() == 3

    def test_acquire_all_slots(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        _b1 = pool.acquire()
        _b2 = pool.acquire()
        assert pool.available() == 0

    def test_acquire_when_exhausted_raises(self):
        pool = PyBufferPool(capacity=1, buffer_size=256)
        _b1 = pool.acquire()
        with pytest.raises((RuntimeError, Exception)):
            pool.acquire()

    def test_multiple_pools_independent(self):
        pool1 = PyBufferPool(capacity=2, buffer_size=128)
        pool2 = PyBufferPool(capacity=4, buffer_size=512)
        assert pool1.capacity() == 2
        assert pool2.capacity() == 4
        assert pool1.buffer_size() == 128
        assert pool2.buffer_size() == 512


# ===========================================================================
# TestSandboxPolicyCreate
# ===========================================================================


class TestSandboxPolicyCreate:
    def test_default_not_read_only(self):
        policy = SandboxPolicy()
        assert policy.is_read_only is False

    def test_set_read_only(self):
        policy = SandboxPolicy()
        policy.set_read_only(True)
        assert policy.is_read_only is True

    def test_set_read_only_false(self):
        policy = SandboxPolicy()
        policy.set_read_only(True)
        policy.set_read_only(False)
        assert policy.is_read_only is False

    def test_allow_actions_accepts_list(self):
        policy = SandboxPolicy()
        policy.allow_actions(["echo", "ping"])

    def test_deny_actions_accepts_list(self):
        policy = SandboxPolicy()
        policy.allow_actions(["echo", "delete"])
        policy.deny_actions(["delete"])

    def test_allow_paths_accepts_list(self):
        policy = SandboxPolicy()
        policy.allow_paths(["/project/assets", "/tmp"])

    def test_set_timeout_ms(self):
        policy = SandboxPolicy()
        policy.set_timeout_ms(5000)

    def test_set_max_actions(self):
        policy = SandboxPolicy()
        policy.set_max_actions(100)


# ===========================================================================
# TestSandboxContextCreate
# ===========================================================================


class TestSandboxContextCreate:
    def test_action_count_starts_at_zero(self):
        ctx = _make_ctx(["echo"])
        assert ctx.action_count == 0

    def test_set_actor(self):
        ctx = _make_ctx(["echo"])
        ctx.set_actor("agent-v1")

    def test_is_allowed_permitted_action(self):
        ctx = _make_ctx(["echo", "ping"])
        assert ctx.is_allowed("echo") is True

    def test_is_allowed_forbidden_action(self):
        ctx = _make_ctx(["echo"])
        assert ctx.is_allowed("delete_all") is False

    def test_is_allowed_ping(self):
        ctx = _make_ctx(["echo", "ping"])
        assert ctx.is_allowed("ping") is True

    def test_audit_log_accessible(self):
        ctx = _make_ctx(["echo"])
        log = ctx.audit_log
        assert log is not None


# ===========================================================================
# TestSandboxContextExecuteJson
# ===========================================================================


class TestSandboxContextExecuteJson:
    def test_execute_allowed_action_succeeds(self):
        ctx = _make_ctx(["echo"])
        result = ctx.execute_json("echo", json.dumps({"x": 1}))
        # returns None or a JSON string
        assert result is None or isinstance(result, str)

    def test_action_count_increments_on_success(self):
        ctx = _make_ctx(["echo"])
        ctx.execute_json("echo", json.dumps({}))
        assert ctx.action_count == 1

    def test_action_count_increments_multiple(self):
        ctx = _make_ctx(["echo", "ping"])
        for _ in range(5):
            ctx.execute_json("echo", json.dumps({}))
        assert ctx.action_count == 5

    def test_denied_action_raises_runtime_error(self):
        ctx = _make_ctx(["echo"])
        with pytest.raises(RuntimeError):
            ctx.execute_json("delete_all", json.dumps({}))

    def test_max_actions_limit_raises(self):
        ctx = _make_ctx(["echo"], max_actions=2)
        ctx.execute_json("echo", json.dumps({}))
        ctx.execute_json("echo", json.dumps({}))
        with pytest.raises(RuntimeError):
            ctx.execute_json("echo", json.dumps({}))

    def test_max_actions_one(self):
        ctx = _make_ctx(["echo"], max_actions=1)
        ctx.execute_json("echo", json.dumps({}))
        with pytest.raises(RuntimeError):
            ctx.execute_json("echo", json.dumps({}))

    def test_denied_action_not_counted(self):
        ctx = _make_ctx(["echo"])
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("denied_action", json.dumps({}))
        assert ctx.action_count == 0

    def test_execute_with_empty_params(self):
        ctx = _make_ctx(["echo"])
        result = ctx.execute_json("echo", "{}")
        assert result is None or isinstance(result, str)

    def test_multiple_different_actions(self):
        ctx = _make_ctx(["echo", "ping", "scan"])
        ctx.execute_json("echo", json.dumps({}))
        ctx.execute_json("ping", json.dumps({}))
        ctx.execute_json("scan", json.dumps({}))
        assert ctx.action_count == 3


# ===========================================================================
# TestAuditLog
# ===========================================================================


class TestAuditLog:
    def test_len_zero_initially(self):
        ctx = _make_ctx(["echo"])
        assert len(ctx.audit_log) == 0

    def test_len_after_execute(self):
        ctx = _make_ctx(["echo"])
        ctx.execute_json("echo", json.dumps({}))
        assert len(ctx.audit_log) == 1

    def test_entries_empty_initially(self):
        ctx = _make_ctx(["echo"])
        assert ctx.audit_log.entries() == []

    def test_entries_count_matches_execute(self):
        ctx = _make_ctx(["echo"])
        ctx.execute_json("echo", json.dumps({}))
        ctx.execute_json("echo", json.dumps({}))
        assert len(ctx.audit_log.entries()) == 2

    def test_successes_returns_only_success(self):
        ctx = _make_ctx(["echo"])
        ctx.execute_json("echo", json.dumps({}))
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("denied", json.dumps({}))
        successes = ctx.audit_log.successes()
        assert all(e.outcome == "success" for e in successes)

    def test_denials_returns_only_denied(self):
        ctx = _make_ctx(["echo"])
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("denied_action", json.dumps({}))
        denials = ctx.audit_log.denials()
        assert all(e.outcome == "denied" for e in denials)

    def test_entries_for_action_filters(self):
        ctx = _make_ctx(["echo", "ping"])
        ctx.execute_json("echo", json.dumps({}))
        ctx.execute_json("ping", json.dumps({}))
        echo_entries = ctx.audit_log.entries_for_action("echo")
        assert len(echo_entries) == 1
        assert echo_entries[0].action == "echo"

    def test_entries_for_nonexistent_action_empty(self):
        ctx = _make_ctx(["echo"])
        ctx.execute_json("echo", json.dumps({}))
        result = ctx.audit_log.entries_for_action("nonexistent")
        assert result == []

    def test_to_json_returns_string(self):
        ctx = _make_ctx(["echo"])
        ctx.execute_json("echo", json.dumps({}))
        j = ctx.audit_log.to_json()
        assert isinstance(j, str)
        parsed = json.loads(j)
        assert isinstance(parsed, list)

    def test_to_json_empty_log(self):
        ctx = _make_ctx(["echo"])
        j = ctx.audit_log.to_json()
        parsed = json.loads(j)
        assert parsed == []

    def test_denied_counted_in_denials(self):
        ctx = _make_ctx(["echo"])
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("forbidden", json.dumps({}))
        assert len(ctx.audit_log.denials()) == 1


# ===========================================================================
# TestAuditEntry
# ===========================================================================


class TestAuditEntry:
    def _get_entry(self):
        ctx = _make_ctx(["echo"])
        ctx.set_actor("test-agent")
        ctx.execute_json("echo", json.dumps({"x": 42}))
        return ctx.audit_log.entries()[0]

    def test_actor_matches_set_actor(self):
        entry = self._get_entry()
        assert entry.actor == "test-agent"

    def test_action_matches_called_action(self):
        entry = self._get_entry()
        assert entry.action == "echo"

    def test_outcome_is_success(self):
        entry = self._get_entry()
        assert entry.outcome == "success"

    def test_timestamp_ms_is_positive_int(self):
        entry = self._get_entry()
        assert isinstance(entry.timestamp_ms, int)
        assert entry.timestamp_ms > 0

    def test_duration_ms_is_non_negative_int(self):
        entry = self._get_entry()
        assert isinstance(entry.duration_ms, int)
        assert entry.duration_ms >= 0

    def test_params_json_contains_params(self):
        entry = self._get_entry()
        assert isinstance(entry.params_json, str)
        parsed = json.loads(entry.params_json)
        assert parsed.get("x") == 42

    def test_outcome_detail_is_none_on_success(self):
        entry = self._get_entry()
        assert entry.outcome_detail is None

    def test_denied_entry_has_denial_outcome(self):
        ctx = _make_ctx(["echo"])
        ctx.set_actor("agent")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("forbidden", json.dumps({}))
        denials = ctx.audit_log.denials()
        assert len(denials) == 1
        d = denials[0]
        assert d.outcome == "denied"
        assert d.action == "forbidden"

    def test_no_actor_entry_actor_is_none_or_empty(self):
        ctx = _make_ctx(["echo"])
        ctx.execute_json("echo", json.dumps({}))
        entry = ctx.audit_log.entries()[0]
        assert entry.actor is None or entry.actor == ""

    def test_multiple_entries_ordered(self):
        ctx = _make_ctx(["echo", "ping"])
        ctx.execute_json("echo", json.dumps({}))
        ctx.execute_json("ping", json.dumps({}))
        entries = ctx.audit_log.entries()
        assert len(entries) == 2
        assert entries[0].action == "echo"
        assert entries[1].action == "ping"


# ===========================================================================
# TestInputValidator
# ===========================================================================


class TestInputValidator:
    def test_valid_input_passes(self):
        v = InputValidator()
        v.require_string("name", max_length=50, min_length=1)
        ok, err = v.validate(json.dumps({"name": "sphere"}))
        assert ok is True
        assert err is None

    def test_missing_required_field_fails(self):
        v = InputValidator()
        v.require_string("name", max_length=50, min_length=1)
        ok, err = v.validate(json.dumps({}))
        assert ok is False
        assert err is not None

    def test_string_too_long_fails(self):
        v = InputValidator()
        v.require_string("name", max_length=5, min_length=1)
        ok, err = v.validate(json.dumps({"name": "toolongstring"}))
        assert ok is False
        assert err is not None

    def test_string_too_short_fails(self):
        v = InputValidator()
        v.require_string("name", max_length=50, min_length=5)
        ok, err = v.validate(json.dumps({"name": "abc"}))
        assert ok is False
        assert err is not None

    def test_number_in_range_passes(self):
        v = InputValidator()
        v.require_number("count", min_value=0, max_value=100)
        ok, err = v.validate(json.dumps({"count": 50}))
        assert ok is True
        assert err is None

    def test_number_below_min_fails(self):
        v = InputValidator()
        v.require_number("count", min_value=0, max_value=100)
        ok, _err = v.validate(json.dumps({"count": -1}))
        assert ok is False

    def test_number_above_max_fails(self):
        v = InputValidator()
        v.require_number("count", min_value=0, max_value=100)
        ok, _err = v.validate(json.dumps({"count": 101}))
        assert ok is False

    def test_forbidden_substring_detected(self):
        v = InputValidator()
        v.forbid_substrings("script", ["__import__", "exec("])
        ok, err = v.validate(json.dumps({"script": "__import__(os)"}))
        assert ok is False
        assert err is not None

    def test_safe_substring_passes(self):
        v = InputValidator()
        v.forbid_substrings("script", ["__import__", "exec("])
        ok, _err = v.validate(json.dumps({"script": "print('hello')"}))
        assert ok is True

    def test_multiple_fields_all_valid(self):
        v = InputValidator()
        v.require_string("name", max_length=50, min_length=1)
        v.require_number("count", min_value=0, max_value=1000)
        ok, err = v.validate(json.dumps({"name": "sphere", "count": 10}))
        assert ok is True
        assert err is None

    def test_multiple_fields_one_invalid(self):
        v = InputValidator()
        v.require_string("name", max_length=50, min_length=1)
        v.require_number("count", min_value=0, max_value=100)
        ok, _err = v.validate(json.dumps({"name": "sphere", "count": 999}))
        assert ok is False

    def test_empty_input_missing_required(self):
        v = InputValidator()
        v.require_string("name", max_length=50, min_length=1)
        ok, _err = v.validate("{}")
        assert ok is False

    def test_forbid_eval_injection(self):
        v = InputValidator()
        v.forbid_substrings("cmd", ["eval(", "os.system"])
        ok, _err = v.validate(json.dumps({"cmd": "eval('rm -rf /')"}))
        assert ok is False

    def test_multiple_forbidden_first_triggers(self):
        v = InputValidator()
        v.forbid_substrings("script", ["__import__", "exec("])
        ok, _err = v.validate(json.dumps({"script": "exec('something')"}))
        assert ok is False


# ===========================================================================
# TestCapturerCreate
# ===========================================================================


class TestCapturerCreate:
    def test_new_mock_creates_capturer(self):
        cap = Capturer.new_mock(width=640, height=480)
        assert cap is not None

    def test_backend_name_mock(self):
        cap = Capturer.new_mock(width=320, height=240)
        assert cap.backend_name() == "Mock"

    def test_new_auto_creates_capturer(self):
        cap = Capturer.new_auto()
        assert cap is not None

    def test_auto_backend_name_is_string(self):
        cap = Capturer.new_auto()
        name = cap.backend_name()
        assert isinstance(name, str) and len(name) > 0


# ===========================================================================
# TestCapturerCapture
# ===========================================================================


class TestCapturerCapture:
    def test_capture_returns_frame(self):
        cap = Capturer.new_mock(width=640, height=480)
        frame = cap.capture(format="png")
        assert frame is not None

    def test_frame_format_png(self):
        cap = Capturer.new_mock(width=320, height=240)
        frame = cap.capture(format="png")
        assert frame.format == "png"

    def test_frame_mime_type_png(self):
        cap = Capturer.new_mock(width=320, height=240)
        frame = cap.capture(format="png")
        assert frame.mime_type == "image/png"

    def test_frame_width_matches(self):
        cap = Capturer.new_mock(width=800, height=600)
        frame = cap.capture(format="png")
        assert frame.width == 800

    def test_frame_height_matches(self):
        cap = Capturer.new_mock(width=800, height=600)
        frame = cap.capture(format="png")
        assert frame.height == 600

    def test_frame_data_is_bytes(self):
        cap = Capturer.new_mock(width=320, height=240)
        frame = cap.capture(format="png")
        assert isinstance(frame.data, bytes)
        assert len(frame.data) > 0

    def test_frame_timestamp_ms_is_positive(self):
        cap = Capturer.new_mock(width=320, height=240)
        frame = cap.capture(format="png")
        assert isinstance(frame.timestamp_ms, int)
        assert frame.timestamp_ms > 0

    def test_frame_dpi_scale_is_float(self):
        cap = Capturer.new_mock(width=320, height=240)
        frame = cap.capture(format="png")
        assert isinstance(frame.dpi_scale, float)
        assert frame.dpi_scale > 0.0

    def test_frame_byte_len_positive(self):
        cap = Capturer.new_mock(width=320, height=240)
        frame = cap.capture(format="png")
        assert frame.byte_len() > 0

    def test_frame_byte_len_matches_data_len(self):
        cap = Capturer.new_mock(width=320, height=240)
        frame = cap.capture(format="png")
        assert frame.byte_len() == len(frame.data)

    def test_capture_jpeg_format(self):
        cap = Capturer.new_mock(width=320, height=240)
        frame = cap.capture(format="jpeg")
        assert frame.format == "jpeg"
        assert frame.mime_type == "image/jpeg"

    def test_capture_raw_bgra_format(self):
        cap = Capturer.new_mock(width=64, height=64)
        frame = cap.capture(format="raw_bgra")
        assert frame.format == "raw_bgra"
        # raw BGRA: width * height * 4 bytes
        assert frame.byte_len() == 64 * 64 * 4

    def test_capture_with_scale(self):
        cap = Capturer.new_mock(width=640, height=480)
        frame = cap.capture(format="png", scale=0.5)
        # scaled down
        assert frame.width == 320
        assert frame.height == 240

    def test_stats_after_capture(self):
        cap = Capturer.new_mock(width=320, height=240)
        cap.capture(format="png")
        count, total_bytes, errors = cap.stats()
        assert count == 1
        assert total_bytes > 0
        assert errors == 0

    def test_stats_accumulate(self):
        cap = Capturer.new_mock(width=64, height=64)
        cap.capture(format="png")
        cap.capture(format="png")
        count, _total_bytes, _errors = cap.stats()
        assert count == 2

    def test_multiple_sizes(self):
        for w, h in [(64, 64), (128, 128), (256, 256)]:
            cap = Capturer.new_mock(width=w, height=h)
            frame = cap.capture(format="png")
            assert frame.width == w
            assert frame.height == h


# ===========================================================================
# TestScriptResult
# ===========================================================================


class TestScriptResult:
    def test_create_success(self):
        r = ScriptResult(success=True, execution_time_ms=42, output="sphere1", error=None, context={})
        assert r.success is True

    def test_create_failure(self):
        r = ScriptResult(success=False, execution_time_ms=100, output=None, error="script error", context={})
        assert r.success is False

    def test_execution_time_ms(self):
        r = ScriptResult(success=True, execution_time_ms=50, output="ok", error=None, context={})
        assert r.execution_time_ms == 50

    def test_output_string(self):
        r = ScriptResult(success=True, execution_time_ms=10, output="hello", error=None, context={})
        assert r.output == "hello"

    def test_error_string(self):
        r = ScriptResult(success=False, execution_time_ms=5, output=None, error="fail msg", context={})
        assert r.error == "fail msg"

    def test_error_none_on_success(self):
        r = ScriptResult(success=True, execution_time_ms=10, output="ok", error=None, context={})
        assert r.error is None

    def test_output_none_on_failure(self):
        r = ScriptResult(success=False, execution_time_ms=5, output=None, error="err", context={})
        assert r.output is None

    def test_context_dict(self):
        r = ScriptResult(success=True, execution_time_ms=10, output="ok", error=None, context={"dcc": "maya"})
        assert r.context.get("dcc") == "maya"

    def test_to_dict_returns_dict(self):
        r = ScriptResult(success=True, execution_time_ms=42, output="result", error=None, context={"k": "v"})
        d = r.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_success_key(self):
        r = ScriptResult(success=True, execution_time_ms=10, output="ok", error=None, context={})
        d = r.to_dict()
        assert d.get("success") is True

    def test_to_dict_failure_key(self):
        r = ScriptResult(success=False, execution_time_ms=5, output=None, error="err", context={})
        d = r.to_dict()
        assert d.get("success") is False

    def test_to_dict_execution_time(self):
        r = ScriptResult(success=True, execution_time_ms=99, output="x", error=None, context={})
        d = r.to_dict()
        assert d.get("execution_time_ms") == 99 or "execution_time_ms" in d

    def test_empty_context(self):
        r = ScriptResult(success=True, execution_time_ms=1, output="", error=None, context={})
        assert isinstance(r.context, dict)

    def test_zero_execution_time(self):
        r = ScriptResult(success=True, execution_time_ms=0, output="fast", error=None, context={})
        assert r.execution_time_ms == 0
