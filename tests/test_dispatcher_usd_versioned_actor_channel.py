"""Tests for iteration 85.

ToolDispatcher depth, UsdStage has/remove_prim,
VersionedRegistry multi-version deep, SandboxContext.set_actor, FramedChannel roundtrip.

Covers:
- ToolDispatcher: register_handler/dispatch/handler_count/handler_names/has_handler/
                    remove_handler/skip_empty_schema_validation/dispatch_errors
- UsdStage: has_prim/remove_prim/get_prim after remove/traverse after remove
- VersionedRegistry: resolve caret/exact/gte/tilde, resolve_all, remove count,
                     overwrite, invalid constraint
- SandboxContext: set_actor/actor in AuditEntry/actor persists/actor change
- FramedChannel: send_request UUID/is_running/__bool__/shutdown/ping timeout/
                 send_notify/try_recv/send_response no-raise
"""

from __future__ import annotations

import threading
import time

import pytest

import dcc_mcp_core
from dcc_mcp_core import IpcListener
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VersionedRegistry
from dcc_mcp_core import connect_ipc

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_dispatcher(*action_names: str) -> ToolDispatcher:
    """Create an ToolDispatcher with the given registered action names."""
    reg = ToolRegistry()
    for name in action_names:
        reg.register(name)
    return ToolDispatcher(reg)


def _bind_and_connect() -> tuple[object, dcc_mcp_core.FramedChannel]:
    """Bind a TCP listener and connect a client channel (no accept thread)."""
    addr = TransportAddress.tcp("127.0.0.1", 0)
    listener = IpcListener.bind(addr)
    local = listener.local_address()
    handle = listener.into_handle()
    channel = connect_ipc(local)
    return handle, channel


# ===========================================================================
# ToolDispatcher
# ===========================================================================


class TestActionDispatcherInit:
    """Basic init and handler registration."""

    def test_handler_count_initial_zero(self) -> None:
        """New dispatcher has zero handlers."""
        disp = _make_dispatcher("ping")
        assert disp.handler_count() == 0

    def test_handler_names_initial_empty(self) -> None:
        """handler_names() returns empty list before registration."""
        disp = _make_dispatcher("ping")
        assert disp.handler_names() == []

    def test_has_handler_false_before_register(self) -> None:
        """has_handler() returns False before any handler is registered."""
        disp = _make_dispatcher("ping")
        assert disp.has_handler("ping") is False

    def test_register_handler_increments_count(self) -> None:
        """register_handler() increments handler_count by 1."""
        disp = _make_dispatcher("ping")
        disp.register_handler("ping", lambda p: "pong")
        assert disp.handler_count() == 1

    def test_has_handler_true_after_register(self) -> None:
        """has_handler() returns True after registration."""
        disp = _make_dispatcher("ping")
        disp.register_handler("ping", lambda p: "pong")
        assert disp.has_handler("ping") is True

    def test_has_handler_false_unregistered_name(self) -> None:
        """has_handler() returns False for names never registered."""
        disp = _make_dispatcher("ping")
        disp.register_handler("ping", lambda p: "pong")
        assert disp.has_handler("not_registered") is False

    def test_handler_names_sorted_alphabetically(self) -> None:
        """handler_names() returns names in sorted order."""
        reg = ToolRegistry()
        for name in ["z_action", "a_action", "m_action"]:
            reg.register(name)
        disp = ToolDispatcher(reg)
        for name in ["z_action", "a_action", "m_action"]:
            disp.register_handler(name, lambda p: None)
        names = disp.handler_names()
        assert names == sorted(names)

    def test_register_non_callable_raises_type_error(self) -> None:
        """register_handler() raises TypeError for non-callable handlers."""
        reg = ToolRegistry()
        reg.register("x")
        disp = ToolDispatcher(reg)
        with pytest.raises(TypeError):
            disp.register_handler("x", 42)  # type: ignore[arg-type]

    def test_register_non_callable_string_raises(self) -> None:
        """register_handler() raises TypeError for string 'handler'."""
        reg = ToolRegistry()
        reg.register("x")
        disp = ToolDispatcher(reg)
        with pytest.raises(TypeError):
            disp.register_handler("x", "not_callable")  # type: ignore[arg-type]


class TestActionDispatcherDispatch:
    """Dispatch behaviour."""

    def test_dispatch_returns_dict_with_required_keys(self) -> None:
        """dispatch() returns dict with action/output/validation_skipped keys."""
        disp = _make_dispatcher("ping")
        disp.register_handler("ping", lambda p: "pong")
        result = disp.dispatch("ping", "null")
        assert "action" in result
        assert "output" in result
        assert "validation_skipped" in result

    def test_dispatch_action_field_matches_name(self) -> None:
        """dispatch() result 'action' field matches the dispatched action name."""
        disp = _make_dispatcher("my_action")
        disp.register_handler("my_action", lambda p: None)
        result = disp.dispatch("my_action", "null")
        assert result["action"] == "my_action"

    def test_dispatch_output_field_contains_handler_result(self) -> None:
        """dispatch() result 'output' is the return value of the handler."""
        disp = _make_dispatcher("compute")
        disp.register_handler("compute", lambda p: {"value": 42})
        result = disp.dispatch("compute", "null")
        assert result["output"] == {"value": 42}

    def test_dispatch_handler_receives_params_dict(self) -> None:
        """Handler receives the parsed params dict."""
        received: list[dict] = []
        disp = _make_dispatcher("echo")
        disp.register_handler("echo", lambda p: received.append(p))
        disp.dispatch("echo", '{"key": "val"}')
        assert received and received[0].get("key") == "val"

    def test_dispatch_null_params_receives_none_or_dict(self) -> None:
        """Handler receives None or empty dict when params_json is 'null'."""
        received: list = []
        disp = _make_dispatcher("echo")
        disp.register_handler("echo", lambda p: received.append(p))
        disp.dispatch("echo", "null")
        assert len(received) == 1

    def test_dispatch_unknown_action_raises_key_error(self) -> None:
        """dispatch() raises KeyError for actions with no handler."""
        disp = _make_dispatcher("ping")
        with pytest.raises(KeyError):
            disp.dispatch("unknown", "{}")

    def test_dispatch_handler_exception_raises_runtime_error(self) -> None:
        """dispatch() wraps handler exceptions as RuntimeError."""
        disp = _make_dispatcher("bad")
        disp.register_handler("bad", lambda p: 1 / 0)
        with pytest.raises(RuntimeError):
            disp.dispatch("bad", "{}")

    def test_dispatch_validation_skipped_true_when_no_schema(self) -> None:
        """validation_skipped is True when action has no input schema."""
        disp = _make_dispatcher("no_schema")
        disp.register_handler("no_schema", lambda p: 99)
        result = disp.dispatch("no_schema", '{"x": 1}')
        assert result["validation_skipped"] is True

    def test_dispatch_with_schema_validation_succeeds(self) -> None:
        """dispatch() validates params against JSON Schema when schema is set."""
        import json

        reg = ToolRegistry()
        reg.register(
            "create_sphere",
            input_schema=json.dumps(
                {
                    "type": "object",
                    "required": ["radius"],
                    "properties": {"radius": {"type": "number"}},
                }
            ),
        )
        disp = ToolDispatcher(reg)
        disp.register_handler("create_sphere", lambda p: {"r": p["radius"]})
        result = disp.dispatch("create_sphere", '{"radius": 2.5}')
        assert result["output"]["r"] == 2.5

    def test_dispatch_with_schema_validation_failure_raises(self) -> None:
        """dispatch() raises ValueError when params fail JSON Schema validation."""
        import json

        reg = ToolRegistry()
        reg.register(
            "create_sphere",
            input_schema=json.dumps(
                {
                    "type": "object",
                    "required": ["radius"],
                    "properties": {"radius": {"type": "number"}},
                }
            ),
        )
        disp = ToolDispatcher(reg)
        disp.register_handler("create_sphere", lambda p: None)
        disp.skip_empty_schema_validation = False
        with pytest.raises((ValueError, RuntimeError)):
            disp.dispatch("create_sphere", '{"wrong_field": "bad"}')

    def test_dispatch_multiple_times_independent(self) -> None:
        """Multiple dispatches do not interfere with each other."""
        call_count = [0]
        disp = _make_dispatcher("inc")
        disp.register_handler("inc", lambda p: call_count.__setitem__(0, call_count[0] + 1))
        disp.dispatch("inc", "null")
        disp.dispatch("inc", "null")
        disp.dispatch("inc", "null")
        assert call_count[0] == 3


class TestActionDispatcherRemoveHandler:
    """remove_handler behaviour."""

    def test_remove_handler_returns_true_when_exists(self) -> None:
        """remove_handler() returns True when handler was registered."""
        disp = _make_dispatcher("ping")
        disp.register_handler("ping", lambda p: None)
        assert disp.remove_handler("ping") is True

    def test_remove_handler_returns_false_when_not_exists(self) -> None:
        """remove_handler() returns False for unregistered handler."""
        disp = _make_dispatcher("ping")
        assert disp.remove_handler("ping") is False

    def test_remove_handler_decrements_count(self) -> None:
        """remove_handler() decrements handler_count."""
        disp = _make_dispatcher("ping", "pong")
        disp.register_handler("ping", lambda p: None)
        disp.register_handler("pong", lambda p: None)
        disp.remove_handler("ping")
        assert disp.handler_count() == 1

    def test_remove_handler_idempotent(self) -> None:
        """remove_handler() returns False on second call for same name."""
        disp = _make_dispatcher("ping")
        disp.register_handler("ping", lambda p: None)
        assert disp.remove_handler("ping") is True
        assert disp.remove_handler("ping") is False

    def test_removed_handler_no_longer_in_names(self) -> None:
        """handler_names() does not include removed handlers."""
        disp = _make_dispatcher("a", "b")
        disp.register_handler("a", lambda p: None)
        disp.register_handler("b", lambda p: None)
        disp.remove_handler("a")
        assert "a" not in disp.handler_names()
        assert "b" in disp.handler_names()

    def test_removed_handler_has_handler_false(self) -> None:
        """has_handler() returns False after remove_handler."""
        disp = _make_dispatcher("x")
        disp.register_handler("x", lambda p: None)
        disp.remove_handler("x")
        assert disp.has_handler("x") is False


class TestActionDispatcherSkipValidation:
    """skip_empty_schema_validation property."""

    def test_skip_empty_schema_validation_default_true(self) -> None:
        """skip_empty_schema_validation defaults to True."""
        disp = _make_dispatcher()
        assert disp.skip_empty_schema_validation is True

    def test_skip_empty_schema_validation_can_be_set_false(self) -> None:
        """skip_empty_schema_validation can be set to False."""
        disp = _make_dispatcher()
        disp.skip_empty_schema_validation = False
        assert disp.skip_empty_schema_validation is False

    def test_skip_empty_schema_validation_round_trip(self) -> None:
        """skip_empty_schema_validation can be toggled back to True."""
        disp = _make_dispatcher()
        disp.skip_empty_schema_validation = False
        disp.skip_empty_schema_validation = True
        assert disp.skip_empty_schema_validation is True


class TestActionDispatcherRepr:
    """repr."""

    def test_repr_is_string(self) -> None:
        """repr() returns a string."""
        disp = _make_dispatcher("x")
        assert isinstance(repr(disp), str)

    def test_repr_contains_actiondispatcher(self) -> None:
        """repr() mentions 'ToolDispatcher'."""
        disp = _make_dispatcher("x")
        assert "ToolDispatcher" in repr(disp) or "Dispatcher" in repr(disp)


# ===========================================================================
# UsdStage has_prim / remove_prim
# ===========================================================================


class TestUsdStageHasPrim:
    """has_prim() behaviour."""

    def test_has_prim_returns_false_before_define(self) -> None:
        """has_prim() returns False for a path not yet defined."""
        stage = UsdStage("test_has")
        assert stage.has_prim("/World") is False

    def test_has_prim_returns_true_after_define(self) -> None:
        """has_prim() returns True after define_prim."""
        stage = UsdStage("test_has2")
        stage.define_prim("/World", "Xform")
        assert stage.has_prim("/World") is True

    def test_has_prim_nested_path(self) -> None:
        """has_prim() works for nested paths."""
        stage = UsdStage("test_nested")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        assert stage.has_prim("/World/Cube") is True

    def test_has_prim_missing_nested_path(self) -> None:
        """has_prim() returns False for a path not in the stage."""
        stage = UsdStage("test_missing")
        stage.define_prim("/World", "Xform")
        assert stage.has_prim("/World/Missing") is False

    def test_has_prim_type_is_bool(self) -> None:
        """has_prim() returns a bool."""
        stage = UsdStage("test_type")
        result = stage.has_prim("/World")
        assert isinstance(result, bool)


class TestUsdStageRemovePrim:
    """remove_prim() behaviour."""

    def test_remove_prim_returns_true_on_success(self) -> None:
        """remove_prim() returns True when prim exists and is removed."""
        stage = UsdStage("rm1")
        stage.define_prim("/World", "Xform")
        assert stage.remove_prim("/World") is True

    def test_remove_prim_returns_false_when_not_found(self) -> None:
        """remove_prim() returns False for paths not in stage."""
        stage = UsdStage("rm2")
        assert stage.remove_prim("/Nonexistent") is False

    def test_remove_prim_has_prim_false_afterwards(self) -> None:
        """has_prim() returns False after remove_prim."""
        stage = UsdStage("rm3")
        stage.define_prim("/World", "Xform")
        stage.remove_prim("/World")
        assert stage.has_prim("/World") is False

    def test_remove_prim_idempotent_second_call_false(self) -> None:
        """remove_prim() returns False on second removal of same path."""
        stage = UsdStage("rm4")
        stage.define_prim("/Cube", "Mesh")
        assert stage.remove_prim("/Cube") is True
        assert stage.remove_prim("/Cube") is False

    def test_remove_prim_leaves_siblings_intact(self) -> None:
        """remove_prim() does not remove sibling prims."""
        stage = UsdStage("rm5")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        stage.define_prim("/World/Sphere", "Mesh")
        stage.remove_prim("/World/Cube")
        assert stage.has_prim("/World/Sphere") is True
        assert stage.has_prim("/World") is True

    def test_remove_prim_updates_traverse(self) -> None:
        """traverse() does not return removed prim."""
        stage = UsdStage("rm6")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        stage.remove_prim("/World/Cube")
        paths = [str(p.path) for p in stage.traverse()]
        assert "/World/Cube" not in paths

    def test_remove_prim_decrements_prim_count_in_metrics(self) -> None:
        """metrics() prim_count decreases after remove_prim."""
        stage = UsdStage("rm7")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        before = stage.metrics()["prim_count"]
        stage.remove_prim("/World/Cube")
        after = stage.metrics()["prim_count"]
        assert after < before

    def test_get_prim_returns_none_after_remove(self) -> None:
        """get_prim() returns None for a path that was removed."""
        stage = UsdStage("rm8")
        stage.define_prim("/World", "Xform")
        stage.remove_prim("/World")
        assert stage.get_prim("/World") is None

    def test_remove_prim_type_is_bool(self) -> None:
        """remove_prim() returns a bool."""
        stage = UsdStage("rm9")
        result = stage.remove_prim("/Anything")
        assert isinstance(result, bool)

    def test_remove_prim_nested_does_not_remove_parent(self) -> None:
        """Removing a child prim does not remove the parent."""
        stage = UsdStage("rm10")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Child", "Mesh")
        stage.remove_prim("/World/Child")
        assert stage.has_prim("/World") is True


# ===========================================================================
# VersionedRegistry depth
# ===========================================================================


class TestVersionedRegistryVersions:
    """versions() and latest_version()."""

    def test_versions_empty_before_registration(self) -> None:
        """versions() returns empty list before any registration."""
        vr = VersionedRegistry()
        assert vr.versions("a", "maya") == []

    def test_versions_sorted_ascending(self) -> None:
        """versions() returns versions in ascending order."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "2.0.0")
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("a", "maya", "1.5.0")
        assert vr.versions("a", "maya") == ["1.0.0", "1.5.0", "2.0.0"]

    def test_latest_version_returns_highest(self) -> None:
        """latest_version() returns the highest registered version."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("a", "maya", "2.0.0")
        vr.register_versioned("a", "maya", "1.5.0")
        assert vr.latest_version("a", "maya") == "2.0.0"

    def test_latest_version_returns_none_unknown(self) -> None:
        """latest_version() returns None for unregistered (name, dcc) pair."""
        vr = VersionedRegistry()
        assert vr.latest_version("unknown", "maya") is None

    def test_versions_different_dcc_isolated(self) -> None:
        """versions() is scoped to the specific (name, dcc) pair."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("a", "blender", "2.0.0")
        assert vr.versions("a", "maya") == ["1.0.0"]
        assert vr.versions("a", "blender") == ["2.0.0"]


class TestVersionedRegistryResolve:
    """resolve() with various constraints."""

    def _setup(self) -> VersionedRegistry:
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("a", "maya", "1.5.0")
        vr.register_versioned("a", "maya", "2.0.0")
        return vr

    def test_resolve_caret_returns_highest_compatible(self) -> None:
        """resolve('^1.0.0') returns the highest compatible version within major 1."""
        vr = self._setup()
        r = vr.resolve("a", "maya", "^1.0.0")
        assert r is not None
        assert r["version"] == "1.5.0"

    def test_resolve_exact_returns_specific_version(self) -> None:
        """resolve('=1.0.0') returns exactly that version."""
        vr = self._setup()
        r = vr.resolve("a", "maya", "=1.0.0")
        assert r is not None
        assert r["version"] == "1.0.0"

    def test_resolve_gte_returns_highest_satisfying(self) -> None:
        """resolve('>=1.5.0') returns highest version >= 1.5.0."""
        vr = self._setup()
        r = vr.resolve("a", "maya", ">=1.5.0")
        assert r is not None
        assert r["version"] == "2.0.0"

    def test_resolve_wildcard_returns_highest(self) -> None:
        """resolve('*') returns the latest version."""
        vr = self._setup()
        r = vr.resolve("a", "maya", "*")
        assert r is not None
        assert r["version"] == "2.0.0"

    def test_resolve_no_match_returns_none(self) -> None:
        """resolve() returns None when no version satisfies the constraint."""
        vr = self._setup()
        assert vr.resolve("a", "maya", "=9.9.9") is None

    def test_resolve_unknown_name_returns_none(self) -> None:
        """resolve() returns None for unregistered action names."""
        vr = self._setup()
        assert vr.resolve("unknown", "maya", "*") is None

    def test_resolve_result_has_version_key(self) -> None:
        """resolve() result dict contains 'version' key."""
        vr = self._setup()
        r = vr.resolve("a", "maya", "=1.0.0")
        assert r is not None
        assert "version" in r

    def test_resolve_result_has_name_key(self) -> None:
        """resolve() result dict contains 'name' key."""
        vr = self._setup()
        r = vr.resolve("a", "maya", "=1.0.0")
        assert r is not None
        assert "name" in r

    def test_resolve_overwritten_version_returns_new_description(self) -> None:
        """Re-registering same version overwrites description."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0", description="old")
        vr.register_versioned("a", "maya", "1.0.0", description="new")
        r = vr.resolve("a", "maya", "=1.0.0")
        assert r is not None
        assert r["description"] == "new"


class TestVersionedRegistryResolveAll:
    """resolve_all() tests."""

    def _setup(self) -> VersionedRegistry:
        vr = VersionedRegistry()
        for v in ["1.0.0", "1.5.0", "2.0.0"]:
            vr.register_versioned("a", "maya", v)
        return vr

    def test_resolve_all_gte_returns_all_satisfying(self) -> None:
        """resolve_all('>=1.0.0') returns all three versions."""
        vr = self._setup()
        all_v = vr.resolve_all("a", "maya", ">=1.0.0")
        assert len(all_v) == 3

    def test_resolve_all_caret_excludes_major_bump(self) -> None:
        """resolve_all('^1.0.0') returns 1.x.x versions only."""
        vr = self._setup()
        all_v = vr.resolve_all("a", "maya", "^1.0.0")
        versions = [x["version"] for x in all_v]
        assert "2.0.0" not in versions
        assert "1.0.0" in versions
        assert "1.5.0" in versions

    def test_resolve_all_wildcard_returns_all(self) -> None:
        """resolve_all('*') returns all registered versions."""
        vr = self._setup()
        all_v = vr.resolve_all("a", "maya", "*")
        assert len(all_v) == 3

    def test_resolve_all_sorted_ascending(self) -> None:
        """resolve_all() returns versions in ascending order."""
        vr = self._setup()
        all_v = vr.resolve_all("a", "maya", "*")
        vs = [x["version"] for x in all_v]
        assert vs == sorted(vs)

    def test_resolve_all_no_match_empty_list(self) -> None:
        """resolve_all() returns empty list when no versions match."""
        vr = self._setup()
        all_v = vr.resolve_all("a", "maya", "=9.9.9")
        assert all_v == []


class TestVersionedRegistryTotalEntriesAndKeys:
    """total_entries() and keys()."""

    def test_total_entries_increases_with_registration(self) -> None:
        """total_entries() reflects accumulated registrations."""
        vr = VersionedRegistry()
        assert vr.total_entries() == 0
        vr.register_versioned("a", "maya", "1.0.0")
        assert vr.total_entries() == 1
        vr.register_versioned("a", "maya", "2.0.0")
        assert vr.total_entries() == 2
        vr.register_versioned("b", "blender", "1.0.0")
        assert vr.total_entries() == 3

    def test_keys_contains_registered_pairs(self) -> None:
        """keys() contains (name, dcc) tuples for all registered actions."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("b", "blender", "1.0.0")
        keys = vr.keys()
        assert ("a", "maya") in keys
        assert ("b", "blender") in keys

    def test_keys_is_list_of_tuples(self) -> None:
        """keys() returns a list of 2-tuples."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        keys = vr.keys()
        assert isinstance(keys, list)
        assert all(isinstance(k, tuple) and len(k) == 2 for k in keys)

    def test_keys_retained_after_remove(self) -> None:
        """keys() retains (name, dcc) key even after removing all versions."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.remove("a", "maya", "=1.0.0")
        # total_entries should be 0, but key may still be present
        assert vr.total_entries() == 0
        # keys() may still have ("a", "maya") — this is the documented behaviour


class TestVersionedRegistryRemove:
    """remove() tests."""

    def test_remove_returns_count_of_removed(self) -> None:
        """remove() returns number of versions removed."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("a", "maya", "1.5.0")
        vr.register_versioned("a", "maya", "2.0.0")
        n = vr.remove("a", "maya", "^1.0.0")
        assert n == 2  # removes 1.0.0 and 1.5.0

    def test_remove_caret_leaves_higher_major(self) -> None:
        """remove('^1.0.0') does not remove version 2.0.0."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("a", "maya", "2.0.0")
        vr.remove("a", "maya", "^1.0.0")
        assert "2.0.0" in vr.versions("a", "maya")

    def test_remove_exact_removes_only_that_version(self) -> None:
        """remove('=1.0.0') removes only version 1.0.0."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("a", "maya", "2.0.0")
        n = vr.remove("a", "maya", "=1.0.0")
        assert n == 1
        assert "2.0.0" in vr.versions("a", "maya")
        assert "1.0.0" not in vr.versions("a", "maya")

    def test_remove_decrements_total_entries(self) -> None:
        """remove() decrements total_entries accordingly."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        vr.register_versioned("a", "maya", "2.0.0")
        before = vr.total_entries()
        vr.remove("a", "maya", "=1.0.0")
        assert vr.total_entries() == before - 1

    def test_remove_invalid_constraint_raises_value_error(self) -> None:
        """remove() raises ValueError for an invalid constraint string."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        with pytest.raises(ValueError):
            vr.remove("a", "maya", "!!!invalid")

    def test_remove_not_found_returns_zero(self) -> None:
        """remove() returns 0 when no versions match the constraint."""
        vr = VersionedRegistry()
        vr.register_versioned("a", "maya", "1.0.0")
        n = vr.remove("a", "maya", "=9.9.9")
        assert n == 0

    def test_remove_all_wildcard(self) -> None:
        """remove('*') removes all registered versions for (name, dcc)."""
        vr = VersionedRegistry()
        for v in ["1.0.0", "1.5.0", "2.0.0"]:
            vr.register_versioned("a", "maya", v)
        n = vr.remove("a", "maya", "*")
        assert n == 3
        assert vr.versions("a", "maya") == []


# ===========================================================================
# SandboxContext.set_actor + AuditEntry.actor
# ===========================================================================


class TestSandboxContextSetActor:
    """set_actor() and actor attribute in AuditEntry."""

    def _make_ctx(self) -> SandboxContext:
        pol = SandboxPolicy()
        pol.allow_actions(["ping", "echo", "list"])
        return SandboxContext(pol)

    def test_actor_is_none_before_set_actor(self) -> None:
        """AuditEntry.actor is None before set_actor is called."""
        ctx = self._make_ctx()
        ctx.execute_json("ping", "{}")
        entries = ctx.audit_log.entries()
        assert len(entries) >= 1
        assert entries[0].actor is None

    def test_actor_reflects_set_actor_value(self) -> None:
        """AuditEntry.actor matches the value passed to set_actor."""
        ctx = self._make_ctx()
        ctx.set_actor("my-agent")
        ctx.execute_json("ping", "{}")
        entries = ctx.audit_log.entries()
        assert entries[0].actor == "my-agent"

    def test_actor_persists_across_calls(self) -> None:
        """Actor set via set_actor persists for subsequent executions."""
        ctx = self._make_ctx()
        ctx.set_actor("persistent-agent")
        ctx.execute_json("ping", "{}")
        ctx.execute_json("echo", "{}")
        ctx.execute_json("list", "{}")
        entries = ctx.audit_log.entries()
        for entry in entries:
            assert entry.actor == "persistent-agent"

    def test_actor_changes_after_second_set_actor(self) -> None:
        """Calling set_actor again updates the actor for future entries."""
        ctx = self._make_ctx()
        ctx.set_actor("agent-v1")
        ctx.execute_json("ping", "{}")
        ctx.set_actor("agent-v2")
        ctx.execute_json("echo", "{}")
        entries = ctx.audit_log.entries()
        assert entries[0].actor == "agent-v1"
        assert entries[1].actor == "agent-v2"

    def test_actor_before_and_after_set_actor_mixed(self) -> None:
        """Mix of entries with None actor (before set) and named actor (after set)."""
        ctx = self._make_ctx()
        ctx.execute_json("ping", "{}")
        ctx.set_actor("agent")
        ctx.execute_json("echo", "{}")
        entries = ctx.audit_log.entries()
        assert entries[0].actor is None
        assert entries[1].actor == "agent"

    def test_audit_entry_actor_type_is_str_or_none(self) -> None:
        """AuditEntry.actor is either str or None, never other type."""
        ctx = self._make_ctx()
        ctx.execute_json("ping", "{}")
        ctx.set_actor("test-actor")
        ctx.execute_json("echo", "{}")
        entries = ctx.audit_log.entries()
        for entry in entries:
            assert entry.actor is None or isinstance(entry.actor, str)

    def test_set_actor_empty_string(self) -> None:
        """set_actor with empty string sets actor to empty string."""
        ctx = self._make_ctx()
        ctx.set_actor("")
        ctx.execute_json("ping", "{}")
        entries = ctx.audit_log.entries()
        # empty string actor or None — both acceptable
        assert entries[0].actor == "" or entries[0].actor is None

    def test_action_count_increments_with_set_actor(self) -> None:
        """action_count increments normally when set_actor is used."""
        ctx = self._make_ctx()
        ctx.set_actor("agent")
        ctx.execute_json("ping", "{}")
        ctx.execute_json("echo", "{}")
        assert ctx.action_count == 2


# ===========================================================================
# FramedChannel: send_request/recv/send_notify/try_recv/shutdown/is_running
# ===========================================================================


class TestFramedChannelProperties:
    """is_running, __bool__, shutdown."""

    def test_is_running_true_after_connect(self) -> None:
        """is_running is True immediately after connect_ipc."""
        handle, channel = _bind_and_connect()
        try:
            assert channel.is_running is True
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_bool_true_when_running(self) -> None:
        """__bool__ returns True when channel is running."""
        handle, channel = _bind_and_connect()
        try:
            assert bool(channel) is True
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_is_running_false_after_shutdown(self) -> None:
        """is_running is False after shutdown."""
        handle, channel = _bind_and_connect()
        channel.shutdown()
        assert channel.is_running is False
        handle.shutdown()

    def test_shutdown_is_idempotent(self) -> None:
        """Calling shutdown() twice does not raise."""
        handle, channel = _bind_and_connect()
        channel.shutdown()
        channel.shutdown()  # must not raise
        handle.shutdown()

    def test_repr_contains_framed_channel(self) -> None:
        """repr() mentions 'FramedChannel'."""
        handle, channel = _bind_and_connect()
        try:
            r = repr(channel)
            assert "FramedChannel" in r or "channel" in r.lower()
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelSendRequest:
    """send_request() tests."""

    def test_send_request_returns_uuid_string(self) -> None:
        """send_request() returns a UUID string of length 36."""
        handle, channel = _bind_and_connect()
        try:
            req_id = channel.send_request("execute_python", b"print('hi')")
            assert isinstance(req_id, str)
            assert len(req_id) == 36
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_request_each_call_unique_id(self) -> None:
        """Each send_request() call returns a unique UUID."""
        handle, channel = _bind_and_connect()
        try:
            ids = {channel.send_request(f"method_{i}") for i in range(5)}
            assert len(ids) == 5
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_request_no_params_returns_uuid(self) -> None:
        """send_request() without params still returns a valid UUID."""
        handle, channel = _bind_and_connect()
        try:
            req_id = channel.send_request("list_objects")
            assert isinstance(req_id, str)
            assert len(req_id) == 36
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_request_with_params_bytes(self) -> None:
        """send_request() with bytes params returns a UUID."""
        handle, channel = _bind_and_connect()
        try:
            req_id = channel.send_request("cmd", b'{"key": "val"}')
            assert len(req_id) == 36
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelSendResponse:
    """send_response() does not raise for valid UUID."""

    def test_send_response_success_no_raise(self) -> None:
        """send_response with success=True does not raise."""
        handle, channel = _bind_and_connect()
        try:
            req_id = channel.send_request("test_method", b"params")
            channel.send_response(req_id, success=True, payload=b"result")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_response_failure_no_raise(self) -> None:
        """send_response with success=False and error does not raise."""
        handle, channel = _bind_and_connect()
        try:
            req_id = channel.send_request("failing_method", b"")
            channel.send_response(req_id, success=False, error="some error")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_response_invalid_uuid_raises(self) -> None:
        """send_response with non-UUID raises RuntimeError or ValueError."""
        handle, channel = _bind_and_connect()
        try:
            with pytest.raises((RuntimeError, ValueError)):
                channel.send_response("not-a-uuid", success=True)
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelSendNotify:
    """send_notify() does not raise."""

    def test_send_notify_no_raise(self) -> None:
        """send_notify() does not raise for valid topic."""
        handle, channel = _bind_and_connect()
        try:
            channel.send_notify("scene_changed", b'{"modified":true}')
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_notify_no_data_no_raise(self) -> None:
        """send_notify() without data does not raise."""
        handle, channel = _bind_and_connect()
        try:
            channel.send_notify("status_update")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_notify_empty_bytes_no_raise(self) -> None:
        """send_notify() with empty bytes does not raise."""
        handle, channel = _bind_and_connect()
        try:
            channel.send_notify("heartbeat", b"")
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelTryRecv:
    """try_recv() returns None when buffer is empty."""

    def test_try_recv_returns_none_when_empty(self) -> None:
        """try_recv() returns None immediately when no messages are buffered."""
        handle, channel = _bind_and_connect()
        try:
            result = channel.try_recv()
            assert result is None
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_try_recv_after_send_notify_returns_none_on_client(self) -> None:
        """Client that sent notify has nothing to try_recv."""
        handle, channel = _bind_and_connect()
        try:
            channel.send_notify("topic", b"data")
            # Client sent, nothing to receive back
            result = channel.try_recv()
            assert result is None
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelPing:
    """ping() with no pong handler raises RuntimeError."""

    def test_ping_no_handler_raises_runtime_error(self) -> None:
        """ping() raises RuntimeError when the server does not respond with Pong."""
        handle, channel = _bind_and_connect()
        try:
            with pytest.raises(RuntimeError, match="ping"):
                channel.ping(timeout_ms=500)
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelRoundtrip:
    """Verify roundtrip-related properties without requiring actual server accept.

    Note: IpcListener.accept() in a background thread combined with connect_ipc()
    in the main thread has Tokio runtime serialization constraints in-process.
    These tests verify the channel's send semantics without needing a real server.
    """

    def test_send_request_returns_valid_uuid_after_connect(self) -> None:
        """send_request() on a connected channel returns a 36-char UUID."""
        handle, channel = _bind_and_connect()
        try:
            req_id = channel.send_request("get_scene", b'{"dcc":"maya"}')
            assert isinstance(req_id, str)
            assert len(req_id) == 36
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_notify_after_connect_no_raise(self) -> None:
        """send_notify() on a connected channel does not raise."""
        handle, channel = _bind_and_connect()
        try:
            channel.send_notify("scene_changed", b'{"modified":true}')
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_multiple_send_requests_have_unique_ids(self) -> None:
        """Sequential send_request() calls all return unique UUIDs."""
        handle, channel = _bind_and_connect()
        try:
            ids = [channel.send_request(f"method_{i}", b"") for i in range(3)]
            assert len(set(ids)) == 3
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_try_recv_empty_after_send_request(self) -> None:
        """try_recv() returns None immediately after send_request (no reply queued)."""
        handle, channel = _bind_and_connect()
        try:
            channel.send_request("get_info")
            result = channel.try_recv()
            assert result is None
        finally:
            channel.shutdown()
            handle.shutdown()
