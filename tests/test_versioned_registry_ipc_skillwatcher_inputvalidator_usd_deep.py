"""Deep tests for VersionedRegistry/SemVer/VersionConstraint, IpcListener/ListenerHandle.

Covers: SkillWatcher, InputValidator, and UsdStage/UsdPrim/SdfPath APIs.
"""

from __future__ import annotations

# Import built-in modules
import json
import tempfile

# Import third-party modules
import pytest

from dcc_mcp_core import InputValidator

# Import local modules
from dcc_mcp_core import IpcListener
from dcc_mcp_core import SdfPath
from dcc_mcp_core import SemVer
from dcc_mcp_core import SkillWatcher
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import UsdPrim
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VersionedRegistry
from dcc_mcp_core import VtValue
from dcc_mcp_core import scene_info_json_to_stage
from dcc_mcp_core import stage_to_scene_info_json


# ---------------------------------------------------------------------------
# TestSemVer
# ---------------------------------------------------------------------------
class TestSemVer:
    """Tests for SemVer parsing and comparison."""

    class TestParsing:
        def test_parse_basic(self):
            sv = SemVer.parse("1.2.3")
            assert sv.major == 1
            assert sv.minor == 2
            assert sv.patch == 3

        def test_parse_major_only_zero(self):
            sv = SemVer.parse("0.0.0")
            assert sv.major == 0
            assert sv.minor == 0
            assert sv.patch == 0

        def test_parse_large_version(self):
            sv = SemVer.parse("10.20.30")
            assert sv.major == 10
            assert sv.minor == 20
            assert sv.patch == 30

        def test_repr_contains_components(self):
            sv = SemVer.parse("3.4.5")
            r = repr(sv)
            assert "3" in r
            assert "4" in r
            assert "5" in r

        def test_parse_returns_semver(self):
            sv = SemVer.parse("1.0.0")
            assert isinstance(sv, SemVer)

    class TestComparison:
        def test_less_than(self):
            sv1 = SemVer.parse("1.0.0")
            sv2 = SemVer.parse("2.0.0")
            assert sv1 < sv2

        def test_equal(self):
            sv1 = SemVer.parse("1.2.3")
            sv2 = SemVer.parse("1.2.3")
            assert sv1 == sv2

        def test_less_than_or_equal(self):
            sv1 = SemVer.parse("1.0.0")
            sv2 = SemVer.parse("2.0.0")
            assert sv1 <= sv2

        def test_less_than_minor(self):
            sv1 = SemVer.parse("1.1.0")
            sv2 = SemVer.parse("1.2.0")
            assert sv1 < sv2

        def test_less_than_patch(self):
            sv1 = SemVer.parse("1.0.0")
            sv2 = SemVer.parse("1.0.1")
            assert sv1 < sv2

        def test_not_equal(self):
            sv1 = SemVer.parse("1.0.0")
            sv2 = SemVer.parse("2.0.0")
            assert sv1 != sv2

    class TestConstraintMatching:
        def test_matches_constraint_gte(self):
            sv = SemVer.parse("1.5.0")
            vc = VersionConstraint.parse(">=1.0.0")
            assert sv.matches_constraint(vc) is True

        def test_matches_constraint_gte_fails(self):
            sv = SemVer.parse("0.9.0")
            vc = VersionConstraint.parse(">=1.0.0")
            assert sv.matches_constraint(vc) is False

        def test_matches_constraint_caret(self):
            sv = SemVer.parse("1.2.3")
            vc = VersionConstraint.parse("^1.0.0")
            assert sv.matches_constraint(vc) is True

        def test_matches_constraint_caret_excludes_major(self):
            sv = SemVer.parse("2.0.0")
            vc = VersionConstraint.parse("^1.0.0")
            assert sv.matches_constraint(vc) is False

        def test_matches_constraint_gt(self):
            sv = SemVer.parse("2.0.0")
            vc = VersionConstraint.parse(">1.0.0")
            assert sv.matches_constraint(vc) is True


# ---------------------------------------------------------------------------
# TestVersionConstraint
# ---------------------------------------------------------------------------
class TestVersionConstraint:
    """Tests for VersionConstraint parsing and matching."""

    def test_parse_gte(self):
        vc = VersionConstraint.parse(">=1.0.0")
        assert isinstance(vc, VersionConstraint)

    def test_parse_caret(self):
        vc = VersionConstraint.parse("^1.0.0")
        assert isinstance(vc, VersionConstraint)

    def test_parse_wildcard(self):
        vc = VersionConstraint.parse("*")
        assert isinstance(vc, VersionConstraint)

    def test_repr_contains_constraint(self):
        vc = VersionConstraint.parse(">=1.0.0")
        r = repr(vc)
        assert "1.0.0" in r

    def test_matches_semver_true(self):
        vc = VersionConstraint.parse(">=1.0.0")
        sv = SemVer.parse("2.0.0")
        assert vc.matches(sv) is True

    def test_matches_semver_false(self):
        vc = VersionConstraint.parse(">2.0.0")
        sv = SemVer.parse("1.9.0")
        assert vc.matches(sv) is False

    def test_wildcard_matches_all(self):
        vc = VersionConstraint.parse("*")
        for v in ["1.0.0", "0.0.1", "99.99.99"]:
            sv = SemVer.parse(v)
            assert vc.matches(sv) is True


# ---------------------------------------------------------------------------
# TestVersionedRegistry
# ---------------------------------------------------------------------------
class TestVersionedRegistry:
    """Tests for VersionedRegistry multi-version action management."""

    class TestRegistration:
        def test_empty_on_create(self):
            vreg = VersionedRegistry()
            assert vreg.total_entries() == 0

        def test_register_one(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            assert vreg.total_entries() == 1

        def test_register_multiple_versions(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            assert vreg.total_entries() == 2

        def test_register_different_dccs(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="blender", version="1.0.0")
            assert vreg.total_entries() == 2

        def test_keys_contains_tuple(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            keys = vreg.keys()
            assert ("act", "maya") in keys

        def test_keys_multiple(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("create_sphere", dcc="maya", version="1.0.0")
            vreg.register_versioned("delete_mesh", dcc="blender", version="2.0.0")
            keys = vreg.keys()
            assert len(keys) == 2

    class TestVersionLookup:
        def test_versions_list(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="1.2.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            versions = vreg.versions("act", dcc="maya")
            assert "1.0.0" in versions
            assert "1.2.0" in versions
            assert "2.0.0" in versions

        def test_versions_sorted(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="1.5.0")
            versions = vreg.versions("act", dcc="maya")
            assert versions == sorted(versions, key=lambda v: [int(x) for x in v.split(".")])

        def test_latest_version(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="1.2.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            latest = vreg.latest_version("act", dcc="maya")
            assert latest == "2.0.0"

        def test_latest_version_single(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="3.1.4")
            latest = vreg.latest_version("act", dcc="maya")
            assert latest == "3.1.4"

    class TestResolve:
        def test_resolve_gte_returns_latest(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            result = vreg.resolve("act", dcc="maya", constraint=">=1.0.0")
            assert result is not None
            assert result["version"] == "2.0.0"

        def test_resolve_caret_returns_within_major(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="1.2.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            result = vreg.resolve("act", dcc="maya", constraint="^1.0.0")
            assert result is not None
            assert result["version"] == "1.2.0"

        def test_resolve_returns_dict_with_keys(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            result = vreg.resolve("act", dcc="maya", constraint="*")
            assert isinstance(result, dict)
            assert "name" in result
            assert "dcc" in result
            assert "version" in result

        def test_resolve_name_correct(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("my_action", dcc="blender", version="1.0.0")
            result = vreg.resolve("my_action", dcc="blender", constraint="*")
            assert result["name"] == "my_action"
            assert result["dcc"] == "blender"

        def test_resolve_all_returns_list(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            results = vreg.resolve_all("act", dcc="maya", constraint="*")
            assert isinstance(results, list)
            assert len(results) == 2

        def test_resolve_all_wildcard_all_versions(self):
            vreg = VersionedRegistry()
            for v in ["1.0.0", "1.2.0", "2.0.0", "3.0.0"]:
                vreg.register_versioned("act", dcc="maya", version=v)
            results = vreg.resolve_all("act", dcc="maya", constraint="*")
            assert len(results) == 4

        def test_resolve_all_caret_subset(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="1.2.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            results = vreg.resolve_all("act", dcc="maya", constraint="^1.0.0")
            versions_found = [r["version"] for r in results]
            assert "1.0.0" in versions_found
            assert "1.2.0" in versions_found
            assert "2.0.0" not in versions_found

    class TestRemove:
        def test_remove_caret_returns_count(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="1.2.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            removed = vreg.remove("act", dcc="maya", constraint="^1.0.0")
            assert removed == 2

        def test_remove_leaves_non_matching(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            vreg.remove("act", dcc="maya", constraint="^1.0.0")
            versions = vreg.versions("act", dcc="maya")
            assert "2.0.0" in versions
            assert "1.0.0" not in versions

        def test_remove_all_wildcard(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="1.0.0")
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            removed = vreg.remove("act", dcc="maya", constraint="*")
            assert removed == 2
            assert vreg.total_entries() == 0

        def test_remove_returns_zero_if_none(self):
            vreg = VersionedRegistry()
            vreg.register_versioned("act", dcc="maya", version="2.0.0")
            removed = vreg.remove("act", dcc="maya", constraint="^1.0.0")
            assert removed == 0


# ---------------------------------------------------------------------------
# TestIpcListenerAndHandle
# ---------------------------------------------------------------------------
_PORT_BASE = 19900  # base port range for IPC tests


class TestIpcListener:
    """Tests for IpcListener.bind and derived ListenerHandle."""

    class TestBind:
        def test_bind_creates_listener(self):
            addr = TransportAddress.default_local("test-bind", _PORT_BASE)
            listener = IpcListener.bind(addr)
            assert listener is not None
            h = listener.into_handle()
            h.shutdown()

        def test_local_address_is_callable(self):
            addr = TransportAddress.default_local("test-la", _PORT_BASE + 1)
            listener = IpcListener.bind(addr)
            la = listener.local_address()
            assert la is not None
            assert isinstance(la, TransportAddress)
            h = listener.into_handle()
            h.shutdown()

        def test_transport_name_is_named_pipe(self):
            addr = TransportAddress.default_local("test-tn", _PORT_BASE + 2)
            listener = IpcListener.bind(addr)
            import sys

            expected = "named_pipe" if sys.platform == "win32" else "unix_socket"
            assert listener.transport_name == expected
            h = listener.into_handle()
            h.shutdown()

        def test_transport_name_is_str(self):
            addr = TransportAddress.default_local("test-tn2", _PORT_BASE + 3)
            listener = IpcListener.bind(addr)
            assert isinstance(listener.transport_name, str)
            h = listener.into_handle()
            h.shutdown()

        def test_into_handle_consumes_listener(self):
            addr = TransportAddress.default_local("test-ih", _PORT_BASE + 4)
            listener = IpcListener.bind(addr)
            h = listener.into_handle()
            assert h is not None
            h.shutdown()

    class TestListenerHandle:
        def test_is_shutdown_false_initially(self):
            addr = TransportAddress.default_local("test-isf", _PORT_BASE + 5)
            listener = IpcListener.bind(addr)
            h = listener.into_handle()
            assert h.is_shutdown is False
            h.shutdown()

        def test_is_shutdown_true_after_shutdown(self):
            addr = TransportAddress.default_local("test-ist", _PORT_BASE + 6)
            listener = IpcListener.bind(addr)
            h = listener.into_handle()
            h.shutdown()
            assert h.is_shutdown is True

        def test_accept_count_zero(self):
            addr = TransportAddress.default_local("test-ac", _PORT_BASE + 7)
            listener = IpcListener.bind(addr)
            h = listener.into_handle()
            assert h.accept_count == 0
            h.shutdown()

        def test_accept_count_is_int(self):
            addr = TransportAddress.default_local("test-aci", _PORT_BASE + 8)
            listener = IpcListener.bind(addr)
            h = listener.into_handle()
            assert isinstance(h.accept_count, int)
            h.shutdown()

        def test_handle_local_address(self):
            addr = TransportAddress.default_local("test-hla", _PORT_BASE + 9)
            listener = IpcListener.bind(addr)
            h = listener.into_handle()
            la = h.local_address
            assert la is not None
            h.shutdown()

        def test_handle_transport_name(self):
            addr = TransportAddress.default_local("test-htn", _PORT_BASE + 10)
            listener = IpcListener.bind(addr)
            h = listener.into_handle()
            import sys

            expected = "named_pipe" if sys.platform == "win32" else "unix_socket"
            assert h.transport_name == expected
            h.shutdown()

        def test_shutdown_idempotent(self):
            addr = TransportAddress.default_local("test-si", _PORT_BASE + 11)
            listener = IpcListener.bind(addr)
            h = listener.into_handle()
            h.shutdown()
            h.shutdown()  # Should not raise
            assert h.is_shutdown is True

        def test_different_listeners_different_addresses(self):
            addr1 = TransportAddress.default_local("listener-a", _PORT_BASE + 12)
            addr2 = TransportAddress.default_local("listener-b", _PORT_BASE + 13)
            l1 = IpcListener.bind(addr1)
            l2 = IpcListener.bind(addr2)
            la1 = str(l1.local_address())
            la2 = str(l2.local_address())
            assert la1 != la2
            l1.into_handle().shutdown()
            l2.into_handle().shutdown()


# ---------------------------------------------------------------------------
# TestSkillWatcher
# ---------------------------------------------------------------------------
class TestSkillWatcher:
    """Tests for SkillWatcher path management and reload."""

    class TestInitialState:
        def test_create_instance(self):
            sw = SkillWatcher()
            assert sw is not None

        def test_watched_paths_empty(self):
            sw = SkillWatcher()
            paths = sw.watched_paths()
            assert isinstance(paths, list)
            assert len(paths) == 0

        def test_skill_count_zero(self):
            sw = SkillWatcher()
            assert sw.skill_count() == 0

        def test_skills_empty(self):
            sw = SkillWatcher()
            skills = sw.skills()
            assert isinstance(skills, list)
            assert len(skills) == 0

    class TestWatchUnwatch:
        def test_watch_adds_path(self):
            sw = SkillWatcher()
            with tempfile.TemporaryDirectory() as tmpdir:
                sw.watch(tmpdir)
                paths = sw.watched_paths()
                assert any(tmpdir in p for p in paths)

        def test_unwatch_removes_path(self):
            sw = SkillWatcher()
            with tempfile.TemporaryDirectory() as tmpdir:
                sw.watch(tmpdir)
                sw.unwatch(tmpdir)
                paths = sw.watched_paths()
                assert all(tmpdir not in p for p in paths)

        def test_watched_paths_returns_list(self):
            sw = SkillWatcher()
            paths = sw.watched_paths()
            assert isinstance(paths, list)

        def test_watch_multiple_paths(self):
            sw = SkillWatcher()
            with tempfile.TemporaryDirectory() as tmp1, tempfile.TemporaryDirectory() as tmp2:
                sw.watch(tmp1)
                sw.watch(tmp2)
                paths = sw.watched_paths()
                assert len(paths) >= 2

        def test_unwatch_nonexistent_noop(self):
            sw = SkillWatcher()
            # Should not raise
            sw.unwatch("/nonexistent/path/that/does/not/exist")

    class TestReload:
        def test_reload_empty_returns(self):
            sw = SkillWatcher()
            # reload() on empty watcher should not raise
            sw.reload()
            assert sw.skill_count() == 0

        def test_reload_with_empty_dir(self):
            sw = SkillWatcher()
            with tempfile.TemporaryDirectory() as tmpdir:
                sw.watch(tmpdir)
                sw.reload()
                assert sw.skill_count() == 0

        def test_skill_count_is_int(self):
            sw = SkillWatcher()
            assert isinstance(sw.skill_count(), int)

        def test_skills_returns_list_type(self):
            sw = SkillWatcher()
            skills = sw.skills()
            assert isinstance(skills, list)


# ---------------------------------------------------------------------------
# TestInputValidator
# ---------------------------------------------------------------------------
class TestInputValidator:
    """Tests for InputValidator field rules and validation."""

    class TestRequireString:
        def test_valid_string(self):
            v = InputValidator()
            v.require_string("name", 50, 1)
            ok, err = v.validate(json.dumps({"name": "hello"}))
            assert ok is True
            assert err is None

        def test_missing_required_field(self):
            v = InputValidator()
            v.require_string("name", 50, 1)
            ok, err = v.validate(json.dumps({}))
            assert ok is False
            assert err is not None
            assert "name" in err

        def test_string_too_short(self):
            v = InputValidator()
            v.require_string("name", 50, 3)
            ok, err = v.validate(json.dumps({"name": "ab"}))
            assert ok is False
            assert err is not None

        def test_string_too_long(self):
            v = InputValidator()
            v.require_string("name", 10, 1)
            ok, err = v.validate(json.dumps({"name": "x" * 20}))
            assert ok is False
            assert err is not None

        def test_string_at_min_length(self):
            v = InputValidator()
            v.require_string("name", 50, 3)
            ok, _err = v.validate(json.dumps({"name": "abc"}))
            assert ok is True

        def test_string_at_max_length(self):
            v = InputValidator()
            v.require_string("name", 5, 1)
            ok, _err = v.validate(json.dumps({"name": "hello"}))
            assert ok is True

        def test_require_string_mutates_in_place(self):
            v = InputValidator()
            ret = v.require_string("name", 50, 1)
            assert ret is None  # mutates in-place, returns None

        def test_validate_returns_tuple(self):
            v = InputValidator()
            v.require_string("name", 50, 1)
            result = v.validate(json.dumps({"name": "test"}))
            assert isinstance(result, tuple)
            assert len(result) == 2

        def test_error_message_contains_field_name(self):
            v = InputValidator()
            v.require_string("username", 50, 1)
            _ok, err = v.validate(json.dumps({}))
            assert "username" in err

    class TestRequireNumber:
        def test_valid_number(self):
            v = InputValidator()
            v.require_number("age", 0.0, 150.0)
            ok, _err = v.validate(json.dumps({"age": 25}))
            assert ok is True

        def test_number_too_low(self):
            v = InputValidator()
            v.require_number("age", 0.0, 150.0)
            ok, _err = v.validate(json.dumps({"age": -1}))
            assert ok is False

        def test_number_too_high(self):
            v = InputValidator()
            v.require_number("age", 0.0, 150.0)
            ok, _err = v.validate(json.dumps({"age": 200}))
            assert ok is False

        def test_number_missing(self):
            v = InputValidator()
            v.require_number("age", 0.0, 150.0)
            ok, _err = v.validate(json.dumps({}))
            assert ok is False

        def test_number_at_min(self):
            v = InputValidator()
            v.require_number("val", 0.0, 100.0)
            ok, _err = v.validate(json.dumps({"val": 0.0}))
            assert ok is True

        def test_number_at_max(self):
            v = InputValidator()
            v.require_number("val", 0.0, 100.0)
            ok, _err = v.validate(json.dumps({"val": 100.0}))
            assert ok is True

    class TestForbidSubstrings:
        def test_safe_value_passes(self):
            v = InputValidator()
            v.forbid_substrings("cmd", ["DROP TABLE", "rm -rf"])
            ok, err = v.validate(json.dumps({"cmd": "ls -la"}))
            assert ok is True
            assert err is None

        def test_forbidden_substring_fails(self):
            v = InputValidator()
            v.forbid_substrings("cmd", ["rm -rf"])
            ok, err = v.validate(json.dumps({"cmd": "rm -rf /"}))
            assert ok is False
            assert err is not None

        def test_error_mentions_forbidden(self):
            v = InputValidator()
            v.forbid_substrings("cmd", ["DROP TABLE"])
            ok, err = v.validate(json.dumps({"cmd": "SELECT * FROM users; DROP TABLE users;"}))
            assert ok is False
            assert "DROP TABLE" in err

        def test_empty_forbid_list_passes_all(self):
            v = InputValidator()
            v.forbid_substrings("cmd", [])
            ok, _err = v.validate(json.dumps({"cmd": "anything goes"}))
            assert ok is True

        def test_multiple_forbidden_strings(self):
            v = InputValidator()
            v.forbid_substrings("cmd", ["bad1", "bad2", "bad3"])
            ok1, _ = v.validate(json.dumps({"cmd": "bad1 present"}))
            ok2, _ = v.validate(json.dumps({"cmd": "bad2 present"}))
            ok3, _ = v.validate(json.dumps({"cmd": "safe"}))
            assert ok1 is False
            assert ok2 is False
            assert ok3 is True

    class TestCombinedRules:
        def test_multiple_rules_all_pass(self):
            v = InputValidator()
            v.require_string("username", 20, 3)
            v.require_number("age", 0.0, 120.0)
            ok, _err = v.validate(json.dumps({"username": "alice", "age": 30}))
            assert ok is True

        def test_multiple_rules_one_fails(self):
            v = InputValidator()
            v.require_string("username", 20, 3)
            v.require_number("age", 0.0, 120.0)
            ok, _err = v.validate(json.dumps({"username": "alice", "age": 200}))
            assert ok is False


# ---------------------------------------------------------------------------
# TestSdfPath
# ---------------------------------------------------------------------------
class TestSdfPath:
    """Tests for SdfPath construction and navigation."""

    class TestConstruction:
        def test_create_root(self):
            p = SdfPath("/World")
            assert str(p) == "/World"

        def test_create_nested(self):
            p = SdfPath("/World/Sphere")
            assert str(p) == "/World/Sphere"

        def test_is_absolute(self):
            p = SdfPath("/World")
            assert p.is_absolute is True

        def test_name_returns_last_component(self):
            p = SdfPath("/World/Sphere")
            assert p.name == "Sphere"

        def test_root_name(self):
            p = SdfPath("/World")
            assert p.name == "World"

        def test_parent(self):
            p = SdfPath("/World/Sphere")
            # parent is a method on SdfPath
            parent = p.parent()
            assert str(parent) == "/World"

        def test_parent_of_root(self):
            p = SdfPath("/World")
            parent = p.parent()
            assert parent is not None  # returns something (pseudo-root)

        def test_child_appends(self):
            p = SdfPath("/World")
            c = p.child("Sphere")
            assert "Sphere" in str(c)

        def test_repr_contains_path(self):
            p = SdfPath("/World/Cube")
            r = repr(p)
            assert "/World/Cube" in r or "World" in r

    class TestEquality:
        def test_same_path_equal(self):
            p1 = SdfPath("/World/Sphere")
            p2 = SdfPath("/World/Sphere")
            assert p1 == p2

        def test_different_paths_not_equal(self):
            p1 = SdfPath("/World/Sphere")
            p2 = SdfPath("/World/Cube")
            assert p1 != p2


# ---------------------------------------------------------------------------
# TestUsdPrim
# ---------------------------------------------------------------------------
class TestUsdPrim:
    """Tests for UsdPrim attributes and navigation."""

    @pytest.fixture
    def stage_with_prims(self):
        stage = scene_info_json_to_stage(json.dumps({"name": "test", "fps": 24.0, "objects": [], "metadata": {}}))
        prim = stage.define_prim("/World/Sphere", "Sphere")
        return stage, prim

    def test_prim_type(self):
        stage = scene_info_json_to_stage(json.dumps({"name": "test", "fps": 24.0, "objects": [], "metadata": {}}))
        prim = stage.define_prim("/World/Sphere", "Sphere")
        assert prim.type_name == "Sphere"

    def test_prim_name(self):
        stage = scene_info_json_to_stage(json.dumps({"name": "test", "fps": 24.0, "objects": [], "metadata": {}}))
        prim = stage.define_prim("/World/MySphere", "Sphere")
        assert prim.name == "MySphere"

    def test_prim_path(self):
        stage = scene_info_json_to_stage(json.dumps({"name": "test", "fps": 24.0, "objects": [], "metadata": {}}))
        prim = stage.define_prim("/World/MySphere", "Sphere")
        assert isinstance(prim.path, SdfPath)
        assert str(prim.path) == "/World/MySphere"

    def test_prim_active_true(self):
        stage = scene_info_json_to_stage(json.dumps({"name": "test", "fps": 24.0, "objects": [], "metadata": {}}))
        prim = stage.define_prim("/World/Active", "Xform")
        assert prim.active is True

    def test_prim_attribute_names_is_list(self):
        stage = scene_info_json_to_stage(json.dumps({"name": "test", "fps": 24.0, "objects": [], "metadata": {}}))
        prim = stage.define_prim("/World/Thing", "Mesh")
        names = prim.attribute_names()
        assert isinstance(names, list)

    def test_prim_set_get_attribute(self):
        stage = scene_info_json_to_stage(json.dumps({"name": "test", "fps": 24.0, "objects": [], "metadata": {}}))
        stage.define_prim("/World/Light", "DistantLight")
        stage.set_attribute("/World/Light", "intensity", VtValue.from_float(500.0))
        val = stage.get_attribute("/World/Light", "intensity")
        assert val is not None
        assert isinstance(val, VtValue)

    def test_prim_attributes_summary_is_dict(self):
        stage = scene_info_json_to_stage(json.dumps({"name": "test", "fps": 24.0, "objects": [], "metadata": {}}))
        prim = stage.define_prim("/World/Obj", "Mesh")
        summary = prim.attributes_summary()
        assert isinstance(summary, dict)


# ---------------------------------------------------------------------------
# TestUsdStage
# ---------------------------------------------------------------------------
class TestUsdStage:
    """Tests for UsdStage creation, prim management, and serialization."""

    def _make_stage(self, name: str = "test", fps: float = 24.0) -> UsdStage:
        return scene_info_json_to_stage(json.dumps({"name": name, "fps": fps, "objects": [], "metadata": {}}))

    class TestCreation:
        def _make_stage(self, name: str = "test", fps: float = 24.0) -> UsdStage:
            return scene_info_json_to_stage(json.dumps({"name": name, "fps": fps, "objects": [], "metadata": {}}))

        def test_scene_info_to_stage(self):
            stage = self._make_stage()
            assert isinstance(stage, UsdStage)

        def test_stage_name(self):
            stage = self._make_stage("my_scene")
            assert stage.name == "my_scene"

        def test_stage_fps(self):
            stage = self._make_stage(fps=30.0)
            assert stage.fps == 30.0

        def test_stage_up_axis_default(self):
            stage = self._make_stage()
            assert isinstance(stage.up_axis, str)

        def test_stage_has_prims(self):
            stage = self._make_stage()
            count = stage.prim_count()
            assert isinstance(count, int)
            assert count >= 0

        def test_stage_repr(self):
            stage = self._make_stage("repr_test")
            r = repr(stage)
            assert "repr_test" in r or "UsdStage" in r

    class TestPrimOperations:
        def _make_stage(self, name: str = "test") -> UsdStage:
            return scene_info_json_to_stage(json.dumps({"name": name, "fps": 24.0, "objects": [], "metadata": {}}))

        def test_define_prim(self):
            stage = self._make_stage()
            prim = stage.define_prim("/World/Sphere", "Sphere")
            assert prim is not None
            assert isinstance(prim, UsdPrim)

        def test_has_prim_true(self):
            stage = self._make_stage()
            stage.define_prim("/World/Cube", "Cube")
            assert stage.has_prim("/World/Cube") is True

        def test_has_prim_false(self):
            stage = self._make_stage()
            assert stage.has_prim("/World/Nonexistent") is False

        def test_get_prim_existing(self):
            stage = self._make_stage()
            stage.define_prim("/World/Box", "Cube")
            prim = stage.get_prim("/World/Box")
            assert prim is not None

        def test_list_prims_returns_list(self):
            stage = self._make_stage()
            prims = stage.list_prims()
            assert isinstance(prims, list)

        def test_remove_prim_reduces_count(self):
            stage = self._make_stage()
            stage.define_prim("/World/Temp", "Mesh")
            count_before = stage.prim_count()
            stage.remove_prim("/World/Temp")
            count_after = stage.prim_count()
            assert count_after < count_before

        def test_remove_prim_not_found(self):
            stage = self._make_stage()
            stage.define_prim("/World/Sphere", "Sphere")
            stage.remove_prim("/World/Sphere")
            assert stage.has_prim("/World/Sphere") is False

        def test_prims_of_type_returns_list(self):
            stage = self._make_stage()
            stage.define_prim("/World/Sphere1", "Sphere")
            stage.define_prim("/World/Sphere2", "Sphere")
            spheres = stage.prims_of_type("Sphere")
            assert isinstance(spheres, list)
            assert len(spheres) >= 2

        def test_prims_of_type_empty_for_missing(self):
            stage = self._make_stage()
            results = stage.prims_of_type("NonExistentType")
            assert isinstance(results, list)
            assert len(results) == 0

    class TestSerialization:
        def _make_stage(self, name: str = "test") -> UsdStage:
            return scene_info_json_to_stage(json.dumps({"name": name, "fps": 24.0, "objects": [], "metadata": {}}))

        def test_to_json_returns_str(self):
            stage = self._make_stage()
            j = stage.to_json()
            assert isinstance(j, str)

        def test_to_json_is_valid_json(self):
            stage = self._make_stage()
            j = stage.to_json()
            data = json.loads(j)
            assert isinstance(data, dict)

        def test_to_json_contains_name(self):
            stage = self._make_stage("serialize_test")
            j = stage.to_json()
            data = json.loads(j)
            assert data.get("name") == "serialize_test"

        def test_stage_to_scene_info_json_returns_str(self):
            stage = self._make_stage()
            j = stage_to_scene_info_json(stage)
            assert isinstance(j, str)

        def test_stage_to_scene_info_json_is_valid_json(self):
            stage = self._make_stage()
            j = stage_to_scene_info_json(stage)
            data = json.loads(j)
            assert isinstance(data, dict)

        def test_scene_info_roundtrip_name(self):
            stage = self._make_stage("roundtrip_test")
            j = stage_to_scene_info_json(stage)
            data = json.loads(j)
            assert data.get("name") == "roundtrip_test"

        def test_export_usda_returns_str(self):
            stage = self._make_stage()
            usda = stage.export_usda()
            assert isinstance(usda, str)
            assert len(usda) > 0

        def test_traverse_returns_list(self):
            stage = self._make_stage()
            stage.define_prim("/World/A", "Mesh")
            result = stage.traverse()
            assert isinstance(result, list)

    class TestMetrics:
        def _make_stage(self) -> UsdStage:
            return scene_info_json_to_stage(
                json.dumps({"name": "metrics_test", "fps": 24.0, "objects": [], "metadata": {}})
            )

        def test_start_time_code(self):
            stage = self._make_stage()
            # start_time_code may be None if not set
            assert stage.start_time_code is None or isinstance(stage.start_time_code, (int, float))

        def test_end_time_code(self):
            stage = self._make_stage()
            # end_time_code may be None if not set
            assert stage.end_time_code is None or isinstance(stage.end_time_code, (int, float))

        def test_meters_per_unit(self):
            stage = self._make_stage()
            assert isinstance(stage.meters_per_unit, float)

        def test_set_meters_per_unit(self):
            stage = self._make_stage()
            stage.set_meters_per_unit(1.0)
            assert stage.meters_per_unit == 1.0

        def test_metrics_is_dict(self):
            stage = self._make_stage()
            metrics = stage.metrics()
            assert isinstance(metrics, dict)

        def test_id_is_str(self):
            stage = self._make_stage()
            assert isinstance(stage.id, str)
            assert len(stage.id) > 0
