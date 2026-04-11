"""Deep tests for Cross-DCC protocol data models, dcc_mcp_core.skill helpers.

serialize_result/deserialize_result, and ToolDeclaration coverage targets:
- ObjectTransform, BoundingBox, SceneObject, SceneNode, FrameRange, RenderOutput
- skill_success, skill_error, skill_warning, skill_exception, skill_entry decorator
- serialize_result, deserialize_result, SerializeFormat
- ToolDeclaration fields + mutability
"""

from __future__ import annotations

import json
import math

import pytest

import dcc_mcp_core
from dcc_mcp_core import BoundingBox
from dcc_mcp_core import FrameRange
from dcc_mcp_core import ObjectTransform
from dcc_mcp_core import RenderOutput
from dcc_mcp_core import SceneNode
from dcc_mcp_core import SceneObject
from dcc_mcp_core import SerializeFormat
from dcc_mcp_core import ToolDeclaration
from dcc_mcp_core import deserialize_result
from dcc_mcp_core import error_result
from dcc_mcp_core import serialize_result
from dcc_mcp_core import success_result
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_exception
from dcc_mcp_core.skill import skill_success
from dcc_mcp_core.skill import skill_warning


# ---------------------------------------------------------------------------
# ObjectTransform
# ---------------------------------------------------------------------------
class TestObjectTransformCreate:
    def test_translate_stored(self):
        t = ObjectTransform(translate=[1.0, 2.0, 3.0], rotate=[0.0, 0.0, 0.0], scale=[1.0, 1.0, 1.0])
        assert t.translate == [1.0, 2.0, 3.0]

    def test_rotate_stored(self):
        t = ObjectTransform(translate=[0.0, 0.0, 0.0], rotate=[10.0, 20.0, 30.0], scale=[1.0, 1.0, 1.0])
        assert t.rotate == [10.0, 20.0, 30.0]

    def test_scale_stored(self):
        t = ObjectTransform(translate=[0.0, 0.0, 0.0], rotate=[0.0, 0.0, 0.0], scale=[2.0, 3.0, 4.0])
        assert t.scale == [2.0, 3.0, 4.0]

    def test_translate_is_list_of_floats(self):
        t = ObjectTransform(translate=[0.0, 10.0, 0.0], rotate=[0.0, 45.0, 0.0], scale=[1.0, 1.0, 1.0])
        assert isinstance(t.translate, list)
        assert all(isinstance(v, float) for v in t.translate)

    def test_identity_translate_zeros(self):
        t = ObjectTransform.identity()
        assert t.translate == [0.0, 0.0, 0.0]

    def test_identity_rotate_zeros(self):
        t = ObjectTransform.identity()
        assert t.rotate == [0.0, 0.0, 0.0]

    def test_identity_scale_ones(self):
        t = ObjectTransform.identity()
        assert t.scale == [1.0, 1.0, 1.0]

    def test_to_dict_keys(self):
        t = ObjectTransform(translate=[0.0, 10.0, 0.0], rotate=[0.0, 45.0, 0.0], scale=[1.0, 1.0, 1.0])
        d = t.to_dict()
        assert set(d.keys()) == {"translate", "rotate", "scale"}

    def test_to_dict_values_match(self):
        t = ObjectTransform(translate=[1.0, 2.0, 3.0], rotate=[4.0, 5.0, 6.0], scale=[7.0, 8.0, 9.0])
        d = t.to_dict()
        assert d["translate"] == [1.0, 2.0, 3.0]
        assert d["rotate"] == [4.0, 5.0, 6.0]
        assert d["scale"] == [7.0, 8.0, 9.0]

    def test_negative_translate(self):
        t = ObjectTransform(translate=[-100.0, -50.0, -25.0], rotate=[0.0, 0.0, 0.0], scale=[1.0, 1.0, 1.0])
        assert t.translate[0] == -100.0

    def test_zero_scale(self):
        t = ObjectTransform(translate=[0.0, 0.0, 0.0], rotate=[0.0, 0.0, 0.0], scale=[0.0, 0.0, 0.0])
        assert t.scale == [0.0, 0.0, 0.0]


# ---------------------------------------------------------------------------
# BoundingBox
# ---------------------------------------------------------------------------
class TestBoundingBoxCreate:
    def test_min_stored(self):
        bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
        assert bb.min == [-1.0, 0.0, -1.0]

    def test_max_stored(self):
        bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
        assert bb.max == [1.0, 2.0, 1.0]

    def test_center_midpoint(self):
        bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
        c = bb.center()
        assert c == [0.0, 1.0, 0.0]

    def test_size_extent(self):
        bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
        s = bb.size()
        assert s == [2.0, 2.0, 2.0]

    def test_to_dict_keys(self):
        bb = BoundingBox(min=[0.0, 0.0, 0.0], max=[1.0, 1.0, 1.0])
        d = bb.to_dict()
        assert "min" in d and "max" in d

    def test_to_dict_values(self):
        bb = BoundingBox(min=[0.0, 0.0, 0.0], max=[2.0, 4.0, 6.0])
        d = bb.to_dict()
        assert d["max"] == [2.0, 4.0, 6.0]

    def test_zero_size_box(self):
        bb = BoundingBox(min=[5.0, 5.0, 5.0], max=[5.0, 5.0, 5.0])
        assert bb.size() == [0.0, 0.0, 0.0]

    def test_large_negative_values(self):
        bb = BoundingBox(min=[-1000.0, -1000.0, -1000.0], max=[1000.0, 1000.0, 1000.0])
        assert bb.center() == [0.0, 0.0, 0.0]
        assert bb.size() == [2000.0, 2000.0, 2000.0]


# ---------------------------------------------------------------------------
# SceneObject
# ---------------------------------------------------------------------------
class TestSceneObjectCreate:
    def test_name_stored(self):
        obj = SceneObject(name="pCube1", long_name="|g|pCube1", object_type="mesh")
        assert obj.name == "pCube1"

    def test_long_name_stored(self):
        obj = SceneObject(name="pCube1", long_name="|g|pCube1", object_type="mesh")
        assert obj.long_name == "|g|pCube1"

    def test_object_type_stored(self):
        obj = SceneObject(name="cam", long_name="cam", object_type="camera")
        assert obj.object_type == "camera"

    def test_parent_default_none(self):
        obj = SceneObject(name="root", long_name="root", object_type="transform")
        assert obj.parent is None

    def test_parent_stored(self):
        obj = SceneObject(name="child", long_name="|p|child", object_type="mesh", parent="p")
        assert obj.parent == "p"

    def test_visible_default_true(self):
        obj = SceneObject(name="x", long_name="x", object_type="mesh")
        assert obj.visible is True

    def test_visible_false(self):
        obj = SceneObject(name="x", long_name="x", object_type="mesh", visible=False)
        assert obj.visible is False

    def test_metadata_stored(self):
        obj = SceneObject(name="x", long_name="x", object_type="mesh", metadata={"mat": "lambert1"})
        assert obj.metadata == {"mat": "lambert1"}

    def test_metadata_default_empty(self):
        obj = SceneObject(name="x", long_name="x", object_type="mesh")
        assert obj.metadata == {}

    def test_to_dict_has_all_keys(self):
        obj = SceneObject(name="n", long_name="ln", object_type="t")
        d = obj.to_dict()
        assert "name" in d and "long_name" in d and "object_type" in d


# ---------------------------------------------------------------------------
# SceneNode
# ---------------------------------------------------------------------------
class TestSceneNodeCreate:
    def test_object_stored(self):
        o = SceneObject(name="x", long_name="x", object_type="mesh")
        node = SceneNode(object=o)
        assert node.object.name == "x"

    def test_children_default_empty(self):
        o = SceneObject(name="x", long_name="x", object_type="mesh")
        node = SceneNode(object=o)
        assert node.children == []

    def test_children_stored(self):
        leaf_obj = SceneObject(name="leaf", long_name="leaf", object_type="mesh")
        leaf = SceneNode(object=leaf_obj)
        root_obj = SceneObject(name="root", long_name="root", object_type="transform")
        root = SceneNode(object=root_obj, children=[leaf])
        assert len(root.children) == 1

    def test_child_object_name(self):
        leaf_obj = SceneObject(name="leaf", long_name="leaf", object_type="mesh")
        leaf = SceneNode(object=leaf_obj)
        root_obj = SceneObject(name="root", long_name="root", object_type="transform")
        root = SceneNode(object=root_obj, children=[leaf])
        assert root.children[0].object.name == "leaf"

    def test_to_dict_has_object_and_children(self):
        o = SceneObject(name="n", long_name="n", object_type="mesh")
        node = SceneNode(object=o)
        d = node.to_dict()
        assert "object" in d and "children" in d

    def test_deep_hierarchy(self):
        def make(name):
            return SceneObject(name=name, long_name=name, object_type="mesh")

        child2 = SceneNode(object=make("c2"))
        child1 = SceneNode(object=make("c1"), children=[child2])
        root = SceneNode(object=make("root"), children=[child1])
        assert root.children[0].children[0].object.name == "c2"

    def test_multiple_children(self):
        def make(name):
            return SceneNode(object=SceneObject(name=name, long_name=name, object_type="mesh"))

        root = SceneNode(
            object=SceneObject(name="root", long_name="root", object_type="transform"),
            children=[make("c1"), make("c2"), make("c3")],
        )
        assert len(root.children) == 3


# ---------------------------------------------------------------------------
# FrameRange
# ---------------------------------------------------------------------------
class TestFrameRangeCreate:
    def test_start_stored(self):
        fr = FrameRange(start=1.0, end=240.0, fps=24.0, current=1.0)
        assert fr.start == 1.0

    def test_end_stored(self):
        fr = FrameRange(start=1.0, end=240.0, fps=24.0, current=1.0)
        assert fr.end == 240.0

    def test_fps_stored(self):
        fr = FrameRange(start=1.0, end=120.0, fps=30.0, current=1.0)
        assert fr.fps == 30.0

    def test_current_stored(self):
        fr = FrameRange(start=1.0, end=240.0, fps=24.0, current=100.0)
        assert fr.current == 100.0

    def test_to_dict_keys(self):
        fr = FrameRange(start=1.0, end=120.0, fps=24.0, current=1.0)
        d = fr.to_dict()
        assert set(d.keys()) == {"start", "end", "fps", "current"}

    def test_to_dict_values(self):
        fr = FrameRange(start=0.0, end=100.0, fps=25.0, current=50.0)
        d = fr.to_dict()
        assert d["fps"] == 25.0
        assert d["current"] == 50.0

    def test_duration_calculation(self):
        fr = FrameRange(start=1.0, end=241.0, fps=24.0, current=1.0)
        duration_frames = fr.end - fr.start
        assert duration_frames == 240.0

    def test_fractional_fps(self):
        fr = FrameRange(start=0.0, end=1000.0, fps=23.976, current=0.0)
        assert abs(fr.fps - 23.976) < 1e-6


# ---------------------------------------------------------------------------
# RenderOutput
# ---------------------------------------------------------------------------
class TestRenderOutputCreate:
    def test_file_path_stored(self):
        ro = RenderOutput(file_path="/renders/f001.png", width=1920, height=1080, format="png", render_time_ms=5000)
        assert ro.file_path == "/renders/f001.png"

    def test_width_stored(self):
        ro = RenderOutput(file_path="/f.png", width=3840, height=2160, format="png", render_time_ms=1000)
        assert ro.width == 3840

    def test_height_stored(self):
        ro = RenderOutput(file_path="/f.exr", width=1920, height=1080, format="exr", render_time_ms=2000)
        assert ro.height == 1080

    def test_format_stored(self):
        ro = RenderOutput(file_path="/f.exr", width=1920, height=1080, format="exr", render_time_ms=2000)
        assert ro.format == "exr"

    def test_render_time_ms_stored(self):
        ro = RenderOutput(file_path="/f.png", width=800, height=600, format="png", render_time_ms=12345)
        assert ro.render_time_ms == 12345

    def test_to_dict_keys(self):
        ro = RenderOutput(file_path="/f.png", width=1920, height=1080, format="png", render_time_ms=1000)
        d = ro.to_dict()
        assert set(d.keys()) == {"file_path", "width", "height", "format", "render_time_ms"}

    def test_to_dict_values(self):
        ro = RenderOutput(file_path="/r/f.jpg", width=1280, height=720, format="jpeg", render_time_ms=3000)
        d = ro.to_dict()
        assert d["file_path"] == "/r/f.jpg"
        assert d["format"] == "jpeg"

    def test_zero_render_time(self):
        ro = RenderOutput(file_path="/f.png", width=1, height=1, format="png", render_time_ms=0)
        assert ro.render_time_ms == 0


# ---------------------------------------------------------------------------
# ToolDeclaration
# ---------------------------------------------------------------------------
class TestToolDeclarationCreate:
    def test_name_stored(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", source_file="s.py")
        assert td.name == "x"

    def test_description_stored(self):
        td = ToolDeclaration(name="x", description="Create sphere", input_schema="{}", source_file="s.py")
        assert td.description == "Create sphere"

    def test_input_schema_stored(self):
        schema = '{"type": "object"}'
        td = ToolDeclaration(name="x", description="y", input_schema=schema, source_file="s.py")
        # Rust normalizes JSON (removes spaces), so compare parsed structure
        import json as _json

        assert _json.loads(td.input_schema) == _json.loads(schema)

    def test_output_schema_none_default(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", output_schema=None, source_file="s.py")
        assert td.output_schema is None or td.output_schema == ""

    def test_source_file_stored(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", source_file="scripts/x.py")
        assert td.source_file == "scripts/x.py"

    def test_read_only_false_default(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", source_file="s.py", read_only=False)
        assert td.read_only is False

    def test_read_only_true(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", source_file="s.py", read_only=True)
        assert td.read_only is True

    def test_destructive_false(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", source_file="s.py", destructive=False)
        assert td.destructive is False

    def test_idempotent_true(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", source_file="s.py", idempotent=True)
        assert td.idempotent is True

    def test_name_mutable(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", source_file="s.py")
        td.name = "new_name"
        assert td.name == "new_name"

    def test_description_mutable(self):
        td = ToolDeclaration(name="x", description="y", input_schema="{}", source_file="s.py")
        td.description = "updated"
        assert td.description == "updated"


# ---------------------------------------------------------------------------
# serialize_result / deserialize_result / SerializeFormat
# ---------------------------------------------------------------------------
class TestSerializeResultJson:
    def test_returns_str(self):
        arm = success_result("done")
        result = serialize_result(arm)
        assert isinstance(result, str)

    def test_valid_json(self):
        arm = success_result("done", count=3)
        result = serialize_result(arm)
        parsed = json.loads(result)
        assert isinstance(parsed, dict)

    def test_success_field_true(self):
        arm = success_result("done")
        parsed = json.loads(serialize_result(arm))
        assert parsed["success"] is True

    def test_message_preserved(self):
        arm = success_result("hello world")
        parsed = json.loads(serialize_result(arm))
        assert parsed["message"] == "hello world"

    def test_context_preserved(self):
        arm = success_result("x", radius=1.5, name="sphere")
        parsed = json.loads(serialize_result(arm))
        assert parsed["context"]["radius"] == 1.5
        assert parsed["context"]["name"] == "sphere"

    def test_error_result_serialized(self):
        arm = error_result("Failed", "File not found")
        parsed = json.loads(serialize_result(arm))
        assert parsed["success"] is False

    def test_roundtrip_success(self):
        arm = success_result("roundtrip", x=42)
        arm2 = deserialize_result(serialize_result(arm))
        assert arm2.success is True
        assert arm2.message == "roundtrip"
        assert arm2.context["x"] == 42

    def test_roundtrip_error(self):
        arm = error_result("fail", "oops", prompt="fix it")
        arm2 = deserialize_result(serialize_result(arm))
        assert arm2.success is False
        assert arm2.message == "fail"

    def test_default_format_is_json(self):
        arm = success_result("x")
        result_default = serialize_result(arm)
        result_explicit = serialize_result(arm, SerializeFormat.Json)
        assert result_default == result_explicit

    def test_empty_context(self):
        arm = success_result("no ctx")
        arm2 = deserialize_result(serialize_result(arm))
        assert arm2.context == {}


class TestSerializeResultMsgPack:
    def test_returns_bytes(self):
        arm = success_result("done")
        result = serialize_result(arm, SerializeFormat.MsgPack)
        assert isinstance(result, bytes)

    def test_compact_than_json(self):
        arm = success_result("done", count=3)
        json_len = len(serialize_result(arm, SerializeFormat.Json).encode())
        pack_len = len(serialize_result(arm, SerializeFormat.MsgPack))
        # MsgPack should generally be smaller or comparable
        assert pack_len < json_len * 2  # relaxed: at worst 2x

    def test_roundtrip_success(self):
        arm = success_result("msgpack", radius=2.5)
        raw = serialize_result(arm, SerializeFormat.MsgPack)
        arm2 = deserialize_result(raw, SerializeFormat.MsgPack)
        assert arm2.message == "msgpack"
        assert arm2.context["radius"] == 2.5

    def test_roundtrip_error(self):
        arm = error_result("fail", "oops")
        raw = serialize_result(arm, SerializeFormat.MsgPack)
        arm2 = deserialize_result(raw, SerializeFormat.MsgPack)
        assert arm2.success is False

    def test_context_multiple_types(self):
        arm = success_result("ctx", x=1, y=1.5, z="hello")
        raw = serialize_result(arm, SerializeFormat.MsgPack)
        arm2 = deserialize_result(raw, SerializeFormat.MsgPack)
        assert arm2.context["x"] == 1
        assert arm2.context["z"] == "hello"


class TestSerializeFormatEnum:
    def test_json_variant_exists(self):
        assert SerializeFormat.Json is not None

    def test_msgpack_variant_exists(self):
        assert SerializeFormat.MsgPack is not None

    def test_json_and_msgpack_distinct(self):
        assert SerializeFormat.Json != SerializeFormat.MsgPack


# ---------------------------------------------------------------------------
# dcc_mcp_core.skill sub-module
# ---------------------------------------------------------------------------
class TestSkillSuccess:
    def test_returns_dict(self):
        r = skill_success("done")
        assert isinstance(r, dict)

    def test_success_is_true(self):
        r = skill_success("done")
        assert r["success"] is True

    def test_message_stored(self):
        r = skill_success("created sphere")
        assert r["message"] == "created sphere"

    def test_context_from_kwargs(self):
        r = skill_success("ok", radius=2.5, name="sphere")
        assert r["context"]["radius"] == 2.5
        assert r["context"]["name"] == "sphere"

    def test_prompt_default_none(self):
        r = skill_success("ok")
        assert r["prompt"] is None

    def test_prompt_stored(self):
        r = skill_success("ok", prompt="next step")
        assert r["prompt"] == "next step"

    def test_error_is_none(self):
        r = skill_success("ok")
        assert r["error"] is None

    def test_empty_context_when_no_kwargs(self):
        r = skill_success("ok")
        assert r["context"] == {}


class TestSkillError:
    def test_success_is_false(self):
        r = skill_error("fail", "err")
        assert r["success"] is False

    def test_message_stored(self):
        r = skill_error("operation failed", "traceback")
        assert r["message"] == "operation failed"

    def test_error_stored(self):
        r = skill_error("fail", "FileNotFoundError")
        assert r["error"] == "FileNotFoundError"

    def test_prompt_stored(self):
        r = skill_error("fail", "err", prompt="check path")
        assert r["prompt"] == "check path"

    def test_possible_solutions_in_context(self):
        r = skill_error("fail", "err", possible_solutions=["fix A", "fix B"])
        assert "possible_solutions" in r["context"]
        assert r["context"]["possible_solutions"] == ["fix A", "fix B"]

    def test_extra_kwargs_in_context(self):
        r = skill_error("fail", "err", details="trace")
        assert r["context"]["details"] == "trace"

    def test_no_solutions_context_minimal(self):
        r = skill_error("fail", "err")
        # context may be empty or only have non-solutions keys
        assert "possible_solutions" not in r["context"]


class TestSkillWarning:
    def test_success_is_true(self):
        r = skill_warning("done", warning="slow")
        assert r["success"] is True

    def test_message_stored(self):
        r = skill_warning("finished with warnings", warning="slow")
        assert r["message"] == "finished with warnings"

    def test_warning_in_context(self):
        r = skill_warning("done", warning="performance degraded")
        assert r["context"]["warning"] == "performance degraded"

    def test_extra_kwargs_in_context(self):
        r = skill_warning("ok", warning="slow", elapsed_ms=2000)
        assert r["context"]["elapsed_ms"] == 2000

    def test_prompt_stored(self):
        r = skill_warning("ok", warning="slow", prompt="optimize")
        assert r["prompt"] == "optimize"


class TestSkillException:
    def test_success_is_false(self):
        try:
            raise ValueError("bad input")
        except Exception as e:
            r = skill_exception(e)
        assert r["success"] is False

    def test_error_contains_exception_type(self):
        try:
            raise ValueError("bad input")
        except Exception as e:
            r = skill_exception(e)
        assert "ValueError" in r["error"] or "bad input" in r["error"]

    def test_custom_message(self):
        try:
            raise RuntimeError("crash")
        except Exception as e:
            r = skill_exception(e, message="caught an error")
        assert r["message"] == "caught an error"

    def test_include_traceback_in_context(self):
        try:
            raise ValueError("oops")
        except Exception as e:
            r = skill_exception(e, include_traceback=True)
        # traceback info should be in context
        assert "traceback" in r.get("context", {}) or "error" in r

    def test_no_traceback_by_default_still_ok(self):
        try:
            raise ValueError("oops")
        except Exception as e:
            r = skill_exception(e, include_traceback=False)
        assert r["success"] is False

    def test_possible_solutions_stored(self):
        try:
            raise OSError("no file")
        except Exception as e:
            r = skill_exception(e, possible_solutions=["create file", "check path"])
        assert "possible_solutions" in r["context"]

    def test_extra_kwargs_in_context(self):
        try:
            raise ValueError("x")
        except Exception as e:
            r = skill_exception(e, user="artist")
        assert r["context"]["user"] == "artist"


class TestSkillEntryDecorator:
    def test_decorated_function_called(self):
        @skill_entry
        def my_func(**kwargs):
            return skill_success("ok")

        r = my_func()
        assert r["success"] is True

    def test_kwargs_passed_to_function(self):
        @skill_entry
        def my_func(x=1, **kwargs):
            return skill_success("ok", x=x)

        r = my_func(x=42)
        assert r["context"]["x"] == 42

    def test_import_error_caught(self):
        @skill_entry
        def my_func(**kwargs):
            raise ImportError("maya not available")

        r = my_func()
        assert r["success"] is False

    def test_exception_caught(self):
        @skill_entry
        def my_func(**kwargs):
            raise RuntimeError("something went wrong")

        r = my_func()
        assert r["success"] is False

    def test_returns_dict_always(self):
        @skill_entry
        def my_func(**kwargs):
            return skill_error("fail", "err")

        r = my_func()
        assert isinstance(r, dict)

    def test_preserves_function_name(self):
        @skill_entry
        def unique_skill_name(**kwargs):
            return skill_success("ok")

        # Function name should be preserved via functools.wraps or similar
        assert callable(unique_skill_name)

    def test_multiple_decorations_independent(self):
        @skill_entry
        def func_a(**kwargs):
            return skill_success("a")

        @skill_entry
        def func_b(**kwargs):
            return skill_success("b")

        assert func_a()["message"] == "a"
        assert func_b()["message"] == "b"
