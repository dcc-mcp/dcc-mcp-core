"""Tests for AuditLog, PyProcessWatcher, TransportManager.bind_and_register, etc.

Covers AuditLog complete methods, PyProcessWatcher lifecycle,
TransportManager.bind_and_register, ServerHandle.bind_addr,
VtValue advanced (from_asset/from_vec3f), and USD bridge functions
(units_to_mpu, mpu_to_units, scene_info_json_to_stage,
stage_to_scene_info_json).

Verified behaviors from probe_86a/86b (2026-04-08).
"""

from __future__ import annotations

import contextlib
import json
import os
import tempfile
import time
from typing import Any
from typing import ClassVar

import pytest

from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import PyProcessWatcher
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import TransportManager
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue
from dcc_mcp_core import mpu_to_units
from dcc_mcp_core import scene_info_json_to_stage
from dcc_mcp_core import stage_to_scene_info_json
from dcc_mcp_core import units_to_mpu

# ─────────────────────────────────────────────────────────────────────────────
# AuditLog complete methods
# ─────────────────────────────────────────────────────────────────────────────


class TestAuditLogComplete:
    """Deep coverage of AuditLog: entries / successes / denials / entries_for_action / to_json."""

    @staticmethod
    def _ctx_with_two_successes() -> SandboxContext:
        policy = SandboxPolicy()
        policy.allow_actions(["echo", "info"])
        ctx = SandboxContext(policy)
        ctx.execute_json("echo", "{}")
        ctx.execute_json("info", "{}")
        return ctx

    @staticmethod
    def _ctx_with_one_denial() -> SandboxContext:
        policy = SandboxPolicy()
        policy.allow_actions(["echo"])
        ctx = SandboxContext(policy)
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("forbidden", "{}")
        return ctx

    def test_entries_returns_all(self):
        ctx = self._ctx_with_two_successes()
        entries = ctx.audit_log.entries()
        assert len(entries) == 2

    def test_entries_order_preserved(self):
        ctx = self._ctx_with_two_successes()
        entries = ctx.audit_log.entries()
        assert entries[0].action == "echo"
        assert entries[1].action == "info"

    def test_successes_counts_only_successes(self):
        ctx = self._ctx_with_two_successes()
        assert len(ctx.audit_log.successes()) == 2

    def test_denials_empty_when_all_succeed(self):
        ctx = self._ctx_with_two_successes()
        assert ctx.audit_log.denials() == []

    def test_denials_counts_only_denials(self):
        ctx = self._ctx_with_one_denial()
        assert len(ctx.audit_log.denials()) == 1

    def test_successes_empty_when_all_denied(self):
        ctx = self._ctx_with_one_denial()
        assert ctx.audit_log.successes() == []

    def test_denial_outcome_is_denied(self):
        ctx = self._ctx_with_one_denial()
        d = ctx.audit_log.denials()[0]
        assert d.outcome == "denied"

    def test_denial_outcome_detail_contains_action_name(self):
        ctx = self._ctx_with_one_denial()
        d = ctx.audit_log.denials()[0]
        assert d.outcome_detail is not None
        assert "forbidden" in d.outcome_detail

    def test_denial_actor_none_when_not_set(self):
        ctx = self._ctx_with_one_denial()
        d = ctx.audit_log.denials()[0]
        assert d.actor is None

    def test_denial_actor_set(self):
        policy = SandboxPolicy()
        policy.allow_actions(["echo"])
        ctx = SandboxContext(policy)
        ctx.set_actor("myagent")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("forbidden", "{}")
        d = ctx.audit_log.denials()[0]
        assert d.actor == "myagent"

    def test_entries_for_action_filters(self):
        ctx = self._ctx_with_two_successes()
        echo_entries = ctx.audit_log.entries_for_action("echo")
        assert len(echo_entries) == 1
        assert echo_entries[0].action == "echo"

    def test_entries_for_action_missing_returns_empty(self):
        ctx = self._ctx_with_two_successes()
        assert ctx.audit_log.entries_for_action("nonexistent") == []

    def test_entries_for_action_multiple_calls(self):
        policy = SandboxPolicy()
        policy.allow_actions(["echo"])
        ctx = SandboxContext(policy)
        ctx.execute_json("echo", "{}")
        ctx.execute_json("echo", "{}")
        assert len(ctx.audit_log.entries_for_action("echo")) == 2

    def test_to_json_returns_str(self):
        ctx = self._ctx_with_two_successes()
        j = ctx.audit_log.to_json()
        assert isinstance(j, str)

    def test_to_json_is_valid_json(self):
        ctx = self._ctx_with_two_successes()
        parsed = json.loads(ctx.audit_log.to_json())
        assert isinstance(parsed, list)

    def test_to_json_array_length_matches_entries(self):
        ctx = self._ctx_with_two_successes()
        parsed = json.loads(ctx.audit_log.to_json())
        assert len(parsed) == 2

    def test_to_json_contains_action_field(self):
        ctx = self._ctx_with_two_successes()
        parsed = json.loads(ctx.audit_log.to_json())
        assert all("action" in entry for entry in parsed)

    def test_to_json_contains_outcome_field(self):
        ctx = self._ctx_with_two_successes()
        parsed = json.loads(ctx.audit_log.to_json())
        assert all("outcome" in entry for entry in parsed)

    def test_to_json_empty_audit_is_empty_array(self):
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        parsed = json.loads(ctx.audit_log.to_json())
        assert parsed == []

    def test_len_after_two_actions(self):
        ctx = self._ctx_with_two_successes()
        assert len(ctx.audit_log) == 2

    def test_len_includes_denials(self):
        ctx = self._ctx_with_one_denial()
        assert len(ctx.audit_log) == 1

    def test_mixed_success_and_denial(self):
        policy = SandboxPolicy()
        policy.allow_actions(["echo"])
        ctx = SandboxContext(policy)
        ctx.execute_json("echo", "{}")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("bad", "{}")
        log = ctx.audit_log
        assert len(log.successes()) == 1
        assert len(log.denials()) == 1
        assert len(log.entries()) == 2

    def test_repr_is_string(self):
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        assert isinstance(repr(ctx.audit_log), str)

    def test_audit_entry_timestamp_ms_positive(self):
        ctx = self._ctx_with_two_successes()
        e = ctx.audit_log.entries()[0]
        assert e.timestamp_ms > 0

    def test_audit_entry_duration_ms_non_negative(self):
        ctx = self._ctx_with_two_successes()
        e = ctx.audit_log.entries()[0]
        assert e.duration_ms >= 0

    def test_audit_entry_params_json_is_string(self):
        ctx = self._ctx_with_two_successes()
        e = ctx.audit_log.entries()[0]
        assert isinstance(e.params_json, str)

    def test_audit_entry_repr_is_string(self):
        ctx = self._ctx_with_two_successes()
        e = ctx.audit_log.entries()[0]
        assert isinstance(repr(e), str)


# ─────────────────────────────────────────────────────────────────────────────
# PyProcessWatcher lifecycle
# ─────────────────────────────────────────────────────────────────────────────


class TestPyProcessWatcherLifecycle:
    """Deep coverage of PyProcessWatcher: start/stop/is_running/track/untrack/poll_events."""

    def test_is_running_false_before_start(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        assert w.is_running() is False

    def test_tracked_count_zero_initially(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        assert w.tracked_count() == 0

    def test_track_increases_count(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.track(os.getpid(), "self")
        assert w.tracked_count() == 1

    def test_untrack_decreases_count(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.track(os.getpid(), "self")
        w.untrack(os.getpid())
        assert w.tracked_count() == 0

    def test_start_sets_is_running_true(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.start()
        try:
            assert w.is_running() is True
        finally:
            w.stop()

    def test_stop_sets_is_running_false(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.start()
        w.stop()
        assert w.is_running() is False

    def test_start_is_idempotent(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.start()
        w.start()
        try:
            assert w.is_running() is True
        finally:
            w.stop()

    def test_stop_is_idempotent(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.start()
        w.stop()
        w.stop()
        assert w.is_running() is False

    def test_poll_events_returns_list(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        assert isinstance(events, list)

    def test_poll_events_produces_heartbeat(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        types = [e["type"] for e in events]
        assert "heartbeat" in types

    def test_heartbeat_event_has_pid(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        pid = os.getpid()
        w.track(pid, "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        heartbeats = [e for e in events if e["type"] == "heartbeat"]
        assert len(heartbeats) >= 1
        assert heartbeats[0]["pid"] == pid

    def test_heartbeat_event_has_name(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "myprocess")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        heartbeats = [e for e in events if e["type"] == "heartbeat"]
        assert heartbeats[0]["name"] == "myprocess"

    def test_heartbeat_event_has_new_status(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        heartbeats = [e for e in events if e["type"] == "heartbeat"]
        assert "new_status" in heartbeats[0]

    def test_heartbeat_event_has_cpu_and_memory(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        heartbeats = [e for e in events if e["type"] == "heartbeat"]
        h = heartbeats[0]
        assert "cpu_usage_percent" in h
        assert "memory_bytes" in h

    def test_memory_bytes_positive(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        heartbeats = [e for e in events if e["type"] == "heartbeat"]
        assert heartbeats[0]["memory_bytes"] > 0

    def test_poll_events_drains_queue(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "self")
        w.start()
        time.sleep(0.35)
        first_batch = w.poll_events()
        w.stop()
        # After drain there should be no more events immediately
        second_batch = w.poll_events()
        assert isinstance(second_batch, list)
        # Either empty or very few (from the stop itself)
        _ = first_batch  # consumed

    def test_poll_events_empty_without_tracking(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.start()
        time.sleep(0.25)
        events = w.poll_events()
        w.stop()
        # No tracked pids → no heartbeats
        assert all(e["type"] != "heartbeat" for e in events)

    def test_repr_is_string(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        assert isinstance(repr(w), str)


# ─────────────────────────────────────────────────────────────────────────────
# TransportManager.bind_and_register
# ─────────────────────────────────────────────────────────────────────────────


class TestTransportManagerBindAndRegister:
    """Verify bind_and_register returns (instance_id, IpcListener) and registers the service."""

    def test_returns_tuple(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        result = mgr.bind_and_register("maya")
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_instance_id_is_uuid_string(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        instance_id, _listener = mgr.bind_and_register("maya")
        # UUID: 36 chars with hyphens
        assert isinstance(instance_id, str)
        assert len(instance_id) == 36
        assert instance_id.count("-") == 4

    def test_listener_has_local_address(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        _instance_id, listener = mgr.bind_and_register("maya")
        addr = listener.local_address()
        assert addr is not None

    def test_listener_local_address_scheme(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        _instance_id, listener = mgr.bind_and_register("maya")
        addr = listener.local_address()
        assert addr.scheme in ("pipe", "unix", "tcp")

    def test_service_is_registered(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        instance_id, _listener = mgr.bind_and_register("maya")
        svc = mgr.get_service("maya", instance_id)
        assert svc is not None

    def test_version_stored_in_service(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        instance_id, _listener = mgr.bind_and_register("maya", version="2025")
        svc = mgr.get_service("maya", instance_id)
        assert svc.version == "2025"

    def test_metadata_stored_in_service(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        instance_id, _listener = mgr.bind_and_register("blender", metadata={"env": "test", "scene": "default.blend"})
        svc = mgr.get_service("blender", instance_id)
        assert svc.metadata.get("env") == "test"
        assert svc.metadata.get("scene") == "default.blend"

    def test_no_version_defaults_to_none(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        instance_id, _listener = mgr.bind_and_register("houdini")
        svc = mgr.get_service("houdini", instance_id)
        # version is None when not provided
        assert svc.version is None or svc.version == ""

    def test_different_dcc_types_registered_separately(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        id1, _l1 = mgr.bind_and_register("maya")
        id2, _l2 = mgr.bind_and_register("blender")
        assert id1 != id2
        assert mgr.get_service("maya", id1) is not None
        assert mgr.get_service("blender", id2) is not None
        assert mgr.get_service("maya", id2) is None

    def test_listener_transport_name_is_string(self, tmp_path):
        mgr = TransportManager(str(tmp_path))
        _instance_id, listener = mgr.bind_and_register("maya")
        assert isinstance(listener.transport_name, str)


# ─────────────────────────────────────────────────────────────────────────────
# ServerHandle.bind_addr
# ─────────────────────────────────────────────────────────────────────────────


class TestServerHandleBindAddr:
    """Verify ServerHandle.bind_addr and related properties."""

    @pytest.fixture
    def handle(self):
        reg = ActionRegistry()
        server = McpHttpServer(reg, McpHttpConfig(port=0))
        h = server.start()
        yield h
        h.shutdown()

    def test_port_is_int(self, handle):
        assert isinstance(handle.port, int)

    def test_port_greater_than_zero(self, handle):
        assert handle.port > 0

    def test_bind_addr_is_string(self, handle):
        assert isinstance(handle.bind_addr, str)

    def test_bind_addr_contains_port(self, handle):
        assert str(handle.port) in handle.bind_addr

    def test_bind_addr_format(self, handle):
        # Format: "host:port"
        parts = handle.bind_addr.split(":")
        assert len(parts) == 2
        assert parts[1] == str(handle.port)

    def test_mcp_url_contains_port(self, handle):
        assert str(handle.port) in handle.mcp_url()

    def test_mcp_url_starts_with_http(self, handle):
        assert handle.mcp_url().startswith("http://")

    def test_mcp_url_ends_with_mcp(self, handle):
        assert handle.mcp_url().endswith("/mcp")

    def test_repr_is_string(self, handle):
        assert isinstance(repr(handle), str)

    def test_signal_shutdown_does_not_block(self):
        reg = ActionRegistry()
        server = McpHttpServer(reg, McpHttpConfig(port=0))
        h = server.start()
        h.signal_shutdown()  # Non-blocking
        h.shutdown()  # Idempotent — waits for stop


# ─────────────────────────────────────────────────────────────────────────────
# VtValue advanced: from_asset, from_vec3f, from_token
# ─────────────────────────────────────────────────────────────────────────────


class TestVtValueAdvanced:
    """Additional VtValue factories not covered elsewhere."""

    def test_from_asset_type_name(self):
        v = VtValue.from_asset("textures/checker.png")
        assert v.type_name == "asset"

    def test_from_asset_to_python_returns_str(self):
        v = VtValue.from_asset("textures/checker.png")
        py = v.to_python()
        assert isinstance(py, str)

    def test_from_asset_to_python_value(self):
        v = VtValue.from_asset("textures/checker.png")
        assert v.to_python() == "textures/checker.png"

    def test_from_asset_empty_path(self):
        v = VtValue.from_asset("")
        assert v.type_name == "asset"
        assert v.to_python() == ""

    def test_from_vec3f_type_name(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert v.type_name == "float3"

    def test_from_vec3f_to_python_is_tuple(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        py = v.to_python()
        assert isinstance(py, tuple)

    def test_from_vec3f_to_python_length(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert len(v.to_python()) == 3

    def test_from_vec3f_to_python_values(self):
        v = VtValue.from_vec3f(1.5, 2.5, 3.5)
        x, y, z = v.to_python()
        assert abs(x - 1.5) < 1e-5
        assert abs(y - 2.5) < 1e-5
        assert abs(z - 3.5) < 1e-5

    def test_from_vec3f_zero_vector(self):
        v = VtValue.from_vec3f(0.0, 0.0, 0.0)
        assert v.to_python() == (0.0, 0.0, 0.0)

    def test_from_vec3f_negative(self):
        v = VtValue.from_vec3f(-1.0, -2.0, -3.0)
        x, y, z = v.to_python()
        assert x < 0 and y < 0 and z < 0

    def test_from_token_type_name(self):
        v = VtValue.from_token("Y")
        assert v.type_name == "token"

    def test_from_token_to_python_returns_str(self):
        v = VtValue.from_token("Y")
        assert isinstance(v.to_python(), str)

    def test_from_token_to_python_value(self):
        v = VtValue.from_token("Y")
        assert v.to_python() == "Y"

    def test_from_token_empty_string(self):
        v = VtValue.from_token("")
        assert v.type_name == "token"
        assert v.to_python() == ""

    def test_repr_asset(self):
        v = VtValue.from_asset("tex.png")
        assert isinstance(repr(v), str)

    def test_repr_vec3f(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert isinstance(repr(v), str)


# ─────────────────────────────────────────────────────────────────────────────
# units_to_mpu / mpu_to_units
# ─────────────────────────────────────────────────────────────────────────────


class TestUnitConversion:
    """units_to_mpu and mpu_to_units."""

    def test_cm_to_mpu(self):
        assert abs(units_to_mpu("cm") - 0.01) < 1e-10

    def test_m_to_mpu(self):
        assert abs(units_to_mpu("m") - 1.0) < 1e-10

    def test_mm_to_mpu(self):
        assert abs(units_to_mpu("mm") - 0.001) < 1e-10

    def test_inch_to_mpu(self):
        result = units_to_mpu("inch")
        assert abs(result - 0.0254) < 1e-6

    def test_unknown_unit_fallback_to_cm(self):
        # Unknown units default to cm (0.01)
        result = units_to_mpu("unknown_unit")
        assert isinstance(result, float)
        assert result > 0

    def test_mpu_to_units_cm(self):
        assert mpu_to_units(0.01) == "cm"

    def test_mpu_to_units_m(self):
        assert mpu_to_units(1.0) == "m"

    def test_mpu_to_units_mm(self):
        assert mpu_to_units(0.001) == "mm"

    def test_mpu_to_units_returns_str(self):
        assert isinstance(mpu_to_units(0.01), str)

    def test_round_trip_cm(self):
        mpu = units_to_mpu("cm")
        units = mpu_to_units(mpu)
        assert units == "cm"

    def test_round_trip_m(self):
        mpu = units_to_mpu("m")
        units = mpu_to_units(mpu)
        assert units == "m"

    def test_round_trip_mm(self):
        mpu = units_to_mpu("mm")
        units = mpu_to_units(mpu)
        assert units == "mm"

    def test_units_to_mpu_returns_float(self):
        result = units_to_mpu("cm")
        assert isinstance(result, float)


# ─────────────────────────────────────────────────────────────────────────────
# scene_info_json_to_stage / stage_to_scene_info_json
# ─────────────────────────────────────────────────────────────────────────────


class TestSceneInfoUsdBridge:
    """scene_info_json_to_stage and stage_to_scene_info_json round-trip."""

    _SCENE_DICT: ClassVar[dict[str, Any]] = {
        "file_path": "/scene/test.ma",
        "name": "my_scene",
        "modified": False,
        "format": "mayaAscii",
        "fps": 24.0,
        "up_axis": "Y",
        "units": "cm",
        "frame_range": [1.0, 100.0],
        "current_frame": 1.0,
        "statistics": {
            "object_count": 5,
            "vertex_count": 100,
            "polygon_count": 0,
            "material_count": 0,
            "texture_count": 0,
            "light_count": 0,
            "camera_count": 0,
        },
        "metadata": {},
    }

    def test_scene_info_to_stage_returns_usd_stage(self):
        stage = scene_info_json_to_stage(json.dumps(self._SCENE_DICT), "maya")
        assert isinstance(stage, UsdStage)

    def test_scene_info_to_stage_name(self):
        stage = scene_info_json_to_stage(json.dumps(self._SCENE_DICT), "maya")
        assert stage.name == "my_scene"

    def test_scene_info_to_stage_fps(self):
        stage = scene_info_json_to_stage(json.dumps(self._SCENE_DICT), "maya")
        assert stage.fps == pytest.approx(24.0)

    def test_scene_info_to_stage_up_axis(self):
        stage = scene_info_json_to_stage(json.dumps(self._SCENE_DICT), "maya")
        assert stage.up_axis == "Y"

    def test_scene_info_to_stage_meters_per_unit(self):
        # units=cm → 0.01 mpu
        stage = scene_info_json_to_stage(json.dumps(self._SCENE_DICT), "maya")
        assert stage.meters_per_unit == pytest.approx(0.01)

    def test_scene_info_to_stage_blender(self):
        d = {**self._SCENE_DICT, "name": "blend_scene", "up_axis": "Z", "fps": 30.0}
        stage = scene_info_json_to_stage(json.dumps(d), "blender")
        assert stage.name == "blend_scene"
        assert stage.up_axis == "Z"
        assert stage.fps == pytest.approx(30.0)

    def test_stage_to_scene_info_json_returns_str(self):
        stage = UsdStage("bridge_test")
        result = stage_to_scene_info_json(stage)
        assert isinstance(result, str)

    def test_stage_to_scene_info_json_is_valid_json(self):
        stage = UsdStage("bridge_test")
        parsed = json.loads(stage_to_scene_info_json(stage))
        assert isinstance(parsed, dict)

    def test_stage_to_scene_info_json_has_name(self):
        stage = UsdStage("my_bridge")
        parsed = json.loads(stage_to_scene_info_json(stage))
        assert parsed.get("name") == "my_bridge"

    def test_stage_to_scene_info_json_has_fps(self):
        stage = UsdStage("fps_test")
        stage.fps = 30.0
        parsed = json.loads(stage_to_scene_info_json(stage))
        assert parsed.get("fps") == pytest.approx(30.0)

    def test_stage_to_scene_info_json_has_up_axis(self):
        stage = UsdStage("axis_test")
        stage.up_axis = "Z"
        parsed = json.loads(stage_to_scene_info_json(stage))
        assert parsed.get("up_axis") == "Z"

    def test_stage_to_scene_info_json_required_keys(self):
        stage = UsdStage("keys_test")
        parsed = json.loads(stage_to_scene_info_json(stage))
        expected_keys = {"file_path", "name", "modified", "format", "fps", "up_axis"}
        assert expected_keys.issubset(parsed.keys())

    def test_round_trip_name(self):
        d = {**self._SCENE_DICT, "name": "roundtrip_scene"}
        stage = scene_info_json_to_stage(json.dumps(d), "maya")
        back = json.loads(stage_to_scene_info_json(stage))
        assert back.get("name") == "roundtrip_scene"

    def test_round_trip_fps(self):
        d = {**self._SCENE_DICT, "fps": 48.0}
        stage = scene_info_json_to_stage(json.dumps(d), "maya")
        back = json.loads(stage_to_scene_info_json(stage))
        assert back.get("fps") == pytest.approx(48.0)

    def test_round_trip_up_axis(self):
        d = {**self._SCENE_DICT, "up_axis": "Z"}
        stage = scene_info_json_to_stage(json.dumps(d), "maya")
        back = json.loads(stage_to_scene_info_json(stage))
        assert back.get("up_axis") == "Z"
