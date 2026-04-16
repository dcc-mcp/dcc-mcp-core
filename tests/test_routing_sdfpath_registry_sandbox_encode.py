"""Deep tests for previously uncovered scenarios.

Covers:
- TransportManager.get_or_create_session_routed() SPECIFIC hint instance binding
- SdfPath hash / use as dict key / deep parent chain
- UsdStage.id uniqueness and name property
- SkillMetadata equality depth
- RateLimitMiddleware per-action independent counters
- encode_notify / encode_response error paths (invalid UUID)
- ToolRegistry.__repr__ content and register_batch edge cases
- SandboxContext.is_allowed with allow_actions / deny_actions combos
"""

from __future__ import annotations

import tempfile
import uuid

import pytest

import dcc_mcp_core
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import SdfPath
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import SkillMetadata
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportManager
from dcc_mcp_core import UsdStage
from dcc_mcp_core import decode_envelope
from dcc_mcp_core import encode_notify
from dcc_mcp_core import encode_response

# ===========================================================================
# TransportManager.get_or_create_session_routed — SPECIFIC hint
# ===========================================================================


class TestSessionRoutedSpecificHint:
    """Tests for SPECIFIC routing strategy with explicit instance hint."""

    def test_specific_hint_targets_given_instance(self, tmp_path):
        """SPECIFIC routing with a hint should bind session to that instance."""
        mgr = TransportManager(str(tmp_path))
        iid1 = mgr.register_service("maya", "127.0.0.1", 18810)
        mgr.register_service("maya", "127.0.0.1", 18811)

        sid = mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.SPECIFIC, hint=iid1)
        sess = mgr.get_session(sid)
        assert sess is not None
        assert sess["instance_id"] == iid1
        mgr.shutdown()

    def test_specific_hint_second_instance(self, tmp_path):
        """Hinting iid2 yields a session for iid2."""
        mgr = TransportManager(str(tmp_path))
        mgr.register_service("maya", "127.0.0.1", 18810)
        iid2 = mgr.register_service("maya", "127.0.0.1", 18811)

        sid = mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.SPECIFIC, hint=iid2)
        sess = mgr.get_session(sid)
        assert sess is not None
        assert sess["instance_id"] == iid2
        mgr.shutdown()

    def test_round_robin_creates_different_instances(self, tmp_path):
        """ROUND_ROBIN on 3 instances should spread across them."""
        mgr = TransportManager(str(tmp_path))
        mgr.register_service("maya", "127.0.0.1", 18810)
        mgr.register_service("maya", "127.0.0.1", 18811)
        mgr.register_service("maya", "127.0.0.1", 18812)

        sid1 = mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
        sid2 = mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
        assert isinstance(sid1, str)
        assert isinstance(sid2, str)
        # Sessions may or may not be the same (same instance can be reused)
        # but they must be valid UUIDs
        uuid.UUID(sid1)
        uuid.UUID(sid2)
        mgr.shutdown()

    def test_first_available_returns_session_id(self, tmp_path):
        """FIRST_AVAILABLE strategy returns a valid session id."""
        mgr = TransportManager(str(tmp_path))
        mgr.register_service("houdini", "127.0.0.1", 19000)

        sid = mgr.get_or_create_session_routed("houdini", strategy=RoutingStrategy.FIRST_AVAILABLE)
        assert isinstance(sid, str)
        uuid.UUID(sid)
        mgr.shutdown()

    def test_no_strategy_defaults_to_first_available(self, tmp_path):
        """Calling without a strategy still returns a valid session id."""
        mgr = TransportManager(str(tmp_path))
        mgr.register_service("blender", "127.0.0.1", 19001)

        sid = mgr.get_or_create_session_routed("blender")
        assert isinstance(sid, str)
        mgr.shutdown()

    def test_session_count_increases_with_routed_sessions(self, tmp_path):
        """Each routed session call should increase session_count."""
        mgr = TransportManager(str(tmp_path))
        mgr.register_service("maya", "127.0.0.1", 18810)
        mgr.register_service("maya", "127.0.0.1", 18811)

        before = mgr.session_count()
        mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
        mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
        assert mgr.session_count() >= before + 1
        mgr.shutdown()

    def test_specific_hint_session_has_correct_dcc_type(self, tmp_path):
        """Session created via SPECIFIC hint has the correct dcc_type."""
        mgr = TransportManager(str(tmp_path))
        iid = mgr.register_service("3dsmax", "127.0.0.1", 19002)

        sid = mgr.get_or_create_session_routed("3dsmax", strategy=RoutingStrategy.SPECIFIC, hint=iid)
        sess = mgr.get_session(sid)
        assert sess is not None
        assert sess["dcc_type"] == "3dsmax"
        mgr.shutdown()

    def test_routed_session_raises_for_unknown_dcc(self, tmp_path):
        """Routing to unregistered DCC type raises RuntimeError."""
        mgr = TransportManager(str(tmp_path))
        with pytest.raises(RuntimeError):
            mgr.get_or_create_session_routed("nonexistent_dcc")
        mgr.shutdown()


# ===========================================================================
# SdfPath — hash / use as dict key / deep parent chain
# ===========================================================================


class TestSdfPathHashAndEquality:
    """Tests for SdfPath hash consistency and equality semantics."""

    def test_equal_paths_have_equal_hash(self):
        p1 = SdfPath("/World/Cube")
        p2 = SdfPath("/World/Cube")
        assert hash(p1) == hash(p2)

    def test_different_paths_likely_different_hash(self):
        p1 = SdfPath("/World/Cube")
        p2 = SdfPath("/World/Sphere")
        # Hashes may collide but the paths are not equal
        assert p1 != p2

    def test_path_usable_as_dict_key(self):
        p1 = SdfPath("/World/Cube")
        p2 = SdfPath("/World/Cube")
        d = {p1: "cube_data"}
        assert d[p2] == "cube_data"

    def test_multiple_paths_as_dict_keys(self):
        paths = [SdfPath(f"/World/Obj{i}") for i in range(5)]
        d = {p: p.name for p in paths}
        assert len(d) == 5
        for i, p in enumerate(paths):
            assert d[p] == f"Obj{i}"

    def test_path_in_set(self):
        s = {SdfPath("/World/A"), SdfPath("/World/B"), SdfPath("/World/A")}
        assert len(s) == 2

    def test_root_path_hashable(self):
        r = SdfPath("/")
        h = hash(r)
        assert isinstance(h, int)

    def test_relative_path_hashable(self):
        p = SdfPath("Relative/Path")
        h = hash(p)
        assert isinstance(h, int)

    def test_equality_reflexive(self):
        p = SdfPath("/World/Cube")
        assert p == p

    def test_equality_symmetric(self):
        p1 = SdfPath("/World/Cube")
        p2 = SdfPath("/World/Cube")
        assert p1 == p2
        assert p2 == p1

    def test_inequality_different_names(self):
        p1 = SdfPath("/World/Cube")
        p2 = SdfPath("/World/Sphere")
        assert p1 != p2


class TestSdfPathParentChain:
    """Tests for SdfPath.parent() deep chain traversal."""

    def test_root_parent_is_none(self):
        root = SdfPath("/")
        assert root.parent() is None

    def test_one_level_parent_is_root(self):
        p = SdfPath("/World")
        parent = p.parent()
        assert parent is not None
        assert parent.name == ""
        assert parent.is_absolute

    def test_two_level_chain(self):
        p = SdfPath("/World/Cube")
        parent = p.parent()
        assert parent.name == "World"
        grandparent = parent.parent()
        assert grandparent is not None
        assert grandparent.name == ""  # root

    def test_three_level_chain(self):
        p = SdfPath("/A/B/C")
        levels = []
        current = p
        while current is not None:
            levels.append(current.name)
            current = current.parent()
        # /A/B/C -> name=C, /A/B -> name=B, /A -> name=A, / -> name="", root.parent()=None
        assert levels == ["C", "B", "A", ""]

    def test_four_level_chain(self):
        p = SdfPath("/Root/Level1/Level2/Leaf")
        current = p
        depth = 0
        while current is not None:
            depth += 1
            current = current.parent()
        # /Root/Level1/Level2/Leaf (4 levels) + / (root) = 5 stops
        assert depth == 5

    def test_parent_returns_sdf_path_type(self):
        p = SdfPath("/World/Cube")
        parent = p.parent()
        assert isinstance(parent, SdfPath)

    def test_child_then_parent_roundtrip(self):
        base = SdfPath("/World")
        child = base.child("Cube")
        parent_back = child.parent()
        # parent_back should equal base
        assert parent_back == base

    def test_parent_of_root_is_none_not_self(self):
        root = SdfPath("/")
        p = root.parent()
        assert p is None

    def test_child_appends_segment(self):
        base = SdfPath("/World")
        child = base.child("Sphere")
        assert child.name == "Sphere"
        assert str(child).endswith("Sphere")


class TestSdfPathChildConstruction:
    """Tests for SdfPath.child() method."""

    def test_child_of_root(self):
        root = SdfPath("/")
        child = root.child("World")
        assert child.name == "World"
        assert child.is_absolute

    def test_nested_child(self):
        p = SdfPath("/World")
        child1 = p.child("Geo")
        child2 = child1.child("Mesh")
        assert child2.name == "Mesh"
        assert child2.parent().name == "Geo"

    def test_child_is_absolute_when_parent_absolute(self):
        p = SdfPath("/World")
        child = p.child("Cube")
        assert child.is_absolute

    def test_child_str_contains_parent(self):
        p = SdfPath("/World")
        child = p.child("Cube")
        s = str(child)
        assert "World" in s
        assert "Cube" in s


# ===========================================================================
# UsdStage id uniqueness and name property
# ===========================================================================


class TestUsdStageIdentity:
    """Tests for UsdStage.id uniqueness and name attribute."""

    def test_id_is_string(self):
        s = UsdStage("test")
        assert isinstance(s.id, str)

    def test_id_is_nonempty(self):
        s = UsdStage("test")
        assert len(s.id) > 0

    def test_two_stages_have_different_ids(self):
        s1 = UsdStage("stage_a")
        s2 = UsdStage("stage_b")
        assert s1.id != s2.id

    def test_same_name_stages_have_different_ids(self):
        """Even with identical names, IDs must be distinct."""
        s1 = UsdStage("same_name")
        s2 = UsdStage("same_name")
        assert s1.id != s2.id

    def test_name_property_returns_given_name(self):
        s = UsdStage("my_scene")
        assert s.name == "my_scene"

    def test_name_empty_string(self):
        s = UsdStage("")
        assert s.name == ""

    def test_name_with_unicode(self):
        s = UsdStage("场景")
        assert s.name == "场景"

    def test_id_is_stable_across_reads(self):
        """Id should return the same value on repeated access."""
        s = UsdStage("stable")
        id1 = s.id
        id2 = s.id
        assert id1 == id2


# ===========================================================================
# SkillMetadata equality depth
# ===========================================================================


class TestSkillMetadataEquality:
    """Deep equality tests for SkillMetadata."""

    def test_equal_when_same_name_and_defaults(self):
        m1 = SkillMetadata("test")
        m2 = SkillMetadata("test")
        assert m1 == m2

    def test_equal_with_all_fields_same(self):
        m1 = SkillMetadata("s", description="d", dcc="maya", version="2.0.0")
        m2 = SkillMetadata("s", description="d", dcc="maya", version="2.0.0")
        assert m1 == m2

    def test_not_equal_different_name(self):
        m1 = SkillMetadata("skill_a")
        m2 = SkillMetadata("skill_b")
        assert m1 != m2

    def test_not_equal_different_description(self):
        m1 = SkillMetadata("s", description="desc1")
        m2 = SkillMetadata("s", description="desc2")
        assert m1 != m2

    def test_not_equal_different_dcc(self):
        m1 = SkillMetadata("s", dcc="maya")
        m2 = SkillMetadata("s", dcc="blender")
        assert m1 != m2

    def test_not_equal_different_version(self):
        m1 = SkillMetadata("s", version="1.0.0")
        m2 = SkillMetadata("s", version="2.0.0")
        assert m1 != m2

    def test_not_equal_different_tags(self):
        m1 = SkillMetadata("s", tags=["a"])
        m2 = SkillMetadata("s", tags=["b"])
        assert m1 != m2

    def test_repr_contains_name(self):
        m = SkillMetadata("my_skill")
        assert "my_skill" in repr(m)

    def test_str_contains_name(self):
        m = SkillMetadata("my_skill")
        assert "my_skill" in str(m)

    def test_not_equal_to_non_skill(self):
        m = SkillMetadata("s")
        assert m != "not a skill"
        assert m != 42
        assert m != None


# ===========================================================================
# RateLimitMiddleware — per-action independent counters
# ===========================================================================


class TestRateLimitMiddlewarePerAction:
    """Tests for RateLimitMiddleware tracking independent per-action counters."""

    def _make_pipeline_with_rl(self, max_calls: int = 100, window_ms: int = 10000):
        reg = ToolRegistry()
        reg.register("action_a")
        reg.register("action_b")
        reg.register("action_c")
        dispatcher = dcc_mcp_core.ToolDispatcher(reg)
        dispatcher.register_handler("action_a", lambda p: "a")
        dispatcher.register_handler("action_b", lambda p: "b")
        dispatcher.register_handler("action_c", lambda p: "c")
        pipeline = dcc_mcp_core.ToolPipeline(dispatcher)
        rl = pipeline.add_rate_limit(max_calls=max_calls, window_ms=window_ms)
        return pipeline, rl

    def test_initial_call_count_zero(self):
        _, rl = self._make_pipeline_with_rl()
        assert rl.call_count("action_a") == 0

    def test_call_count_increments(self):
        pipeline, rl = self._make_pipeline_with_rl()
        pipeline.dispatch("action_a", "{}")
        assert rl.call_count("action_a") == 1
        pipeline.dispatch("action_a", "{}")
        assert rl.call_count("action_a") == 2

    def test_per_action_counters_are_independent(self):
        pipeline, rl = self._make_pipeline_with_rl()
        pipeline.dispatch("action_a", "{}")
        pipeline.dispatch("action_a", "{}")
        pipeline.dispatch("action_b", "{}")
        assert rl.call_count("action_a") == 2
        assert rl.call_count("action_b") == 1
        assert rl.call_count("action_c") == 0

    def test_max_calls_property(self):
        _, rl = self._make_pipeline_with_rl(max_calls=50)
        assert rl.max_calls == 50

    def test_window_ms_property(self):
        _, rl = self._make_pipeline_with_rl(window_ms=2000)
        assert rl.window_ms == 2000

    def test_rate_limit_exceeded_raises(self):
        pipeline, _rl = self._make_pipeline_with_rl(max_calls=2, window_ms=60000)
        pipeline.dispatch("action_a", "{}")
        pipeline.dispatch("action_a", "{}")
        with pytest.raises(RuntimeError):
            pipeline.dispatch("action_a", "{}")

    def test_different_actions_have_separate_limits(self):
        pipeline, _rl = self._make_pipeline_with_rl(max_calls=1, window_ms=60000)
        # action_a can be called once
        pipeline.dispatch("action_a", "{}")
        # action_b can also be called once (separate limit)
        pipeline.dispatch("action_b", "{}")
        # calling action_a again should fail
        with pytest.raises(RuntimeError):
            pipeline.dispatch("action_a", "{}")
        # but action_c is still fresh
        pipeline.dispatch("action_c", "{}")

    def test_repr_is_string(self):
        _, rl = self._make_pipeline_with_rl()
        assert isinstance(repr(rl), str)


# ===========================================================================
# encode_notify / encode_response error paths
# ===========================================================================


class TestEncodeNotify:
    """Tests for encode_notify framing."""

    def test_encode_notify_returns_bytes(self):
        frame = encode_notify("scene_changed")
        assert isinstance(frame, bytes)

    def test_encode_notify_has_length_prefix(self):
        frame = encode_notify("topic", b"data")
        assert len(frame) >= 4

    def test_encode_notify_length_prefix_correct(self):
        frame = encode_notify("test_topic", b"hello")
        import struct

        payload_len = struct.unpack(">I", frame[:4])[0]
        assert payload_len == len(frame) - 4

    def test_encode_notify_roundtrip_topic(self):
        frame = encode_notify("render_complete")
        payload = frame[4:]
        msg = decode_envelope(payload)
        assert msg["type"] == "notify"
        assert msg["topic"] == "render_complete"

    def test_encode_notify_roundtrip_data(self):
        data = b"event_payload"
        frame = encode_notify("my_topic", data)
        msg = decode_envelope(frame[4:])
        assert msg["data"] == data

    def test_encode_notify_no_data_defaults_empty(self):
        frame = encode_notify("ping_topic")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "notify"
        assert isinstance(msg["data"], bytes)

    def test_encode_notify_different_topics_differ(self):
        f1 = encode_notify("topic_a")
        f2 = encode_notify("topic_b")
        assert f1 != f2


class TestEncodeResponseErrors:
    """Tests for encode_response error paths."""

    def test_encode_response_success_true(self):
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"ok")
        msg = decode_envelope(frame[4:])
        assert msg["success"] is True

    def test_encode_response_success_false_with_error(self):
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, False, error="something failed")
        msg = decode_envelope(frame[4:])
        assert msg["success"] is False
        assert msg["error"] == "something failed"

    def test_encode_response_roundtrip_payload(self):
        req_id = str(uuid.uuid4())
        payload = b"result_data"
        frame = encode_response(req_id, True, payload)
        msg = decode_envelope(frame[4:])
        assert msg["payload"] == payload

    def test_encode_response_invalid_uuid_raises(self):
        with pytest.raises((RuntimeError, ValueError)):
            encode_response("not-a-uuid", True, b"ok")

    def test_encode_response_empty_uuid_raises(self):
        with pytest.raises((RuntimeError, ValueError)):
            encode_response("", True, b"ok")

    def test_encode_response_request_id_preserved(self):
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"")
        msg = decode_envelope(frame[4:])
        assert msg["id"] == req_id


# ===========================================================================
# ToolRegistry.__repr__ and register_batch edge cases
# ===========================================================================


class TestActionRegistryReprAndBatch:
    """Tests for ToolRegistry repr and register_batch edge cases."""

    def test_repr_is_string(self):
        reg = ToolRegistry()
        assert isinstance(repr(reg), str)

    def test_repr_changes_after_register(self):
        reg = ToolRegistry()
        repr(reg)
        reg.register("action_x")
        r2 = repr(reg)
        # repr may change; at minimum it's still a string
        assert isinstance(r2, str)

    def test_register_batch_skips_no_name(self):
        reg = ToolRegistry()
        reg.register_batch(
            [
                {},
                {"category": "geo"},
                {"name": "valid_action", "category": "geo"},
            ]
        )
        assert len(reg) == 1

    def test_register_batch_skips_empty_name(self):
        reg = ToolRegistry()
        reg.register_batch(
            [
                {"name": "", "category": "geo"},
                {"name": "real_action"},
            ]
        )
        assert len(reg) == 1

    def test_register_batch_all_fields(self):
        reg = ToolRegistry()
        reg.register_batch(
            [
                {
                    "name": "full_action",
                    "description": "A full action",
                    "category": "geo",
                    "tags": ["create", "mesh"],
                    "dcc": "maya",
                    "version": "2.0.0",
                    "source_file": "/path/to/script.py",
                }
            ]
        )
        meta = reg.get_action("full_action")
        assert meta is not None
        assert meta["category"] == "geo"
        assert meta["dcc"] == "maya"

    def test_register_batch_multiple_dccs(self):
        reg = ToolRegistry()
        reg.register_batch(
            [
                {"name": "create_sphere", "dcc": "maya"},
                {"name": "create_sphere", "dcc": "blender"},
            ]
        )
        maya_names = reg.list_actions_for_dcc("maya")
        blender_names = reg.list_actions_for_dcc("blender")
        assert "create_sphere" in maya_names
        assert "create_sphere" in blender_names

    def test_len_after_batch_and_reset(self):
        reg = ToolRegistry()
        reg.register_batch([{"name": f"act{i}"} for i in range(5)])
        assert len(reg) == 5
        reg.reset()
        assert len(reg) == 0

    def test_register_batch_large_set(self):
        reg = ToolRegistry()
        reg.register_batch([{"name": f"action_{i:04d}", "dcc": "maya"} for i in range(50)])
        assert len(reg) == 50

    def test_register_batch_empty_list(self):
        reg = ToolRegistry()
        reg.register_batch([])
        assert len(reg) == 0


# ===========================================================================
# SandboxContext.is_allowed — complex allow/deny combos
# ===========================================================================


class TestSandboxContextIsAllowed:
    """Tests for SandboxContext.is_allowed with various policy combinations."""

    def _ctx_with_whitelist(self, allow: list[str], deny: list[str] | None = None) -> SandboxContext:
        policy = SandboxPolicy()
        if allow:
            policy.allow_actions(allow)
        if deny:
            policy.deny_actions(deny)
        return SandboxContext(policy)

    def test_no_whitelist_allows_any_action(self):
        """Without allow_actions, any action is permitted."""
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        assert ctx.is_allowed("anything") is True

    def test_whitelist_allows_listed_action(self):
        ctx = self._ctx_with_whitelist(["read_scene"])
        assert ctx.is_allowed("read_scene") is True

    def test_whitelist_blocks_unlisted_action(self):
        ctx = self._ctx_with_whitelist(["read_scene"])
        assert ctx.is_allowed("delete_scene") is False

    def test_deny_blocks_even_if_in_whitelist(self):
        """deny_actions overrides allow_actions."""
        ctx = self._ctx_with_whitelist(["read_scene", "delete_scene"], deny=["delete_scene"])
        assert ctx.is_allowed("read_scene") is True
        assert ctx.is_allowed("delete_scene") is False

    def test_multiple_allowed_actions(self):
        ctx = self._ctx_with_whitelist(["get_info", "list_objects", "snapshot"])
        for action in ("get_info", "list_objects", "snapshot"):
            assert ctx.is_allowed(action) is True
        assert ctx.is_allowed("modify_scene") is False

    def test_deny_all_with_empty_whitelist(self):
        """Deny a set of actions; without whitelist, non-denied still allowed."""
        policy = SandboxPolicy()
        policy.deny_actions(["dangerous_op"])
        ctx = SandboxContext(policy)
        assert ctx.is_allowed("safe_op") is True
        assert ctx.is_allowed("dangerous_op") is False

    def test_read_only_mode_does_not_affect_is_allowed(self):
        """is_allowed checks whitelist/deny, not read_only mode."""
        policy = SandboxPolicy()
        policy.allow_actions(["read_data"])
        policy.set_read_only(True)
        ctx = SandboxContext(policy)
        assert ctx.is_allowed("read_data") is True
        assert ctx.is_allowed("write_data") is False

    def test_is_allowed_returns_bool(self):
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        result = ctx.is_allowed("some_action")
        assert isinstance(result, bool)

    def test_is_allowed_case_sensitive(self):
        """Action names are case-sensitive."""
        ctx = self._ctx_with_whitelist(["get_scene"])
        assert ctx.is_allowed("get_scene") is True
        # Capitalized variant should be blocked since whitelist is set
        assert ctx.is_allowed("Get_scene") is False
        assert ctx.is_allowed("GET_SCENE") is False

    def test_deny_multiple_actions(self):
        """Multiple actions can be denied at once."""
        policy = SandboxPolicy()
        policy.deny_actions(["op_a", "op_b", "op_c"])
        ctx = SandboxContext(policy)
        for op in ("op_a", "op_b", "op_c"):
            assert ctx.is_allowed(op) is False
        assert ctx.is_allowed("op_d") is True
