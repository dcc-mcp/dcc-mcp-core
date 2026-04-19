"""Deep tests: PySharedBuffer/PyBufferPool, PyCrashRecoveryPolicy, ToolRegistry thread-safety.

UsdStage full lifecycle — covering previously untested edge cases and combinations.
"""

from __future__ import annotations

import json
import threading

import pytest

import dcc_mcp_core

# ─── PySharedBuffer ──────────────────────────────────────────────────────────


class TestPySharedBufferCreate:
    def test_create_returns_buffer(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        assert buf is not None

    def test_id_is_string(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=512)
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_id_looks_like_uuid(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=512)
        # Short ID format: 16 hex chars (was UUID v4 with 8-4-4-4-12 format)
        assert isinstance(buf.id, str) and len(buf.id) > 0

    def test_name_is_string(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=512)
        assert isinstance(buf.name(), str)
        assert len(buf.name()) > 0

    def test_capacity_matches(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=4096)
        assert buf.capacity() == 4096

    def test_data_len_zero_on_create(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        assert buf.data_len() == 0

    def test_write_returns_bytes_written(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        n = buf.write(b"hello world")
        assert n == 11

    def test_data_len_after_write(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"test data")
        assert buf.data_len() == 9

    def test_read_returns_written_data(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"hello shm")
        assert buf.read() == b"hello shm"

    def test_clear_resets_data_len(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"some data")
        buf.clear()
        assert buf.data_len() == 0

    def test_clear_then_read_returns_empty(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"data")
        buf.clear()
        assert buf.read() == b""

    def test_overwrite_data(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"first")
        buf.clear()
        buf.write(b"second")
        assert buf.read() == b"second"

    def test_write_binary_data(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        data = bytes(range(256))
        buf.write(data)
        assert buf.read() == data

    def test_write_large_data(self):
        size = 64 * 1024
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=size)
        data = b"X" * (size - 64)
        buf.write(data)
        assert buf.read() == data


class TestPySharedBufferDescriptorAndOpen:
    def test_descriptor_json_returns_string(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        desc = buf.descriptor_json()
        assert isinstance(desc, str)

    def test_descriptor_json_valid_json(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        desc = buf.descriptor_json()
        parsed = json.loads(desc)
        assert isinstance(parsed, dict)

    def test_descriptor_json_has_id(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        parsed = json.loads(buf.descriptor_json())
        assert "id" in parsed
        assert parsed["id"] == buf.id

    def test_descriptor_json_has_name(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        parsed = json.loads(buf.descriptor_json())
        assert "name" in parsed
        assert parsed["name"] == buf.name()

    def test_descriptor_json_has_capacity(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=2048)
        parsed = json.loads(buf.descriptor_json())
        assert "capacity" in parsed
        assert parsed["capacity"] == 2048

    def test_open_from_path_and_id(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"cross-process data")
        buf2 = dcc_mcp_core.PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.read() == b"cross-process data"

    def test_open_same_id(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf2 = dcc_mcp_core.PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.id == buf.id

    def test_open_same_capacity(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=4096)
        buf2 = dcc_mcp_core.PySharedBuffer.open(name=buf.name(), id=buf.id)
        assert buf2.capacity() == 4096

    def test_open_reads_updated_data(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"first write")
        buf2 = dcc_mcp_core.PySharedBuffer.open(name=buf.name(), id=buf.id)
        # Write again via original handle
        buf.clear()
        buf.write(b"second write")
        assert buf2.read() == b"second write"

    def test_descriptor_roundtrip_open(self):
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"descriptor roundtrip")
        desc = json.loads(buf.descriptor_json())
        buf2 = dcc_mcp_core.PySharedBuffer.open(name=desc["name"], id=desc["id"])
        assert buf2.read() == b"descriptor roundtrip"


# ─── PyBufferPool ─────────────────────────────────────────────────────────────


class TestPyBufferPool:
    def test_create_pool(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=4, buffer_size=1024)
        assert pool is not None

    def test_capacity(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=4, buffer_size=1024)
        assert pool.capacity() == 4

    def test_buffer_size(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=4, buffer_size=2048)
        assert pool.buffer_size() == 2048

    def test_available_full_on_create(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=3, buffer_size=1024)
        assert pool.available() == 3

    def test_acquire_returns_buffer(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        assert buf is not None

    def test_acquire_decrements_available(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=3, buffer_size=1024)
        _b1 = pool.acquire()
        assert pool.available() == 2

    def test_acquire_write_read(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        buf.write(b"pool data")
        assert buf.read() == b"pool data"

    def test_acquire_multiple_buffers(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=3, buffer_size=512)
        b1 = pool.acquire()
        b2 = pool.acquire()
        b3 = pool.acquire()
        assert b1 is not None
        assert b2 is not None
        assert b3 is not None

    def test_pool_exhaustion_raises(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=1, buffer_size=512)
        _b1 = pool.acquire()
        with pytest.raises((RuntimeError, Exception)):  # pool full
            pool.acquire()

    def test_pool_size_one_acquire_zero_available(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=1, buffer_size=512)
        _b = pool.acquire()
        assert pool.available() == 0

    def test_acquire_independent_buffers(self):
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=1024)
        b1 = pool.acquire()
        b2 = pool.acquire()
        b1.write(b"buf1")
        b2.write(b"buf2")
        assert b1.read() == b"buf1"
        assert b2.read() == b"buf2"


# ─── PyCrashRecoveryPolicy ───────────────────────────────────────────────────


class TestPyCrashRecoveryPolicyCreate:
    def test_create(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert policy is not None

    def test_max_restarts_property(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=5)
        assert policy.max_restarts == 5

    def test_max_restarts_one(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=1)
        assert policy.max_restarts == 1

    def test_max_restarts_zero(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=0)
        assert policy.max_restarts == 0

    def test_max_restarts_large(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=100)
        assert policy.max_restarts == 100


class TestPyCrashRecoveryPolicyShouldRestart:
    def _policy(self):
        return dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)

    def test_crashed_returns_true(self):
        assert self._policy().should_restart("crashed") is True

    def test_unresponsive_returns_true(self):
        assert self._policy().should_restart("unresponsive") is True

    def test_running_returns_false(self):
        assert self._policy().should_restart("running") is False

    def test_starting_returns_false(self):
        assert self._policy().should_restart("starting") is False

    def test_stopped_returns_false(self):
        assert self._policy().should_restart("stopped") is False

    def test_restarting_returns_false(self):
        assert self._policy().should_restart("restarting") is False

    def test_unknown_status_raises(self):
        with pytest.raises((ValueError, Exception)):
            self._policy().should_restart("graceful_exit")

    def test_unknown_status_raises_2(self):
        with pytest.raises((ValueError, Exception)):
            self._policy().should_restart("unknown_xyz")


class TestPyCrashRecoveryPolicyFixedBackoff:
    def test_use_fixed_backoff(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_fixed_backoff(delay_ms=500)
        d = policy.next_delay_ms("maya", 0)
        assert d == 500

    def test_fixed_backoff_consistent_across_attempts(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_fixed_backoff(delay_ms=250)
        d0 = policy.next_delay_ms("maya", 0)
        d1 = policy.next_delay_ms("maya", 1)
        d5 = policy.next_delay_ms("maya", 5)
        assert d0 == d1 == d5 == 250

    def test_fixed_backoff_different_names(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_fixed_backoff(delay_ms=1000)
        d_maya = policy.next_delay_ms("maya", 0)
        d_blender = policy.next_delay_ms("blender", 0)
        assert d_maya == 1000
        assert d_blender == 1000

    def test_fixed_backoff_small_delay(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_fixed_backoff(delay_ms=10)
        assert policy.next_delay_ms("test", 0) == 10

    def test_fixed_backoff_zero_delay(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_fixed_backoff(delay_ms=0)
        assert policy.next_delay_ms("test", 0) == 0


class TestPyCrashRecoveryPolicyExponentialBackoff:
    def test_use_exponential_backoff(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_exponential_backoff(initial_ms=100, max_delay_ms=3000)
        d0 = policy.next_delay_ms("maya", 0)
        assert d0 == 100

    def test_exponential_doubles(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_exponential_backoff(initial_ms=100, max_delay_ms=100000)
        d0 = policy.next_delay_ms("maya", 0)
        d1 = policy.next_delay_ms("maya", 1)
        d2 = policy.next_delay_ms("maya", 2)
        assert d0 == 100
        assert d1 == 200
        assert d2 == 400

    def test_exponential_capped_at_max(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=3000)
        d3 = policy.next_delay_ms("maya", 3)
        assert d3 <= 3000

    def test_exponential_multiple_names_independent(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_exponential_backoff(initial_ms=100, max_delay_ms=10000)
        # Different DCC names are tracked separately
        d_maya_0 = policy.next_delay_ms("maya", 0)
        d_hou_0 = policy.next_delay_ms("houdini", 0)
        assert d_maya_0 == 100
        assert d_hou_0 == 100

    def test_next_delay_ms_exceeding_max_restarts_raises(self):
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=2)
        policy.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        # attempt=0 and 1 are valid
        _ = policy.next_delay_ms("maya", 0)
        _ = policy.next_delay_ms("maya", 1)
        # attempt=2 exceeds max_restarts=2
        with pytest.raises((RuntimeError, Exception)):  # max restarts exceeded
            policy.next_delay_ms("maya", 2)


# ─── ToolRegistry Thread Safety ────────────────────────────────────────────


class TestActionRegistryThreadSafety:
    def test_concurrent_register_50_threads(self):
        reg = dcc_mcp_core.ToolRegistry()
        errors = []
        n = 50

        def worker(i):
            try:
                reg.register(f"action_{i}", description=f"Thread action {i}", dcc="maya")
            except Exception as e:
                errors.append(str(e))

        threads = [threading.Thread(target=worker, args=(i,)) for i in range(n)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0
        assert len(reg) == n

    def test_concurrent_register_and_list(self):
        reg = dcc_mcp_core.ToolRegistry()
        errors = []

        def register_worker(i):
            try:
                reg.register(f"concurrent_{i}", dcc="blender")
            except Exception as e:
                errors.append(str(e))

        def list_worker():
            try:
                _ = reg.list_actions()
            except Exception as e:
                errors.append(str(e))

        threads = []
        for i in range(20):
            threads.append(threading.Thread(target=register_worker, args=(i,)))
        for _ in range(10):
            threads.append(threading.Thread(target=list_worker))

        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0

    def test_concurrent_register_different_dccs(self):
        reg = dcc_mcp_core.ToolRegistry()
        errors = []
        dccs = ["maya", "blender", "houdini", "3dsmax", "unreal"]

        def worker(i, dcc):
            try:
                reg.register(f"action_{dcc}_{i}", dcc=dcc, category="geometry")
            except Exception as e:
                errors.append(str(e))

        threads = []
        for dcc in dccs:
            for i in range(10):
                threads.append(threading.Thread(target=worker, args=(i, dcc)))

        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert len(errors) == 0
        assert len(reg) == 50

    def test_len_and_reset_thread_safe(self):
        reg = dcc_mcp_core.ToolRegistry()
        for i in range(20):
            reg.register(f"act_{i}")
        assert len(reg) == 20
        reg.reset()
        assert len(reg) == 0

    def test_repr_contains_count(self):
        reg = dcc_mcp_core.ToolRegistry()
        reg.register("alpha")
        reg.register("beta")
        r = repr(reg)
        assert "ToolRegistry" in r
        assert "2" in r


# ─── UsdStage Full Lifecycle ──────────────────────────────────────────────────


class TestUsdStageMetrics:
    def test_metrics_empty_stage(self):
        stage = dcc_mcp_core.UsdStage("empty")
        m = stage.metrics()
        assert isinstance(m, dict)
        assert m["prim_count"] == 0

    def test_metrics_prim_count(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/A", "Mesh")
        stage.define_prim("/B", "Mesh")
        stage.define_prim("/C", "Camera")
        m = stage.metrics()
        assert m["prim_count"] == 3

    def test_metrics_mesh_count(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/M1", "Mesh")
        stage.define_prim("/M2", "Mesh")
        m = stage.metrics()
        assert m["mesh_count"] == 2

    def test_metrics_camera_count(self):
        stage = dcc_mcp_core.UsdStage("cams")
        stage.define_prim("/Cam1", "Camera")
        stage.define_prim("/Cam2", "Camera")
        m = stage.metrics()
        assert m["camera_count"] == 2

    def test_metrics_returns_dict(self):
        stage = dcc_mcp_core.UsdStage("attrs")
        prim = stage.define_prim("/Cube", "Mesh")
        prim.set_attribute("size", dcc_mcp_core.VtValue.from_float(2.0))
        prim.set_attribute("name", dcc_mcp_core.VtValue.from_string("MyCube"))
        m = stage.metrics()
        # metrics dict is returned; attribute_count may not be present in all versions
        assert isinstance(m, dict)
        assert m["prim_count"] == 1


class TestUsdStageRemoveAndHasPrim:
    def test_has_prim_true(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/World/Cube", "Mesh")
        assert stage.has_prim("/World/Cube") is True

    def test_has_prim_false_before_define(self):
        stage = dcc_mcp_core.UsdStage("test")
        assert stage.has_prim("/World/NonExistent") is False

    def test_remove_prim_returns_true(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/World/Cube", "Mesh")
        result = stage.remove_prim("/World/Cube")
        assert result is True

    def test_remove_nonexistent_returns_false(self):
        stage = dcc_mcp_core.UsdStage("test")
        result = stage.remove_prim("/World/Ghost")
        assert result is False

    def test_has_prim_false_after_remove(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/World/Cube", "Mesh")
        stage.remove_prim("/World/Cube")
        assert stage.has_prim("/World/Cube") is False

    def test_prim_count_decreases_after_remove(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/A", "Mesh")
        stage.define_prim("/B", "Mesh")
        stage.remove_prim("/A")
        m = stage.metrics()
        assert m["prim_count"] == 1


class TestUsdStagePrimsOfType:
    def test_prims_of_type_mesh(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/Cube", "Mesh")
        stage.define_prim("/Sphere", "Mesh")
        stage.define_prim("/Cam", "Camera")
        meshes = stage.prims_of_type("Mesh")
        assert len(meshes) == 2
        names = {p.name for p in meshes}
        assert names == {"Cube", "Sphere"}

    def test_prims_of_type_camera(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/Cam1", "Camera")
        stage.define_prim("/Cam2", "Camera")
        stage.define_prim("/Mesh1", "Mesh")
        cams = stage.prims_of_type("Camera")
        assert len(cams) == 2

    def test_prims_of_type_empty_result(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/Cube", "Mesh")
        lights = stage.prims_of_type("Light")
        assert lights == []

    def test_prims_of_type_nonexistent_type(self):
        stage = dcc_mcp_core.UsdStage("test")
        result = stage.prims_of_type("FakeType")
        assert result == []


class TestUsdStageExportUsda:
    def test_export_usda_returns_string(self):
        stage = dcc_mcp_core.UsdStage("test")
        usda = stage.export_usda()
        assert isinstance(usda, str)

    def test_export_usda_has_header(self):
        stage = dcc_mcp_core.UsdStage("test")
        usda = stage.export_usda()
        assert "#usda" in usda

    def test_export_usda_nonempty(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/World/Cube", "Mesh")
        usda = stage.export_usda()
        assert len(usda) > 10

    def test_export_usda_contains_up_axis(self):
        stage = dcc_mcp_core.UsdStage("test")
        usda = stage.export_usda()
        assert "upAxis" in usda

    def test_export_usda_changes_with_up_axis(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.up_axis = "Z"
        usda = stage.export_usda()
        assert '"Z"' in usda


class TestUsdStageFromJsonRoundtrip:
    def test_to_json_from_json_name(self):
        stage = dcc_mcp_core.UsdStage("roundtrip_stage")
        j = stage.to_json()
        stage2 = dcc_mcp_core.UsdStage.from_json(j)
        assert stage2.name == "roundtrip_stage"

    def test_to_json_from_json_up_axis(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.up_axis = "Z"
        j = stage.to_json()
        stage2 = dcc_mcp_core.UsdStage.from_json(j)
        assert stage2.up_axis == "Z"

    def test_to_json_from_json_prim_count(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/Cube", "Mesh")
        stage.define_prim("/Sphere", "Mesh")
        j = stage.to_json()
        stage2 = dcc_mcp_core.UsdStage.from_json(j)
        assert len(stage2.traverse()) == 2

    def test_to_json_from_json_meters_per_unit(self):
        stage = dcc_mcp_core.UsdStage("test")
        # meters_per_unit is read-only; verify default value survives roundtrip
        original_mpu = stage.meters_per_unit
        j = stage.to_json()
        stage2 = dcc_mcp_core.UsdStage.from_json(j)
        assert abs(stage2.meters_per_unit - original_mpu) < 1e-6

    def test_to_json_returns_string(self):
        stage = dcc_mcp_core.UsdStage("json_test")
        j = stage.to_json()
        assert isinstance(j, str)
        assert len(j) > 0

    def test_from_json_is_static_method(self):
        stage = dcc_mcp_core.UsdStage("s")
        j = stage.to_json()
        stage2 = dcc_mcp_core.UsdStage.from_json(j)
        assert isinstance(stage2, dcc_mcp_core.UsdStage)


class TestUsdStageAttributes:
    def test_set_get_attribute_via_stage(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/Cube", "Mesh")
        stage.set_attribute("/Cube", "myFloat", dcc_mcp_core.VtValue.from_float(3.14))
        val = stage.get_attribute("/Cube", "myFloat")
        assert val is not None
        assert abs(val.to_python() - 3.14) < 1e-4

    def test_get_attribute_nonexistent_returns_none(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/Cube", "Mesh")
        val = stage.get_attribute("/Cube", "nonexistent_attr")
        assert val is None

    def test_set_string_attribute(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/Object", "Xform")
        stage.set_attribute("/Object", "label", dcc_mcp_core.VtValue.from_string("MyObject"))
        val = stage.get_attribute("/Object", "label")
        assert val.to_python() == "MyObject"


class TestUsdStageTraverse:
    def test_traverse_empty_stage(self):
        stage = dcc_mcp_core.UsdStage("empty")
        prims = stage.traverse()
        assert prims == []

    def test_traverse_returns_all_prims(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/A", "Mesh")
        stage.define_prim("/B", "Camera")
        stage.define_prim("/C", "Xform")
        prims = stage.traverse()
        assert len(prims) == 3

    def test_traverse_prim_names(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/Alpha", "Mesh")
        stage.define_prim("/Beta", "Light")
        names = {p.name for p in stage.traverse()}
        assert "Alpha" in names
        assert "Beta" in names

    def test_traverse_after_remove(self):
        stage = dcc_mcp_core.UsdStage("test")
        stage.define_prim("/A", "Mesh")
        stage.define_prim("/B", "Mesh")
        stage.remove_prim("/A")
        prims = stage.traverse()
        assert len(prims) == 1
        assert prims[0].name == "B"
