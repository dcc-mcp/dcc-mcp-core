"""Tests for VtValue all types, UsdStage.traverse/prims_of_type, ToolRegistry.count_actions/reset.

ServiceEntry full attribute set, TimingMiddleware.last_elapsed_ms precision,
PyBufferPool acquire/release lifecycle, PySharedSceneBuffer write/read/compression.
"""

from __future__ import annotations

import gc
import tempfile

import pytest

from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import PySharedSceneBuffer
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolPipeline
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportManager
from dcc_mcp_core import UsdPrim
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue

# ---------------------------------------------------------------------------
# VtValue — all factory methods + type_name + to_python
# ---------------------------------------------------------------------------


class TestVtValueBool:
    def test_type_name_is_bool(self) -> None:
        v = VtValue.from_bool(True)
        assert v.type_name == "bool"

    def test_type_name_false_is_bool(self) -> None:
        v = VtValue.from_bool(False)
        assert v.type_name == "bool"

    def test_to_python_true(self) -> None:
        v = VtValue.from_bool(True)
        assert v.to_python() is True

    def test_to_python_false(self) -> None:
        v = VtValue.from_bool(False)
        assert v.to_python() is False

    def test_to_python_returns_bool_type(self) -> None:
        v = VtValue.from_bool(True)
        assert isinstance(v.to_python(), bool)

    def test_repr_contains_bool(self) -> None:
        v = VtValue.from_bool(True)
        r = repr(v)
        assert "bool" in r

    def test_repr_is_str(self) -> None:
        v = VtValue.from_bool(True)
        assert isinstance(repr(v), str)


class TestVtValueInt:
    def test_type_name_is_int(self) -> None:
        v = VtValue.from_int(42)
        assert v.type_name == "int"

    def test_to_python_positive(self) -> None:
        v = VtValue.from_int(42)
        assert v.to_python() == 42

    def test_to_python_zero(self) -> None:
        v = VtValue.from_int(0)
        assert v.to_python() == 0

    def test_to_python_negative(self) -> None:
        v = VtValue.from_int(-7)
        assert v.to_python() == -7

    def test_to_python_returns_int_type(self) -> None:
        v = VtValue.from_int(1)
        assert isinstance(v.to_python(), int)

    def test_repr_contains_int(self) -> None:
        v = VtValue.from_int(42)
        assert "int" in repr(v).lower() or "42" in repr(v)


class TestVtValueFloat:
    def test_type_name_is_float(self) -> None:
        v = VtValue.from_float(3.14)
        assert v.type_name == "float"

    def test_to_python_approx(self) -> None:
        v = VtValue.from_float(3.14)
        result = v.to_python()
        assert isinstance(result, float)
        assert abs(result - 3.14) < 0.001

    def test_to_python_zero(self) -> None:
        v = VtValue.from_float(0.0)
        assert v.to_python() == 0.0

    def test_to_python_negative(self) -> None:
        v = VtValue.from_float(-1.5)
        result = v.to_python()
        assert isinstance(result, float)
        assert result < 0

    def test_repr_is_str(self) -> None:
        v = VtValue.from_float(1.0)
        assert isinstance(repr(v), str)


class TestVtValueString:
    def test_type_name_is_string(self) -> None:
        v = VtValue.from_string("hello")
        assert v.type_name == "string"

    def test_to_python_roundtrip(self) -> None:
        v = VtValue.from_string("hello world")
        assert v.to_python() == "hello world"

    def test_to_python_empty_string(self) -> None:
        v = VtValue.from_string("")
        assert v.to_python() == ""

    def test_to_python_returns_str_type(self) -> None:
        v = VtValue.from_string("test")
        assert isinstance(v.to_python(), str)

    def test_unicode_roundtrip(self) -> None:
        v = VtValue.from_string("日本語テスト")
        assert v.to_python() == "日本語テスト"


class TestVtValueToken:
    def test_type_name_is_token(self) -> None:
        v = VtValue.from_token("myToken")
        assert v.type_name == "token"

    def test_to_python_returns_str(self) -> None:
        v = VtValue.from_token("myToken")
        assert isinstance(v.to_python(), str)

    def test_to_python_value(self) -> None:
        v = VtValue.from_token("render_preview")
        assert v.to_python() == "render_preview"

    def test_repr_is_str(self) -> None:
        v = VtValue.from_token("tok")
        assert isinstance(repr(v), str)


class TestVtValueAsset:
    def test_type_name_is_asset(self) -> None:
        v = VtValue.from_asset("path/to/file.usd")
        assert v.type_name == "asset"

    def test_to_python_returns_str(self) -> None:
        v = VtValue.from_asset("path/to/file.usd")
        assert isinstance(v.to_python(), str)

    def test_to_python_value(self) -> None:
        v = VtValue.from_asset("scene.usd")
        assert v.to_python() == "scene.usd"

    def test_repr_is_str(self) -> None:
        v = VtValue.from_asset("f.usd")
        assert isinstance(repr(v), str)


class TestVtValueVec3f:
    def test_type_name_is_float3(self) -> None:
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert v.type_name == "float3"

    def test_to_python_returns_tuple(self) -> None:
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        result = v.to_python()
        assert isinstance(result, tuple)

    def test_to_python_length_3(self) -> None:
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert len(v.to_python()) == 3

    def test_to_python_values(self) -> None:
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        t = v.to_python()
        assert abs(t[0] - 1.0) < 1e-6
        assert abs(t[1] - 2.0) < 1e-6
        assert abs(t[2] - 3.0) < 1e-6

    def test_to_python_zero_vector(self) -> None:
        v = VtValue.from_vec3f(0.0, 0.0, 0.0)
        t = v.to_python()
        assert t == (0.0, 0.0, 0.0)

    def test_repr_contains_vec3f_or_float3(self) -> None:
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        r = repr(v)
        assert "float3" in r or "Vec3f" in r

    def test_repr_is_str(self) -> None:
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert isinstance(repr(v), str)


# ---------------------------------------------------------------------------
# UsdStage.traverse + prims_of_type
# ---------------------------------------------------------------------------


class TestUsdStageTraverse:
    def test_traverse_returns_list(self) -> None:
        stage = UsdStage("test_traverse_list")
        result = stage.traverse()
        assert isinstance(result, list)

    def test_empty_stage_traverse_empty(self) -> None:
        stage = UsdStage("empty_traverse")
        result = stage.traverse()
        assert isinstance(result, list)
        assert len(result) == 0

    def test_traverse_returns_usdprim_items(self) -> None:
        stage = UsdStage("test_traverse_types")
        stage.define_prim("/World", "Xform")
        result = stage.traverse()
        assert len(result) >= 1
        for p in result:
            assert isinstance(p, UsdPrim)

    def test_traverse_includes_all_prims(self) -> None:
        stage = UsdStage("test_traverse_all")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        stage.define_prim("/World/Sphere", "Mesh")
        result = stage.traverse()
        paths = {str(p.path) for p in result}
        assert "/World" in paths
        assert "/World/Cube" in paths
        assert "/World/Sphere" in paths

    def test_traverse_count_matches_define_count(self) -> None:
        stage = UsdStage("test_traverse_count")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/A/B", "Mesh")
        stage.define_prim("/A/C", "Camera")
        result = stage.traverse()
        assert len(result) == 3

    def test_traverse_after_remove_prim(self) -> None:
        stage = UsdStage("test_traverse_remove")
        stage.define_prim("/X", "Xform")
        stage.define_prim("/X/Y", "Mesh")
        stage.remove_prim("/X/Y")
        result = stage.traverse()
        paths = {str(p.path) for p in result}
        assert "/X/Y" not in paths
        assert "/X" in paths

    def test_prims_of_type_returns_list(self) -> None:
        stage = UsdStage("test_pot_list")
        result = stage.prims_of_type("Mesh")
        assert isinstance(result, list)

    def test_prims_of_type_empty_stage(self) -> None:
        stage = UsdStage("test_pot_empty")
        result = stage.prims_of_type("Mesh")
        assert result == []

    def test_prims_of_type_single(self) -> None:
        stage = UsdStage("test_pot_single")
        stage.define_prim("/Cube", "Mesh")
        result = stage.prims_of_type("Mesh")
        assert len(result) == 1

    def test_prims_of_type_multiple(self) -> None:
        stage = UsdStage("test_pot_multi")
        stage.define_prim("/A", "Mesh")
        stage.define_prim("/B", "Mesh")
        stage.define_prim("/C", "Camera")
        meshes = stage.prims_of_type("Mesh")
        assert len(meshes) == 2

    def test_prims_of_type_no_match(self) -> None:
        stage = UsdStage("test_pot_nomatch")
        stage.define_prim("/A", "Mesh")
        result = stage.prims_of_type("NonExistentType")
        assert result == []

    def test_prims_of_type_items_are_usdprim(self) -> None:
        stage = UsdStage("test_pot_items")
        stage.define_prim("/A", "Mesh")
        result = stage.prims_of_type("Mesh")
        for p in result:
            assert isinstance(p, UsdPrim)

    def test_prims_of_type_correct_types(self) -> None:
        stage = UsdStage("test_pot_correct_type")
        stage.define_prim("/Mesh1", "Mesh")
        stage.define_prim("/Cam1", "Camera")
        for p in stage.prims_of_type("Mesh"):
            assert p.type_name == "Mesh"
        for p in stage.prims_of_type("Camera"):
            assert p.type_name == "Camera"

    def test_traverse_prim_path_is_sdfpath(self) -> None:
        from dcc_mcp_core import SdfPath

        stage = UsdStage("test_traverse_path_type")
        stage.define_prim("/Root", "Xform")
        prims = stage.traverse()
        assert len(prims) >= 1
        assert isinstance(prims[0].path, SdfPath)


# ---------------------------------------------------------------------------
# ToolRegistry.count_actions + reset
# ---------------------------------------------------------------------------


class TestActionRegistryCountAndReset:
    def test_count_initial_zero(self) -> None:
        reg = ToolRegistry()
        assert reg.count_actions() == 0

    def test_count_after_one_register(self) -> None:
        reg = ToolRegistry()
        reg.register("act1")
        assert reg.count_actions() == 1

    def test_count_after_multiple_registers(self) -> None:
        reg = ToolRegistry()
        reg.register("a1", category="geo")
        reg.register("a2", category="geo")
        reg.register("a3", category="anim")
        assert reg.count_actions() == 3

    def test_count_by_category(self) -> None:
        reg = ToolRegistry()
        reg.register("a1", category="geo")
        reg.register("a2", category="geo")
        reg.register("a3", category="anim")
        assert reg.count_actions(category="geo") == 2
        assert reg.count_actions(category="anim") == 1

    def test_count_zero_for_nonexistent_category(self) -> None:
        reg = ToolRegistry()
        reg.register("a1", category="geo")
        assert reg.count_actions(category="export") == 0

    def test_count_by_dcc_name(self) -> None:
        reg = ToolRegistry()
        reg.register("a1", dcc="maya")
        reg.register("a2", dcc="maya")
        reg.register("a3", dcc="blender")
        assert reg.count_actions(dcc_name="maya") == 2
        assert reg.count_actions(dcc_name="blender") == 1

    def test_count_returns_int(self) -> None:
        reg = ToolRegistry()
        reg.register("act")
        assert isinstance(reg.count_actions(), int)

    def test_count_after_batch_register(self) -> None:
        reg = ToolRegistry()
        reg.register_batch(
            [
                {"name": "a1", "category": "geo"},
                {"name": "a2", "category": "geo"},
                {"name": "a3", "category": "anim"},
            ]
        )
        assert reg.count_actions() == 3

    def test_count_after_unregister(self) -> None:
        reg = ToolRegistry()
        reg.register("a1")
        reg.register("a2")
        reg.unregister("a1")
        assert reg.count_actions() == 1

    def test_reset_clears_all(self) -> None:
        reg = ToolRegistry()
        reg.register("a1")
        reg.register("a2")
        reg.reset()
        assert reg.count_actions() == 0

    def test_reset_clears_list_actions(self) -> None:
        reg = ToolRegistry()
        reg.register("a1")
        reg.reset()
        assert reg.list_actions() == []

    def test_reset_allows_re_register(self) -> None:
        reg = ToolRegistry()
        reg.register("a1")
        reg.reset()
        reg.register("a1")
        assert reg.count_actions() == 1

    def test_count_filter_tags(self) -> None:
        reg = ToolRegistry()
        reg.register("a1", tags=["create", "mesh"])
        reg.register("a2", tags=["delete"])
        assert reg.count_actions(tags=["create"]) == 1
        assert reg.count_actions(tags=["delete"]) == 1
        assert reg.count_actions(tags=["create", "mesh"]) == 1


# ---------------------------------------------------------------------------
# ServiceEntry full attribute set
# ---------------------------------------------------------------------------


class TestServiceEntryAttributes:
    def _make_entry(self, **kwargs):
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812, **kwargs)
            entry = mgr.get_service("maya", iid)
            # Return a snapshot dict and instance_id to avoid use-after-free
            return entry.to_dict(), iid, entry

    def test_entry_dcc_type(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("blender", "127.0.0.1", 19000)
            entry = mgr.get_service("blender", iid)
            assert entry.dcc_type == "blender"

    def test_entry_instance_id_is_str(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert isinstance(entry.instance_id, str)
            assert entry.instance_id == iid

    def test_entry_host(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert entry.host == "127.0.0.1"

    def test_entry_port(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert entry.port == 18812

    def test_entry_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812, version="2025")
            entry = mgr.get_service("maya", iid)
            assert entry.version == "2025"

    def test_entry_version_none(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert entry.version is None

    def test_entry_scene(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812, scene="scene.ma")
            entry = mgr.get_service("maya", iid)
            assert entry.scene == "scene.ma"

    def test_entry_scene_none(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert entry.scene is None

    def test_entry_metadata_empty(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert isinstance(entry.metadata, dict)
            assert entry.metadata == {}

    def test_entry_status_available(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert entry.status == ServiceStatus.AVAILABLE

    def test_entry_is_ipc_false_no_transport_addr(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert entry.is_ipc is False

    def test_entry_last_heartbeat_ms_positive(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert isinstance(entry.last_heartbeat_ms, int)
            assert entry.last_heartbeat_ms > 0

    def test_entry_effective_address_tcp(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            from dcc_mcp_core import TransportAddress

            addr = entry.effective_address()
            assert isinstance(addr, TransportAddress)

    def test_entry_effective_address_is_tcp(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            addr = entry.effective_address()
            assert addr.is_tcp

    def test_entry_to_dict_keys(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            d = entry.to_dict()
            expected_keys = {
                "dcc_type",
                "display_name",
                "documents",
                "extras",
                "host",
                "instance_id",
                "last_heartbeat_ms",
                "metadata",
                "pid",
                "port",
                "scene",
                "status",
                "version",
            }
            assert set(d.keys()) == expected_keys

    def test_entry_to_dict_dcc_type(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("houdini", "127.0.0.1", 20000)
            entry = mgr.get_service("houdini", iid)
            assert entry.to_dict()["dcc_type"] == "houdini"

    def test_entry_repr_is_str(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            r = repr(entry)
            assert isinstance(r, str)

    def test_entry_repr_contains_dcc_type(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert "maya" in repr(entry)

    def test_entry_status_after_update(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            mgr = TransportManager(tmpdir)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            mgr.update_service_status("maya", iid, ServiceStatus.BUSY)
            entry = mgr.get_service("maya", iid)
            assert entry.status == ServiceStatus.BUSY


# ---------------------------------------------------------------------------
# TimingMiddleware.last_elapsed_ms precision
# ---------------------------------------------------------------------------


class TestTimingMiddlewarePrecision:
    def _make_pipeline(self, handler=None):
        reg = ToolRegistry()
        reg.register("fast_action", category="util")
        disp = ToolDispatcher(reg)
        disp.register_handler("fast_action", handler or (lambda params: "ok"))
        pipe = ToolPipeline(disp)
        timing = pipe.add_timing()
        return pipe, timing

    def test_unknown_action_returns_none(self) -> None:
        _pipe, timing = self._make_pipeline()
        assert timing.last_elapsed_ms("nonexistent") is None

    def test_before_dispatch_returns_none(self) -> None:
        _pipe, timing = self._make_pipeline()
        assert timing.last_elapsed_ms("fast_action") is None

    def test_after_dispatch_returns_int(self) -> None:
        pipe, timing = self._make_pipeline()
        pipe.dispatch("fast_action", "{}")
        result = timing.last_elapsed_ms("fast_action")
        assert isinstance(result, int)

    def test_after_dispatch_gte_zero(self) -> None:
        pipe, timing = self._make_pipeline()
        pipe.dispatch("fast_action", "{}")
        assert timing.last_elapsed_ms("fast_action") >= 0

    def test_second_dispatch_updates_value(self) -> None:
        pipe, timing = self._make_pipeline()
        pipe.dispatch("fast_action", "{}")
        ms1 = timing.last_elapsed_ms("fast_action")
        pipe.dispatch("fast_action", "{}")
        ms2 = timing.last_elapsed_ms("fast_action")
        # Both should be valid non-negative integers
        assert isinstance(ms1, int)
        assert isinstance(ms2, int)
        assert ms2 >= 0

    def test_multiple_actions_independent(self) -> None:
        reg = ToolRegistry()
        reg.register("action_a", category="util")
        reg.register("action_b", category="util")
        disp = ToolDispatcher(reg)
        disp.register_handler("action_a", lambda p: "a")
        disp.register_handler("action_b", lambda p: "b")
        pipe = ToolPipeline(disp)
        timing = pipe.add_timing()

        pipe.dispatch("action_a", "{}")
        # B not dispatched yet
        assert timing.last_elapsed_ms("action_b") is None
        pipe.dispatch("action_b", "{}")
        # Now both should have values
        assert timing.last_elapsed_ms("action_a") is not None
        assert timing.last_elapsed_ms("action_b") is not None

    def test_repr_is_str(self) -> None:
        _pipe, timing = self._make_pipeline()
        assert isinstance(repr(timing), str)


# ---------------------------------------------------------------------------
# PyBufferPool acquire + release lifecycle
# ---------------------------------------------------------------------------


class TestPyBufferPoolLifecycle:
    def test_capacity_matches_init(self) -> None:
        pool = PyBufferPool(capacity=3, buffer_size=1024)
        assert pool.capacity() == 3

    def test_buffer_size_matches_init(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=4096)
        assert pool.buffer_size() == 4096

    def test_available_initially_equals_capacity(self) -> None:
        pool = PyBufferPool(capacity=4, buffer_size=512)
        assert pool.available() == 4

    def test_acquire_returns_shared_buffer(self) -> None:
        from dcc_mcp_core import PySharedBuffer

        pool = PyBufferPool(capacity=1, buffer_size=1024)
        buf = pool.acquire()
        assert isinstance(buf, PySharedBuffer)

    def test_acquire_decreases_available(self) -> None:
        pool = PyBufferPool(capacity=3, buffer_size=512)
        _buf = pool.acquire()  # keep ref to prevent immediate GC
        assert pool.available() == 2

    def test_acquire_two_decreases_by_two(self) -> None:
        pool = PyBufferPool(capacity=3, buffer_size=512)
        _b1 = pool.acquire()  # keep refs
        _b2 = pool.acquire()
        assert pool.available() == 1

    def test_exhaust_pool_raises_runtime_error(self) -> None:
        pool = PyBufferPool(capacity=1, buffer_size=512)
        _b = pool.acquire()  # keep reference so not GC'd
        with pytest.raises(RuntimeError):
            pool.acquire()

    def test_error_message_contains_capacity(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=512)
        b1 = pool.acquire()
        b2 = pool.acquire()
        with pytest.raises(RuntimeError, match="2"):
            pool.acquire()
        del b1, b2

    def test_release_via_gc_restores_available(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        assert pool.available() == 1
        del buf
        gc.collect()
        assert pool.available() == 2

    def test_acquire_after_gc_succeeds(self) -> None:
        pool = PyBufferPool(capacity=1, buffer_size=512)
        buf = pool.acquire()
        del buf
        gc.collect()
        buf2 = pool.acquire()
        assert buf2 is not None

    def test_buffer_from_pool_has_correct_capacity(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        assert buf.capacity() == 1024

    def test_buffer_from_pool_can_write_and_read(self) -> None:
        pool = PyBufferPool(capacity=1, buffer_size=4096)
        buf = pool.acquire()
        data = b"hello pool"
        buf.write(data)
        assert buf.read() == data

    def test_repr_is_str(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=512)
        assert isinstance(repr(pool), str)


# ---------------------------------------------------------------------------
# PySharedSceneBuffer write/read/compression
# ---------------------------------------------------------------------------


class TestPySharedSceneBuffer:
    def test_write_returns_scene_buffer(self) -> None:
        data = b"scene data" * 10
        ssb = PySharedSceneBuffer.write(data=data)
        assert isinstance(ssb, PySharedSceneBuffer)

    def test_id_is_str(self) -> None:
        data = b"hello" * 20
        ssb = PySharedSceneBuffer.write(data=data)
        assert isinstance(ssb.id, str)

    def test_id_nonempty(self) -> None:
        data = b"hello" * 20
        ssb = PySharedSceneBuffer.write(data=data)
        assert len(ssb.id) > 0

    def test_total_bytes_equals_input_size(self) -> None:
        data = b"x" * 500
        ssb = PySharedSceneBuffer.write(data=data)
        assert ssb.total_bytes == 500

    def test_is_inline_for_small_data(self) -> None:
        data = b"small" * 100
        ssb = PySharedSceneBuffer.write(data=data)
        assert ssb.is_inline is True

    def test_is_chunked_false_for_small_data(self) -> None:
        data = b"small" * 100
        ssb = PySharedSceneBuffer.write(data=data)
        assert ssb.is_chunked is False

    def test_read_roundtrip(self) -> None:
        data = b"vertex data abc" * 50
        ssb = PySharedSceneBuffer.write(data=data)
        assert ssb.read() == data

    def test_descriptor_json_is_str(self) -> None:
        data = b"desc test"
        ssb = PySharedSceneBuffer.write(data=data)
        assert isinstance(ssb.descriptor_json(), str)

    def test_descriptor_json_contains_id(self) -> None:
        data = b"desc test 2"
        ssb = PySharedSceneBuffer.write(data=data)
        assert ssb.id in ssb.descriptor_json()

    def test_kind_geometry(self) -> None:
        data = b"geo data" * 100
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Geometry)
        assert ssb.read() == data

    def test_kind_screenshot(self) -> None:
        data = b"img data" * 100
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Screenshot)
        assert ssb.read() == data

    def test_kind_animation_cache(self) -> None:
        data = b"anim data" * 100
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.AnimationCache)
        assert ssb.read() == data

    def test_source_dcc_accepted(self) -> None:
        data = b"maya data" * 100
        ssb = PySharedSceneBuffer.write(data=data, source_dcc="maya")
        assert ssb.read() == data

    def test_compression_roundtrip(self) -> None:
        data = b"compressible " * 200
        ssb = PySharedSceneBuffer.write(data=data, use_compression=True)
        assert ssb.read() == data

    def test_compression_total_bytes_preserved(self) -> None:
        data = b"test " * 300
        ssb = PySharedSceneBuffer.write(data=data, use_compression=True)
        # total_bytes reflects original size
        assert ssb.total_bytes == len(data)

    def test_no_compression_roundtrip(self) -> None:
        data = b"raw data " * 100
        ssb = PySharedSceneBuffer.write(data=data, use_compression=False)
        assert ssb.read() == data

    def test_repr_is_str(self) -> None:
        data = b"repr data"
        ssb = PySharedSceneBuffer.write(data=data)
        assert isinstance(repr(ssb), str)

    def test_repr_contains_id(self) -> None:
        data = b"repr data 2"
        ssb = PySharedSceneBuffer.write(data=data)
        assert ssb.id in repr(ssb)

    def test_two_writes_have_different_ids(self) -> None:
        data = b"data"
        ssb1 = PySharedSceneBuffer.write(data=data)
        ssb2 = PySharedSceneBuffer.write(data=data)
        assert ssb1.id != ssb2.id
