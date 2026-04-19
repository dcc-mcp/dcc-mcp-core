"""Deep tests for ToolDispatcher, ToolValidator, PyDccLauncher, PyBufferPool/PySharedBuffer, DccInfo/DccCapabilities, and TransportManager.

Each class is grouped into its own TestXxx class. All tests are
pure-Python / mock-based; no real DCC process is spawned.
"""

from __future__ import annotations

import gc
import json
import tempfile
import uuid

import pytest

from dcc_mcp_core import DccCapabilities
from dcc_mcp_core import DccInfo
from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PyDccLauncher
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import PySharedSceneBuffer
from dcc_mcp_core import ScriptLanguage
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import ToolValidator
from dcc_mcp_core import TransportManager

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_registry(*names: str, category: str = "geo", dcc: str = "maya") -> ToolRegistry:
    reg = ToolRegistry()
    for name in names:
        reg.register(name, description=f"desc {name}", category=category, dcc=dcc)
    return reg


def _make_dispatcher(*names: str) -> ToolDispatcher:
    reg = _make_registry(*names)
    return ToolDispatcher(reg)


# ---------------------------------------------------------------------------
# ToolDispatcher
# ---------------------------------------------------------------------------


class TestActionDispatcherConstruction:
    def test_creates_with_registry(self) -> None:
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        assert disp is not None

    def test_repr_contains_class_name(self) -> None:
        disp = _make_dispatcher()
        assert "Dispatcher" in repr(disp) or "dispatcher" in repr(disp).lower()

    def test_initial_handler_count_is_zero(self) -> None:
        disp = _make_dispatcher("sphere")
        assert disp.handler_count() == 0

    def test_initial_handler_names_empty(self) -> None:
        disp = _make_dispatcher("sphere")
        assert disp.handler_names() == []

    def test_skip_empty_schema_validation_default_true(self) -> None:
        disp = _make_dispatcher()
        assert disp.skip_empty_schema_validation is True


class TestActionDispatcherRegisterHandler:
    def test_register_increases_count(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {"ok": True})
        assert disp.handler_count() == 1

    def test_register_appears_in_names(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {})
        assert "sphere" in disp.handler_names()

    def test_has_handler_true_after_register(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {})
        assert disp.has_handler("sphere") is True

    def test_has_handler_false_before_register(self) -> None:
        disp = _make_dispatcher("sphere")
        assert disp.has_handler("sphere") is False

    def test_has_handler_for_unregistered_action(self) -> None:
        disp = _make_dispatcher("sphere")
        assert disp.has_handler("cube") is False

    def test_multiple_handlers(self) -> None:
        disp = _make_dispatcher("a", "b", "c")
        for name in ("a", "b", "c"):
            disp.register_handler(name, lambda p: {})
        assert disp.handler_count() == 3
        assert set(disp.handler_names()) == {"a", "b", "c"}

    def test_register_overwrites_existing(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {"v": 1})
        disp.register_handler("sphere", lambda p: {"v": 2})
        result = disp.dispatch("sphere", "{}")
        assert result["output"]["v"] == 2


class TestActionDispatcherRemoveHandler:
    def test_remove_decreases_count(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {})
        disp.remove_handler("sphere")
        assert disp.handler_count() == 0

    def test_remove_not_in_names(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {})
        disp.remove_handler("sphere")
        assert "sphere" not in disp.handler_names()

    def test_has_handler_false_after_remove(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {})
        disp.remove_handler("sphere")
        assert disp.has_handler("sphere") is False

    def test_remove_nonexistent_is_noop(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.remove_handler("nonexistent")  # should not raise
        assert disp.handler_count() == 0

    def test_partial_remove_leaves_others(self) -> None:
        disp = _make_dispatcher("a", "b")
        disp.register_handler("a", lambda p: {})
        disp.register_handler("b", lambda p: {})
        disp.remove_handler("a")
        assert disp.has_handler("a") is False
        assert disp.has_handler("b") is True


class TestActionDispatcherDispatch:
    def test_dispatch_returns_dict(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {"name": "s1"})
        result = disp.dispatch("sphere", "{}")
        assert isinstance(result, dict)

    def test_dispatch_contains_action_key(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {})
        result = disp.dispatch("sphere", "{}")
        assert result["action"] == "sphere"

    def test_dispatch_contains_output_key(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {"x": 42})
        result = disp.dispatch("sphere", "{}")
        assert result["output"] == {"x": 42}

    def test_dispatch_contains_validation_skipped(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {})
        result = disp.dispatch("sphere", "{}")
        assert "validation_skipped" in result

    def test_dispatch_handler_receives_params(self) -> None:
        received = []
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: received.append(p) or {})
        disp.dispatch("sphere", json.dumps({"radius": 2.5}))
        assert len(received) == 1

    def test_dispatch_no_handler_raises_key_error(self) -> None:
        disp = _make_dispatcher("sphere")
        with pytest.raises(KeyError):
            disp.dispatch("sphere", "{}")

    def test_dispatch_unknown_action_raises(self) -> None:
        disp = _make_dispatcher("sphere")
        with pytest.raises(KeyError):
            disp.dispatch("unknown_action_xyz", "{}")

    def test_dispatch_handler_return_none_ok(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: None)
        result = disp.dispatch("sphere", "{}")
        assert result["output"] is None

    def test_dispatch_after_remove_raises(self) -> None:
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: {})
        disp.remove_handler("sphere")
        with pytest.raises(KeyError):
            disp.dispatch("sphere", "{}")

    def test_dispatch_multiple_calls_same_handler(self) -> None:
        counter = {"n": 0}
        disp = _make_dispatcher("sphere")
        disp.register_handler("sphere", lambda p: counter.update(n=counter["n"] + 1) or {})
        for _ in range(5):
            disp.dispatch("sphere", "{}")
        assert counter["n"] == 5


# ---------------------------------------------------------------------------
# ToolValidator
# ---------------------------------------------------------------------------


class TestActionValidatorFromSchemaJson:
    def test_creates_from_valid_schema(self) -> None:
        schema = json.dumps({"type": "object", "properties": {}})
        v = ToolValidator.from_schema_json(schema)
        assert v is not None

    def test_validate_empty_object_always_valid(self) -> None:
        schema = json.dumps({"type": "object", "properties": {}})
        v = ToolValidator.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert ok is True
        assert errors == []

    def test_validate_required_field_present(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
            }
        )
        v = ToolValidator.from_schema_json(schema)
        ok, _errors = v.validate(json.dumps({"name": "sphere"}))
        assert ok is True

    def test_validate_required_field_missing(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
            }
        )
        v = ToolValidator.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) > 0

    def test_validate_returns_tuple(self) -> None:
        schema = json.dumps({"type": "object"})
        v = ToolValidator.from_schema_json(schema)
        result = v.validate("{}")
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_validate_type_mismatch_returns_errors(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"radius": {"type": "number"}},
                "required": ["radius"],
            }
        )
        v = ToolValidator.from_schema_json(schema)
        ok, errors = v.validate(json.dumps({"radius": "not_a_number"}))
        assert ok is False
        assert any("radius" in e for e in errors)

    def test_validate_number_in_range(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"x": {"type": "number", "minimum": 0, "maximum": 100}},
                "required": ["x"],
            }
        )
        v = ToolValidator.from_schema_json(schema)
        ok, _ = v.validate(json.dumps({"x": 50}))
        assert ok is True

    def test_validate_errors_list_contains_field_name(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"count": {"type": "integer"}},
                "required": ["count"],
            }
        )
        v = ToolValidator.from_schema_json(schema)
        _, errors = v.validate(json.dumps({"count": "abc"}))
        assert any("count" in e for e in errors)

    def test_validate_multiple_required_all_missing(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {
                    "a": {"type": "string"},
                    "b": {"type": "number"},
                },
                "required": ["a", "b"],
            }
        )
        v = ToolValidator.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) >= 2

    def test_validate_multiple_required_one_missing(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {
                    "a": {"type": "string"},
                    "b": {"type": "number"},
                },
                "required": ["a", "b"],
            }
        )
        v = ToolValidator.from_schema_json(schema)
        ok, errors = v.validate(json.dumps({"a": "hello"}))
        assert ok is False
        assert len(errors) == 1

    def test_validator_independence(self) -> None:
        schema1 = json.dumps({"type": "object", "required": ["x"]})
        schema2 = json.dumps({"type": "object", "required": ["y"]})
        v1 = ToolValidator.from_schema_json(schema1)
        v2 = ToolValidator.from_schema_json(schema2)
        ok1, _ = v1.validate(json.dumps({"x": 1}))
        ok2, _ = v2.validate(json.dumps({"y": 1}))
        assert ok1 is True
        assert ok2 is True


class TestActionValidatorFromActionRegistry:
    def test_creates_from_registry(self) -> None:
        reg = ToolRegistry()
        reg.register(
            "sphere",
            description="desc",
            category="geo",
            dcc="maya",
            input_schema=json.dumps({"type": "object", "properties": {}}),
        )
        v = ToolValidator.from_action_registry(reg, "sphere")
        assert v is not None

    def test_validate_against_registered_schema(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"radius": {"type": "number"}},
                "required": ["radius"],
            }
        )
        reg = ToolRegistry()
        reg.register("sphere", description="desc", category="geo", dcc="maya", input_schema=schema)
        v = ToolValidator.from_action_registry(reg, "sphere")
        ok, _ = v.validate(json.dumps({"radius": 1.0}))
        assert ok is True

    def test_validate_missing_required_from_registry(self) -> None:
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"radius": {"type": "number"}},
                "required": ["radius"],
            }
        )
        reg = ToolRegistry()
        reg.register("sphere", description="desc", category="geo", dcc="maya", input_schema=schema)
        v = ToolValidator.from_action_registry(reg, "sphere")
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) > 0


# ---------------------------------------------------------------------------
# PyDccLauncher
# ---------------------------------------------------------------------------


class TestPyDccLauncherConstruction:
    def test_creates_instance(self) -> None:
        launcher = PyDccLauncher()
        assert launcher is not None

    def test_repr_contains_running(self) -> None:
        launcher = PyDccLauncher()
        assert "running" in repr(launcher).lower()

    def test_initial_running_count_zero(self) -> None:
        launcher = PyDccLauncher()
        assert launcher.running_count() == 0

    def test_independence_of_instances(self) -> None:
        l1 = PyDccLauncher()
        l2 = PyDccLauncher()
        assert l1 is not l2
        assert l1.running_count() == 0
        assert l2.running_count() == 0


class TestPyDccLauncherNonexistentProcess:
    def test_pid_of_nonexistent_is_none(self) -> None:
        launcher = PyDccLauncher()
        assert launcher.pid_of("nonexistent") is None

    def test_restart_count_nonexistent_is_zero(self) -> None:
        launcher = PyDccLauncher()
        assert launcher.restart_count("nonexistent") == 0

    def test_kill_nonexistent_raises_runtime_error(self) -> None:
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError, match="not running"):
            launcher.kill("nonexistent")

    def test_terminate_nonexistent_raises_runtime_error(self) -> None:
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError, match="not running"):
            launcher.terminate("nonexistent")

    def test_kill_error_message_contains_name(self) -> None:
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError) as exc_info:
            launcher.kill("maya-test")
        assert "maya-test" in str(exc_info.value)

    def test_terminate_error_message_contains_name(self) -> None:
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError) as exc_info:
            launcher.terminate("blender-test")
        assert "blender-test" in str(exc_info.value)

    def test_pid_of_different_names_all_none(self) -> None:
        launcher = PyDccLauncher()
        for name in ("maya", "blender", "houdini", "3dsmax"):
            assert launcher.pid_of(name) is None

    def test_restart_count_multiple_names_all_zero(self) -> None:
        launcher = PyDccLauncher()
        for name in ("maya", "blender", "houdini"):
            assert launcher.restart_count(name) == 0


# ---------------------------------------------------------------------------
# PyBufferPool and PySharedBuffer
# ---------------------------------------------------------------------------


class TestPyBufferPoolConstruction:
    def test_creates_with_capacity_and_size(self) -> None:
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool is not None

    def test_capacity_method(self) -> None:
        pool = PyBufferPool(capacity=3, buffer_size=512)
        assert pool.capacity() == 3

    def test_buffer_size_method(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=2048)
        assert pool.buffer_size() == 2048

    def test_initial_available_equals_capacity(self) -> None:
        pool = PyBufferPool(capacity=5, buffer_size=256)
        assert pool.available() == 5

    def test_repr_contains_pool(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=64)
        r = repr(pool).lower()
        assert "pool" in r or "buffer" in r

    def test_capacity_one_is_valid(self) -> None:
        pool = PyBufferPool(capacity=1, buffer_size=64)
        assert pool.capacity() == 1
        assert pool.available() == 1


class TestPyBufferPoolAcquire:
    def test_acquire_returns_shared_buffer(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=128)
        buf = pool.acquire()
        assert buf is not None

    def test_acquire_decreases_available(self) -> None:
        pool = PyBufferPool(capacity=3, buffer_size=128)
        _buf = pool.acquire()
        assert pool.available() == 2

    def test_release_on_del_restores_available(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=128)
        buf = pool.acquire()
        assert pool.available() == 1
        del buf
        gc.collect()
        assert pool.available() == 2

    def test_exhaust_pool_raises_runtime_error(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=64)
        _b1 = pool.acquire()
        _b2 = pool.acquire()
        assert pool.available() == 0
        with pytest.raises(RuntimeError, match="exhausted"):
            pool.acquire()

    def test_acquire_all_then_release_one_allows_another(self) -> None:
        pool = PyBufferPool(capacity=2, buffer_size=64)
        b1 = pool.acquire()
        b2 = pool.acquire()
        del b2
        gc.collect()
        b3 = pool.acquire()
        assert b3 is not None
        del b1, b3

    def test_multiple_buffers_have_distinct_ids(self) -> None:
        pool = PyBufferPool(capacity=4, buffer_size=64)
        bufs = [pool.acquire() for _ in range(4)]
        ids = [b.id for b in bufs]
        # IDs are pool-prefixed strings, all distinct
        assert len(set(ids)) == 4
        del bufs


class TestPySharedBufferOperations:
    def setup_method(self) -> None:
        self.pool = PyBufferPool(capacity=2, buffer_size=4096)

    def test_write_and_read_roundtrip(self) -> None:
        buf = self.pool.acquire()
        data = b"hello dcc world"
        buf.write(data)
        assert buf.read() == data

    def test_write_overwrites_previous(self) -> None:
        buf = self.pool.acquire()
        buf.write(b"first")
        buf.write(b"second")
        assert buf.read() == b"second"

    def test_data_len_after_write(self) -> None:
        buf = self.pool.acquire()
        data = b"x" * 100
        buf.write(data)
        assert buf.data_len() == 100

    def test_clear_resets_data_len(self) -> None:
        buf = self.pool.acquire()
        buf.write(b"something")
        buf.clear()
        assert buf.data_len() == 0

    def test_id_is_string(self) -> None:
        buf = self.pool.acquire()
        assert isinstance(buf.id, str)

    def test_id_is_pool_prefixed(self) -> None:
        buf = self.pool.acquire()
        # Pool buffer IDs are pool-<uuid>-<index> format
        assert buf.id.startswith("pool-")

    def test_name_is_string(self) -> None:
        buf = self.pool.acquire()
        assert isinstance(buf.name(), str)

    def test_capacity_matches_pool_buffer_size(self) -> None:
        buf = self.pool.acquire()
        assert buf.capacity() == 4096

    def test_descriptor_json_is_string(self) -> None:
        buf = self.pool.acquire()
        buf.write(b"data")
        desc = buf.descriptor_json()
        assert isinstance(desc, str)

    def test_descriptor_json_parseable(self) -> None:
        buf = self.pool.acquire()
        buf.write(b"data")
        desc = json.loads(buf.descriptor_json())
        assert isinstance(desc, dict)

    def test_read_empty_after_clear(self) -> None:
        buf = self.pool.acquire()
        buf.write(b"some data")
        buf.clear()
        result = buf.read()
        assert result == b""

    def test_write_large_data_up_to_capacity(self) -> None:
        buf = self.pool.acquire()
        data = b"A" * 4096
        buf.write(data)
        assert buf.read() == data


# ---------------------------------------------------------------------------
# PySharedSceneBuffer
# ---------------------------------------------------------------------------


class TestPySharedSceneBufferConstruction:
    def test_write_returns_instance(self) -> None:
        data = b"vertex data"
        buf = PySharedSceneBuffer.write(
            data=data,
            kind=PySceneDataKind.Geometry,
            source_dcc="Maya",
            use_compression=False,
        )
        assert buf is not None

    def test_id_is_uuid_string(self) -> None:
        buf = PySharedSceneBuffer.write(
            data=b"test",
            kind=PySceneDataKind.Geometry,
            source_dcc="Maya",
            use_compression=False,
        )
        uuid.UUID(buf.id)

    def test_total_bytes_matches_data(self) -> None:
        data = b"hello" * 20
        buf = PySharedSceneBuffer.write(
            data=data,
            kind=PySceneDataKind.Geometry,
            source_dcc="Maya",
            use_compression=False,
        )
        assert buf.total_bytes == len(data)

    def test_is_inline_for_small_data(self) -> None:
        buf = PySharedSceneBuffer.write(
            data=b"small",
            kind=PySceneDataKind.Geometry,
            source_dcc="Maya",
            use_compression=False,
        )
        assert buf.is_inline is True

    def test_is_chunked_false_for_small_data(self) -> None:
        buf = PySharedSceneBuffer.write(
            data=b"small",
            kind=PySceneDataKind.Geometry,
            source_dcc="Maya",
            use_compression=False,
        )
        assert buf.is_chunked is False

    def test_read_roundtrip_no_compression(self) -> None:
        data = b"geometry bytes " * 50
        buf = PySharedSceneBuffer.write(
            data=data,
            kind=PySceneDataKind.Geometry,
            source_dcc="Blender",
            use_compression=False,
        )
        assert buf.read() == data

    def test_read_roundtrip_with_compression(self) -> None:
        data = b"animation frame" * 200
        buf = PySharedSceneBuffer.write(
            data=data,
            kind=PySceneDataKind.AnimationCache,
            source_dcc="Houdini",
            use_compression=True,
        )
        assert buf.read() == data

    def test_descriptor_json_is_parseable(self) -> None:
        buf = PySharedSceneBuffer.write(
            data=b"desc test",
            kind=PySceneDataKind.Screenshot,
            source_dcc="Maya",
            use_compression=False,
        )
        desc = json.loads(buf.descriptor_json())
        assert isinstance(desc, dict)

    def test_descriptor_has_meta_key(self) -> None:
        buf = PySharedSceneBuffer.write(
            data=b"x",
            kind=PySceneDataKind.Geometry,
            source_dcc="Maya",
            use_compression=False,
        )
        desc = json.loads(buf.descriptor_json())
        assert "meta" in desc

    def test_descriptor_meta_id_matches_buf_id(self) -> None:
        buf = PySharedSceneBuffer.write(
            data=b"x",
            kind=PySceneDataKind.Geometry,
            source_dcc="Maya",
            use_compression=False,
        )
        desc = json.loads(buf.descriptor_json())
        assert desc["meta"]["id"] == buf.id

    def test_descriptor_meta_source_dcc(self) -> None:
        buf = PySharedSceneBuffer.write(
            data=b"x",
            kind=PySceneDataKind.Geometry,
            source_dcc="Blender",
            use_compression=False,
        )
        desc = json.loads(buf.descriptor_json())
        assert desc["meta"]["source_dcc"] == "Blender"

    def test_different_kinds_yield_different_meta_kind(self) -> None:
        kinds = [
            PySceneDataKind.Geometry,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Screenshot,
            PySceneDataKind.Arbitrary,
        ]
        kind_strings = set()
        for kind in kinds:
            buf = PySharedSceneBuffer.write(
                data=b"payload",
                kind=kind,
                source_dcc="Maya",
                use_compression=False,
            )
            desc = json.loads(buf.descriptor_json())
            kind_strings.add(desc["meta"]["kind"])
        assert len(kind_strings) == 4

    def test_empty_data(self) -> None:
        buf = PySharedSceneBuffer.write(
            data=b"",
            kind=PySceneDataKind.Arbitrary,
            source_dcc="Maya",
            use_compression=False,
        )
        assert buf.read() == b""


class TestPySceneDataKindEnum:
    def test_geometry_exists(self) -> None:
        assert hasattr(PySceneDataKind, "Geometry")

    def test_animation_cache_exists(self) -> None:
        assert hasattr(PySceneDataKind, "AnimationCache")

    def test_screenshot_exists(self) -> None:
        assert hasattr(PySceneDataKind, "Screenshot")

    def test_arbitrary_exists(self) -> None:
        assert hasattr(PySceneDataKind, "Arbitrary")

    def test_all_four_kinds_distinct(self) -> None:
        kinds = [
            PySceneDataKind.Geometry,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Screenshot,
            PySceneDataKind.Arbitrary,
        ]
        # All are not equal to each other
        for i, k1 in enumerate(kinds):
            for j, k2 in enumerate(kinds):
                if i != j:
                    assert k1 != k2


# ---------------------------------------------------------------------------
# DccInfo
# ---------------------------------------------------------------------------


class TestDccInfoConstruction:
    def test_creates_with_all_fields(self) -> None:
        info = DccInfo(
            dcc_type="maya",
            version="2025.0.0",
            pid=1234,
            python_version="3.11.0",
            platform="windows",
            metadata={},
        )
        assert info is not None

    def test_dcc_type_field(self) -> None:
        info = DccInfo(
            dcc_type="blender",
            version="4.0.0",
            pid=5678,
            python_version="3.11.0",
            platform="linux",
            metadata={},
        )
        assert info.dcc_type == "blender"

    def test_version_field(self) -> None:
        info = DccInfo(
            dcc_type="maya",
            version="2024.1",
            pid=100,
            python_version="3.10.0",
            platform="macos",
            metadata={},
        )
        assert info.version == "2024.1"

    def test_pid_field(self) -> None:
        info = DccInfo(
            dcc_type="maya",
            version="2025",
            pid=99999,
            python_version="3.11",
            platform="windows",
            metadata={},
        )
        assert info.pid == 99999

    def test_python_version_field(self) -> None:
        info = DccInfo(
            dcc_type="houdini",
            version="20.0",
            pid=42,
            python_version="3.10.12",
            platform="linux",
            metadata={},
        )
        assert info.python_version == "3.10.12"

    def test_platform_field(self) -> None:
        info = DccInfo(
            dcc_type="maya",
            version="2025",
            pid=1,
            python_version="3.11",
            platform="windows",
            metadata={},
        )
        assert info.platform == "windows"

    def test_metadata_field(self) -> None:
        info = DccInfo(
            dcc_type="maya",
            version="2025",
            pid=1,
            python_version="3.11",
            platform="windows",
            metadata={"project": "test_project"},
        )
        assert info.metadata == {"project": "test_project"}

    def test_to_dict_returns_dict(self) -> None:
        info = DccInfo(
            dcc_type="maya",
            version="2025",
            pid=1,
            python_version="3.11",
            platform="windows",
            metadata={},
        )
        d = info.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_contains_dcc_type(self) -> None:
        info = DccInfo(
            dcc_type="blender",
            version="4.0",
            pid=1,
            python_version="3.11",
            platform="linux",
            metadata={},
        )
        d = info.to_dict()
        assert d.get("dcc_type") == "blender" or "blender" in str(d)

    def test_independence(self) -> None:
        info1 = DccInfo(dcc_type="maya", version="2025", pid=1, python_version="3.11", platform="win", metadata={})
        info2 = DccInfo(dcc_type="blender", version="4.0", pid=2, python_version="3.11", platform="linux", metadata={})
        assert info1.dcc_type != info2.dcc_type


# ---------------------------------------------------------------------------
# DccCapabilities
# ---------------------------------------------------------------------------


class TestDccCapabilitiesConstruction:
    def test_creates_minimal(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert caps is not None

    def test_snapshot_true(self) -> None:
        caps = DccCapabilities(
            snapshot=True,
            scene_info=False,
            script_languages=[],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert caps.snapshot is True

    def test_snapshot_false(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert caps.snapshot is False

    def test_scene_info_field(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=True,
            script_languages=[],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert caps.scene_info is True

    def test_file_operations_field(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[],
            file_operations=True,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert caps.file_operations is True

    def test_selection_field(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[],
            file_operations=False,
            selection=True,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert caps.selection is True

    def test_undo_redo_field(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[],
            file_operations=False,
            selection=False,
            undo_redo=True,
            progress_reporting=False,
            extensions={},
        )
        assert caps.undo_redo is True

    def test_progress_reporting_field(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=True,
            extensions={},
        )
        assert caps.progress_reporting is True

    def test_script_languages_python(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[ScriptLanguage.PYTHON],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert len(caps.script_languages) == 1

    def test_script_languages_multiple(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert len(caps.script_languages) == 2

    def test_extensions_empty_dict(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={},
        )
        assert caps.extensions == {}

    def test_extensions_with_value(self) -> None:
        caps = DccCapabilities(
            snapshot=False,
            scene_info=False,
            script_languages=[],
            file_operations=False,
            selection=False,
            undo_redo=False,
            progress_reporting=False,
            extensions={"materialx": True, "usd": False},
        )
        assert caps.extensions.get("materialx") is True
        assert caps.extensions.get("usd") is False

    def test_script_language_variants_exist(self) -> None:
        assert hasattr(ScriptLanguage, "PYTHON")
        assert hasattr(ScriptLanguage, "MEL")


# ---------------------------------------------------------------------------
# TransportManager
# ---------------------------------------------------------------------------


class TestTransportManagerConstruction:
    def test_creates_with_registry_dir(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        assert tm is not None
        tm.shutdown()

    def test_initial_session_count_zero(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        assert tm.session_count() == 0
        tm.shutdown()

    def test_initial_pool_size_zero(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        assert tm.pool_size() == 0
        tm.shutdown()

    def test_initial_list_all_services_empty(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        assert tm.list_all_services() == []
        tm.shutdown()

    def test_initial_list_all_instances_empty(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        assert tm.list_all_instances() == []
        tm.shutdown()

    def test_is_shutdown_false_initially(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        assert tm.is_shutdown() is False
        tm.shutdown()


class TestTransportManagerBindAndRegister:
    def test_bind_and_register_returns_tuple(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        result = tm.bind_and_register("maya", version="2025")
        assert isinstance(result, tuple)
        assert len(result) == 2
        tm.shutdown()

    def test_instance_id_is_uuid_string(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        instance_id, _ = tm.bind_and_register("maya", version="2025")
        uuid.UUID(instance_id)  # Should not raise
        tm.shutdown()

    def test_registered_service_appears_in_list(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        services = tm.list_all_services()
        assert len(services) == 1
        tm.shutdown()

    def test_service_dcc_type_correct(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("blender", version="4.0")
        entry = tm.list_all_services()[0]
        assert entry.dcc_type == "blender"
        tm.shutdown()

    def test_service_version_correct(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        entry = tm.list_all_services()[0]
        assert "2025" in str(entry.version)
        tm.shutdown()

    def test_service_initial_status_available(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        entry = tm.list_all_services()[0]
        assert entry.status == ServiceStatus.AVAILABLE
        tm.shutdown()

    def test_service_is_ipc_true_on_windows(self, tmp_path) -> None:
        import sys

        if sys.platform != "win32":
            pytest.skip("IPC test only on Windows named pipe")
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        entry = tm.list_all_services()[0]
        assert entry.is_ipc is True
        tm.shutdown()

    def test_service_to_dict_has_keys(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        entry = tm.list_all_services()[0]
        d = entry.to_dict()
        for key in ("instance_id", "dcc_type", "status"):
            assert key in d
        tm.shutdown()

    def test_multiple_dccs_registered(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        tm.bind_and_register("blender", version="4.0")
        tm.bind_and_register("houdini", version="20.0")
        assert len(tm.list_all_services()) == 3
        tm.shutdown()

    def test_list_instances_by_dcc_type(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        tm.bind_and_register("maya", version="2024")
        tm.bind_and_register("blender", version="4.0")
        maya_instances = tm.list_instances("maya")
        blender_instances = tm.list_instances("blender")
        assert len(maya_instances) == 2
        assert len(blender_instances) == 1
        tm.shutdown()

    def test_list_all_instances_count(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        tm.bind_and_register("blender", version="4.0")
        all_instances = tm.list_all_instances()
        assert len(all_instances) == 2
        tm.shutdown()


class TestTransportManagerServiceOps:
    def test_update_service_status(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        instance_id, _ = tm.bind_and_register("maya", version="2025")
        ok = tm.update_service_status("maya", instance_id, ServiceStatus.BUSY)
        assert ok is True
        entry = tm.list_all_services()[0]
        assert entry.status == ServiceStatus.BUSY
        tm.shutdown()

    def test_update_status_nonexistent_instance(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        ok = tm.update_service_status("maya", "00000000-0000-0000-0000-000000000001", ServiceStatus.BUSY)
        assert ok is False
        tm.shutdown()

    def test_deregister_service(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        instance_id, _ = tm.bind_and_register("maya", version="2025")
        ok = tm.deregister_service("maya", instance_id)
        assert ok is True
        assert len(tm.list_all_services()) == 0
        tm.shutdown()

    def test_deregister_nonexistent(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        ok = tm.deregister_service("maya", "00000000-0000-0000-0000-000000000001")
        assert ok is False
        tm.shutdown()

    def test_get_service_found(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        instance_id, _ = tm.bind_and_register("blender", version="4.0")
        entry = tm.get_service("blender", instance_id)
        assert entry is not None
        assert entry.instance_id == instance_id
        tm.shutdown()

    def test_get_service_not_found_returns_none(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        entry = tm.get_service("maya", "00000000-0000-0000-0000-000000000001")
        assert entry is None
        tm.shutdown()

    def test_heartbeat_does_not_raise(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        instance_id, _ = tm.bind_and_register("maya", version="2025")
        tm.heartbeat("maya", instance_id)  # should not raise
        tm.shutdown()

    def test_rank_services_returns_list(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        tm.bind_and_register("maya", version="2024")
        ranked = tm.rank_services("maya")
        assert isinstance(ranked, list)
        assert len(ranked) == 2
        tm.shutdown()

    def test_rank_services_raises_for_unknown_dcc(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        with pytest.raises(RuntimeError):
            tm.rank_services("nonexistent_dcc_xyz")
        tm.shutdown()

    def test_pool_count_for_dcc_initial_zero(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        assert tm.pool_count_for_dcc("maya") == 0
        tm.shutdown()


class TestTransportManagerShutdown:
    def test_shutdown_sets_is_shutdown_true(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.shutdown()
        assert tm.is_shutdown() is True

    def test_shutdown_idempotent(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.shutdown()
        tm.shutdown()  # should not raise
        assert tm.is_shutdown() is True

    def test_shutdown_with_services_registered(self, tmp_path) -> None:
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.bind_and_register("maya", version="2025")
        tm.bind_and_register("blender", version="4.0")
        tm.shutdown()
        assert tm.is_shutdown() is True
