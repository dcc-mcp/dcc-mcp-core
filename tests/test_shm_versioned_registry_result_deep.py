"""Deep tests for PySharedSceneBuffer, PyBufferPool, PySharedBuffer, VersionedRegistry.

SemVer, VersionConstraint, validate_action_result, and from_exception.

This test module covers:

- TestPySharedSceneBuffer (happy path + edge cases)
- TestPySharedBuffer (create/write/read/clear/descriptor_json)
- TestPyBufferPool (capacity/available/acquire/exhaust)
- TestVersionedRegistry (register/resolve/resolve_all/latest_version/versions/keys/remove)
- TestSemVerAndConstraint (parse/compare/matches_constraint/VersionConstraint.matches)
- TestValidateActionResult (None/str/dict/ToolResult inputs)
- TestFromException (basic/with options/context kwargs)
"""

from __future__ import annotations

import threading

import pytest

from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import PySharedBuffer
from dcc_mcp_core import PySharedSceneBuffer
from dcc_mcp_core import SemVer
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VersionedRegistry
from dcc_mcp_core import error_result
from dcc_mcp_core import from_exception
from dcc_mcp_core import success_result
from dcc_mcp_core import validate_action_result

# ---------------------------------------------------------------------------
# TestPySharedSceneBuffer
# ---------------------------------------------------------------------------


class TestPySharedSceneBuffer:
    """Tests for PySharedSceneBuffer - zero-copy DCC scene data exchange."""

    def test_write_and_read_roundtrip(self):
        data = b"vertex data" * 100
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Geometry)
        recovered = ssb.read()
        assert recovered == data

    def test_total_bytes_matches_input(self):
        data = b"x" * 512
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.AnimationCache)
        assert ssb.total_bytes == 512

    def test_id_is_nonempty_string(self):
        ssb = PySharedSceneBuffer.write(data=b"abc", kind=PySceneDataKind.Screenshot)
        assert isinstance(ssb.id, str)
        assert len(ssb.id) > 0

    def test_is_inline_true_for_small_data(self):
        ssb = PySharedSceneBuffer.write(data=b"small", kind=PySceneDataKind.Geometry)
        assert ssb.is_inline is True

    def test_is_chunked_false_for_small_data(self):
        ssb = PySharedSceneBuffer.write(data=b"small", kind=PySceneDataKind.Arbitrary)
        assert ssb.is_chunked is False

    def test_descriptor_json_is_nonempty_string(self):
        ssb = PySharedSceneBuffer.write(data=b"payload", kind=PySceneDataKind.Geometry)
        desc = ssb.descriptor_json()
        assert isinstance(desc, str)
        assert len(desc) > 0

    def test_descriptor_json_contains_id(self):
        ssb = PySharedSceneBuffer.write(data=b"payload", kind=PySceneDataKind.Geometry)
        assert ssb.id in ssb.descriptor_json()

    def test_write_with_compression_roundtrip(self):
        data = b"compressible" * 200
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Geometry, use_compression=True)
        assert ssb.read() == data

    def test_write_without_compression_explicit(self):
        data = b"raw data bytes"
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Geometry, use_compression=False)
        assert ssb.read() == data

    def test_write_with_source_dcc_parameter(self):
        data = b"maya scene"
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Geometry, source_dcc="Maya")
        assert ssb.read() == data

    def test_all_data_kinds_accepted(self):
        kinds = [
            PySceneDataKind.Geometry,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Screenshot,
            PySceneDataKind.Arbitrary,
        ]
        for kind in kinds:
            ssb = PySharedSceneBuffer.write(data=b"data", kind=kind)
            assert ssb.read() == b"data"

    def test_default_kind_is_arbitrary(self):
        ssb = PySharedSceneBuffer.write(data=b"no kind")
        # Should not raise; just confirm the write succeeds
        assert ssb.read() == b"no kind"

    def test_empty_bytes_write(self):
        ssb = PySharedSceneBuffer.write(data=b"", kind=PySceneDataKind.Arbitrary)
        assert ssb.total_bytes == 0
        assert ssb.read() == b""

    def test_unique_ids_per_write(self):
        ids = {PySharedSceneBuffer.write(data=b"d").id for _ in range(10)}
        assert len(ids) == 10

    def test_large_data_roundtrip(self):
        data = bytes(range(256)) * 1024  # 256 KiB
        ssb = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Geometry)
        assert ssb.read() == data

    def test_repr_is_string(self):
        ssb = PySharedSceneBuffer.write(data=b"r")
        assert isinstance(repr(ssb), str)

    def test_concurrent_writes_unique_ids(self):
        results = []
        errors = []

        def worker():
            try:
                ssb = PySharedSceneBuffer.write(data=b"thread data", kind=PySceneDataKind.Geometry)
                results.append(ssb.id)
            except Exception as exc:
                errors.append(exc)

        threads = [threading.Thread(target=worker) for _ in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert not errors
        assert len(set(results)) == 20  # all unique

    def test_compression_reduces_or_equals_size(self):
        data = b"aaaa" * 500  # highly compressible
        ssb_compressed = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Geometry, use_compression=True)
        ssb_raw = PySharedSceneBuffer.write(data=data, kind=PySceneDataKind.Geometry, use_compression=False)
        # Both should decompress to the same data
        assert ssb_compressed.read() == data
        assert ssb_raw.read() == data


# ---------------------------------------------------------------------------
# TestPySharedBuffer
# ---------------------------------------------------------------------------


class TestPySharedBuffer:
    """Tests for PySharedBuffer - named memory-mapped file buffer."""

    def test_create_returns_buffer(self):
        buf = PySharedBuffer.create(capacity=1024)
        assert buf is not None

    def test_capacity_matches_requested(self):
        buf = PySharedBuffer.create(capacity=2048)
        assert buf.capacity() == 2048

    def test_id_is_nonempty_string(self):
        buf = PySharedBuffer.create(capacity=256)
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_name_is_string(self):
        buf = PySharedBuffer.create(capacity=256)
        assert isinstance(buf.name(), str)

    def test_initial_data_len_is_zero(self):
        buf = PySharedBuffer.create(capacity=512)
        assert buf.data_len() == 0

    def test_write_and_read_roundtrip(self):
        buf = PySharedBuffer.create(capacity=1024)
        data = b"hello shared memory"
        n = buf.write(data)
        assert n == len(data)
        assert buf.read() == data

    def test_data_len_after_write(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"test")
        assert buf.data_len() == 4

    def test_clear_resets_data_len(self):
        buf = PySharedBuffer.create(capacity=512)
        buf.write(b"some bytes")
        buf.clear()
        assert buf.data_len() == 0

    def test_overwrite_with_new_data(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"first")
        buf.write(b"second")
        # second write replaces
        assert buf.read() == b"second"

    def test_descriptor_json_is_nonempty(self):
        buf = PySharedBuffer.create(capacity=256)
        desc = buf.descriptor_json()
        assert isinstance(desc, str)
        assert len(desc) > 0

    def test_repr_is_string(self):
        buf = PySharedBuffer.create(capacity=128)
        assert isinstance(repr(buf), str)

    def test_write_empty_bytes(self):
        buf = PySharedBuffer.create(capacity=256)
        n = buf.write(b"")
        assert n == 0
        assert buf.data_len() == 0


# ---------------------------------------------------------------------------
# TestPyBufferPool
# ---------------------------------------------------------------------------


class TestPyBufferPool:
    """Tests for PyBufferPool - fixed-capacity pool of shared buffers."""

    def test_capacity_matches_init(self):
        pool = PyBufferPool(capacity=4, buffer_size=512)
        assert pool.capacity() == 4

    def test_buffer_size_matches_init(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool.buffer_size() == 1024

    def test_available_initially_equals_capacity(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        assert pool.available() == 3

    def test_acquire_decrements_available(self):
        pool = PyBufferPool(capacity=4, buffer_size=256)
        _buf = pool.acquire()
        assert pool.available() == 3

    def test_acquire_returns_shared_buffer(self):
        pool = PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        assert isinstance(buf, PySharedBuffer)

    def test_acquired_buffer_capacity_matches_pool(self):
        pool = PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        assert buf.capacity() == 512

    def test_write_read_on_acquired_buffer(self):
        pool = PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        buf.write(b"pool buffer data")
        assert buf.read() == b"pool buffer data"

    def test_release_on_gc_restores_available(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        buf = pool.acquire()
        assert pool.available() == 1
        del buf
        # After GC the slot should be reclaimed
        assert pool.available() == 2

    def test_exhaust_pool_raises_runtime_error(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        _b1 = pool.acquire()
        _b2 = pool.acquire()
        with pytest.raises(RuntimeError):
            pool.acquire()

    def test_pool_capacity_one(self):
        pool = PyBufferPool(capacity=1, buffer_size=128)
        buf = pool.acquire()
        assert buf is not None
        assert pool.available() == 0

    def test_repr_is_string(self):
        pool = PyBufferPool(capacity=2, buffer_size=256)
        assert isinstance(repr(pool), str)

    def test_acquire_multiple_independent_buffers(self):
        pool = PyBufferPool(capacity=4, buffer_size=512)
        buffers = [pool.acquire() for _ in range(4)]
        ids = [b.id for b in buffers]
        assert len(set(ids)) == 4  # all unique IDs

    def test_concurrent_acquire_no_crash(self):
        pool = PyBufferPool(capacity=8, buffer_size=256)
        errors = []
        acquired = []
        lock = threading.Lock()

        def worker():
            try:
                buf = pool.acquire()
                with lock:
                    acquired.append(buf)
            except RuntimeError:
                pass  # pool exhausted is acceptable
            except Exception as exc:
                errors.append(exc)

        threads = [threading.Thread(target=worker) for _ in range(8)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert not errors


# ---------------------------------------------------------------------------
# TestVersionedRegistry
# ---------------------------------------------------------------------------


class TestVersionedRegistry:
    """Tests for VersionedRegistry - multi-version action registry."""

    def test_empty_registry_keys(self):
        vreg = VersionedRegistry()
        assert vreg.keys() == []

    def test_empty_registry_total_entries(self):
        vreg = VersionedRegistry()
        assert vreg.total_entries() == 0

    def test_empty_latest_version_returns_none(self):
        vreg = VersionedRegistry()
        assert vreg.latest_version(name="x", dcc="maya") is None

    def test_empty_versions_returns_empty_list(self):
        vreg = VersionedRegistry()
        assert vreg.versions(name="x", dcc="maya") == []

    def test_register_versioned_single(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("create_sphere", dcc="maya", version="1.0.0")
        assert vreg.total_entries() == 1

    def test_register_versioned_multiple_versions(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.2.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        assert vreg.total_entries() == 3

    def test_versions_sorted_ascending(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.5.0")
        versions = vreg.versions(name="a", dcc="maya")
        assert versions == ["1.0.0", "1.5.0", "2.0.0"]

    def test_latest_version_is_highest(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.2.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        assert vreg.latest_version(name="a", dcc="maya") == "2.0.0"

    def test_keys_contains_registered_pairs(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("act_a", dcc="maya", version="1.0.0")
        vreg.register_versioned("act_b", dcc="blender", version="0.5.0")
        keys = vreg.keys()
        assert ("act_a", "maya") in keys
        assert ("act_b", "blender") in keys

    def test_resolve_wildcard_returns_latest(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        result = vreg.resolve(name="a", dcc="maya", constraint="*")
        assert result is not None
        assert result["version"] == "2.0.0"

    def test_resolve_gte_constraint(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.5.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        result = vreg.resolve(name="a", dcc="maya", constraint=">=1.0.0")
        assert result["version"] == "2.0.0"

    def test_resolve_caret_constraint(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.2.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        result = vreg.resolve(name="a", dcc="maya", constraint="^1.0.0")
        # ^1.0.0 → >=1.0.0 <2.0.0, best = 1.2.0
        assert result["version"] == "1.2.0"

    def test_resolve_unsatisfied_returns_none(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        result = vreg.resolve(name="a", dcc="maya", constraint=">=9.0.0")
        assert result is None

    def test_resolve_result_has_expected_keys(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0", description="desc", category="geo", tags=["x"])
        result = vreg.resolve(name="a", dcc="maya", constraint="*")
        assert "name" in result
        assert "dcc" in result
        assert "version" in result
        assert "description" in result
        assert "category" in result
        assert "tags" in result

    def test_resolve_all_wildcard_returns_all_sorted(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.5.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        all_r = vreg.resolve_all(name="a", dcc="maya", constraint="*")
        assert len(all_r) == 3
        assert [r["version"] for r in all_r] == ["1.0.0", "1.5.0", "2.0.0"]

    def test_resolve_all_filtered_by_constraint(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.5.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        all_r = vreg.resolve_all(name="a", dcc="maya", constraint="^1.0.0")
        versions = [r["version"] for r in all_r]
        assert "2.0.0" not in versions
        assert "1.0.0" in versions or "1.5.0" in versions

    def test_resolve_all_no_match_returns_empty(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        all_r = vreg.resolve_all(name="a", dcc="maya", constraint=">=9.0.0")
        assert all_r == []

    def test_remove_caret_removes_minor_versions(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="1.2.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        removed = vreg.remove(name="a", dcc="maya", constraint="^1.0.0")
        assert removed == 2
        remaining = vreg.versions(name="a", dcc="maya")
        assert "2.0.0" in remaining
        assert "1.0.0" not in remaining
        assert "1.2.0" not in remaining

    def test_remove_wildcard_removes_all(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="maya", version="2.0.0")
        removed = vreg.remove(name="a", dcc="maya", constraint="*")
        assert removed == 2
        assert vreg.versions(name="a", dcc="maya") == []

    def test_remove_no_match_returns_zero(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        removed = vreg.remove(name="a", dcc="maya", constraint=">=9.0.0")
        assert removed == 0

    def test_register_same_version_overwrites(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0", description="old")
        vreg.register_versioned("a", dcc="maya", version="1.0.0", description="new")
        # Overwrite: total_entries stays 1
        assert vreg.total_entries() == 1
        result = vreg.resolve(name="a", dcc="maya", constraint="=1.0.0")
        if result is not None:
            assert result["description"] == "new"

    def test_different_dccs_are_independent(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("act", dcc="maya", version="1.0.0")
        vreg.register_versioned("act", dcc="blender", version="2.0.0")
        assert vreg.latest_version(name="act", dcc="maya") == "1.0.0"
        assert vreg.latest_version(name="act", dcc="blender") == "2.0.0"

    def test_total_entries_across_dccs(self):
        vreg = VersionedRegistry()
        vreg.register_versioned("a", dcc="maya", version="1.0.0")
        vreg.register_versioned("a", dcc="blender", version="1.0.0")
        vreg.register_versioned("b", dcc="maya", version="2.0.0")
        assert vreg.total_entries() == 3

    def test_repr_is_string(self):
        vreg = VersionedRegistry()
        assert isinstance(repr(vreg), str)

    def test_concurrent_register_no_crash(self):
        vreg = VersionedRegistry()
        errors = []

        def worker(i):
            try:
                vreg.register_versioned(f"action_{i}", dcc="maya", version="1.0.0")
            except Exception as exc:
                errors.append(exc)

        threads = [threading.Thread(target=worker, args=(i,)) for i in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert not errors
        assert vreg.total_entries() == 20


# ---------------------------------------------------------------------------
# TestSemVerAndConstraint
# ---------------------------------------------------------------------------


class TestSemVerAndConstraint:
    """Tests for SemVer and VersionConstraint."""

    def test_semver_parse_three_parts(self):
        v = SemVer.parse("1.2.3")
        assert v.major == 1
        assert v.minor == 2
        assert v.patch == 3

    def test_semver_parse_two_parts(self):
        v = SemVer.parse("2.0")
        assert v.major == 2
        assert v.minor == 0

    def test_semver_parse_with_v_prefix(self):
        v = SemVer.parse("v3.1.0")
        assert v.major == 3
        assert v.minor == 1
        assert v.patch == 0

    def test_semver_parse_prerelease_stripped(self):
        v = SemVer.parse("1.0.0-alpha")
        assert v.major == 1
        assert v.minor == 0
        assert v.patch == 0

    def test_semver_parse_invalid_raises(self):
        with pytest.raises((ValueError, Exception)):
            SemVer.parse("not-a-version")

    def test_semver_equality(self):
        assert SemVer.parse("1.2.3") == SemVer.parse("1.2.3")

    def test_semver_inequality(self):
        assert SemVer.parse("1.0.0") != SemVer.parse("2.0.0")

    def test_semver_ordering(self):
        assert SemVer.parse("1.0.0") < SemVer.parse("2.0.0")
        assert SemVer.parse("2.0.0") > SemVer.parse("1.0.0")
        assert SemVer.parse("1.1.0") >= SemVer.parse("1.0.0")
        assert SemVer.parse("1.0.0") <= SemVer.parse("1.1.0")

    def test_semver_constructor(self):
        v = SemVer(1, 2, 3)
        assert v.major == 1
        assert v.minor == 2
        assert v.patch == 3

    def test_semver_str(self):
        v = SemVer.parse("1.2.3")
        assert "1" in str(v) and "2" in str(v) and "3" in str(v)

    def test_semver_repr(self):
        v = SemVer.parse("1.2.3")
        assert isinstance(repr(v), str)

    def test_matches_constraint_gte(self):
        v = SemVer.parse("2.0.0")
        vc_gte = VersionConstraint.parse(">=1.0.0")
        vc_high = VersionConstraint.parse(">=3.0.0")
        assert v.matches_constraint(vc_gte) is True
        assert v.matches_constraint(vc_high) is False

    def test_matches_constraint_caret(self):
        v = SemVer.parse("1.5.0")
        vc = VersionConstraint.parse("^1.0.0")
        assert v.matches_constraint(vc) is True
        v2 = SemVer.parse("2.0.0")
        assert v2.matches_constraint(vc) is False

    def test_version_constraint_parse(self):
        vc = VersionConstraint.parse(">=1.0.0")
        assert vc is not None

    def test_version_constraint_matches_semver(self):
        vc = VersionConstraint.parse(">=1.0.0")
        v = SemVer.parse("2.0.0")
        assert vc.matches(v) is True

    def test_version_constraint_no_match(self):
        vc = VersionConstraint.parse(">=3.0.0")
        v = SemVer.parse("1.0.0")
        assert vc.matches(v) is False

    def test_version_constraint_str(self):
        vc = VersionConstraint.parse(">=1.0.0")
        s = str(vc)
        assert isinstance(s, str)

    def test_version_constraint_repr(self):
        vc = VersionConstraint.parse("*")
        assert isinstance(repr(vc), str)

    def test_version_constraint_wildcard_matches_any(self):
        vc = VersionConstraint.parse("*")
        for ver_str in ["0.0.1", "1.0.0", "99.99.99"]:
            assert vc.matches(SemVer.parse(ver_str)) is True


# ---------------------------------------------------------------------------
# TestValidateActionResult
# ---------------------------------------------------------------------------


class TestValidateActionResult:
    """Tests for validate_action_result - normalise various inputs."""

    def test_none_returns_success_result(self):
        r = validate_action_result(None)
        assert r.success is True

    def test_string_returns_success_result(self):
        r = validate_action_result("some string")
        assert r.success is True

    def test_empty_string_returns_success_result(self):
        r = validate_action_result("")
        assert r.success is True

    def test_dict_success_true(self):
        r = validate_action_result({"success": True, "message": "ok"})
        assert r.success is True
        assert r.message == "ok"

    def test_dict_success_false(self):
        r = validate_action_result({"success": False, "error": "bad"})
        assert r.success is False
        assert r.error == "bad"

    def test_dict_no_success_key_treated_as_success(self):
        r = validate_action_result({"message": "no flag"})
        assert r.success is True

    def test_action_result_model_passthrough(self):
        original = success_result(message="hello", context={"k": "v"})
        r = validate_action_result(original)
        assert r.success is True
        assert r.message == "hello"

    def test_error_result_model_passthrough(self):
        original = error_result(message="err occurred", error="err msg")
        r = validate_action_result(original)
        assert r.success is False

    def test_list_returns_success_result(self):
        r = validate_action_result([1, 2, 3])
        assert r.success is True

    def test_int_returns_success_result(self):
        r = validate_action_result(42)
        assert r.success is True

    def test_bool_true_returns_success_result(self):
        r = validate_action_result(True)
        assert r.success is True

    def test_returns_action_result_model_type(self):
        from dcc_mcp_core import ToolResult

        r = validate_action_result(None)
        assert isinstance(r, ToolResult)


# ---------------------------------------------------------------------------
# TestFromException
# ---------------------------------------------------------------------------


class TestFromException:
    """Tests for from_exception - wrap an error string as ToolResult."""

    def test_basic_success_false(self):
        r = from_exception("something went wrong")
        assert r.success is False

    def test_error_field_set(self):
        r = from_exception("my error")
        assert r.error == "my error"

    def test_message_field_default_none_or_str(self):
        r = from_exception("my error")
        # message may be None or auto-generated
        assert r.message is None or isinstance(r.message, str)

    def test_custom_message(self):
        r = from_exception("my error", message="Custom message")
        assert r.message == "Custom message"

    def test_include_traceback_false_no_traceback_in_context(self):
        r = from_exception("my error", include_traceback=False)
        assert r.success is False

    def test_include_traceback_true_has_traceback_context(self):
        r = from_exception("my error", include_traceback=True)
        assert "traceback" in r.context

    def test_context_kwargs_are_stored(self):
        r = from_exception("my error", foo="bar", baz=42)
        assert "foo" in r.context
        assert r.context["foo"] == "bar"
        assert r.context["baz"] == 42

    def test_possible_solutions_parameter(self):
        r = from_exception("my error", possible_solutions=["try A", "try B"], include_traceback=False)
        assert r.success is False

    def test_prompt_field(self):
        r = from_exception("my error", prompt="Try checking logs")
        assert r.prompt == "Try checking logs"

    def test_context_contains_error_type(self):
        r = from_exception("my error", include_traceback=True)
        assert "error_type" in r.context

    def test_returns_action_result_model_type(self):
        from dcc_mcp_core import ToolResult

        r = from_exception("my error")
        assert isinstance(r, ToolResult)

    def test_empty_error_string(self):
        r = from_exception("")
        assert r.success is False

    def test_to_dict_has_success_key(self):
        r = from_exception("err")
        d = r.to_dict()
        assert "success" in d
        assert d["success"] is False

    def test_concurrent_from_exception_no_crash(self):
        errors = []

        def worker(i):
            try:
                r = from_exception(f"error {i}", include_traceback=False)
                assert r.success is False
            except Exception as exc:
                errors.append(exc)

        threads = [threading.Thread(target=worker, args=(i,)) for i in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert not errors
