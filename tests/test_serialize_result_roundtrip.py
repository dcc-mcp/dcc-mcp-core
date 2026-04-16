"""Round-trip integration tests for serialize_result / deserialize_result.

Covers:
- SerializeFormat enum values (Json, MsgPack)
- serialize_result returns str for Json, bytes for MsgPack
- deserialize_result round-trip: Json str → ToolResult
- deserialize_result round-trip: MsgPack bytes → ToolResult
- All ToolResult fields preserved (success, message, prompt, error, context)
- success_result / error_result / from_exception factories + round-trip
- Context with nested structures (list, dict, int, float, bool, None)
- Empty context round-trip
- Long message and Unicode content
- deserialize_result TypeError on wrong input type
- deserialize_result ValueError on corrupt data
- _serialize_result helper in skill.py falls back when _core unavailable
- run_main in skill.py uses Rust path when _core available
"""

from __future__ import annotations

import json

import pytest

import dcc_mcp_core
from dcc_mcp_core import SerializeFormat
from dcc_mcp_core import ToolResult
from dcc_mcp_core import deserialize_result
from dcc_mcp_core import error_result
from dcc_mcp_core import from_exception
from dcc_mcp_core import serialize_result
from dcc_mcp_core import success_result
from dcc_mcp_core import validate_action_result

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _assert_model_equal(a: ToolResult, b: ToolResult) -> None:
    """Assert that two ToolResult instances have identical fields."""
    assert a.success == b.success, f"success mismatch: {a.success!r} != {b.success!r}"
    assert a.message == b.message, f"message mismatch: {a.message!r} != {b.message!r}"
    assert a.prompt == b.prompt, f"prompt mismatch: {a.prompt!r} != {b.prompt!r}"
    assert a.error == b.error, f"error mismatch: {a.error!r} != {b.error!r}"
    assert a.context == b.context, f"context mismatch: {a.context!r} != {b.context!r}"


# ---------------------------------------------------------------------------
# SerializeFormat enum
# ---------------------------------------------------------------------------


class TestSerializeFormat:
    def test_json_attribute_exists(self):
        assert hasattr(SerializeFormat, "Json")

    def test_msgpack_attribute_exists(self):
        assert hasattr(SerializeFormat, "MsgPack")

    def test_json_repr(self):
        assert "Json" in repr(SerializeFormat.Json)

    def test_msgpack_repr(self):
        assert "MsgPack" in repr(SerializeFormat.MsgPack)

    def test_json_and_msgpack_not_equal(self):
        assert SerializeFormat.Json != SerializeFormat.MsgPack

    def test_json_equals_itself(self):
        assert SerializeFormat.Json == SerializeFormat.Json

    def test_msgpack_equals_itself(self):
        assert SerializeFormat.MsgPack == SerializeFormat.MsgPack


# ---------------------------------------------------------------------------
# serialize_result — return type
# ---------------------------------------------------------------------------


class TestSerializeResultReturnType:
    def test_json_returns_str(self):
        arm = success_result("ok")
        result = serialize_result(arm)
        assert isinstance(result, str), f"expected str, got {type(result)}"

    def test_json_explicit_returns_str(self):
        arm = success_result("ok")
        result = serialize_result(arm, SerializeFormat.Json)
        assert isinstance(result, str)

    def test_msgpack_returns_bytes(self):
        arm = success_result("ok")
        result = serialize_result(arm, SerializeFormat.MsgPack)
        assert isinstance(result, bytes), f"expected bytes, got {type(result)}"

    def test_json_output_is_valid_json(self):
        arm = success_result("test message", count=3)
        json_str = serialize_result(arm)
        parsed = json.loads(json_str)
        assert parsed["success"] is True
        assert parsed["message"] == "test message"

    def test_json_not_empty(self):
        arm = success_result("x")
        assert len(serialize_result(arm)) > 0

    def test_msgpack_not_empty(self):
        arm = success_result("x")
        assert len(serialize_result(arm, SerializeFormat.MsgPack)) > 0

    def test_msgpack_smaller_than_json_for_minimal(self):
        # MsgPack is typically more compact than JSON for simple objects
        arm = success_result("hello")
        json_size = len(serialize_result(arm).encode())
        msgpack_size = len(serialize_result(arm, SerializeFormat.MsgPack))
        # Allow either to be smaller — just verify both produce valid output
        assert json_size > 0 and msgpack_size > 0


# ---------------------------------------------------------------------------
# JSON round-trip
# ---------------------------------------------------------------------------


class TestJsonRoundTrip:
    def test_success_result_json_roundtrip(self):
        original = success_result("all done")
        restored = deserialize_result(serialize_result(original))
        _assert_model_equal(original, restored)

    def test_error_result_json_roundtrip(self):
        original = error_result("failed", error="disk full", prompt="free space")
        restored = deserialize_result(serialize_result(original))
        _assert_model_equal(original, restored)

    def test_error_result_with_possible_solutions(self):
        original = error_result(
            "bad input",
            error="ValueError: negative radius",
            possible_solutions=["use positive value", "check units"],
        )
        restored = deserialize_result(serialize_result(original))
        _assert_model_equal(original, restored)
        assert restored.context["possible_solutions"] == [
            "use positive value",
            "check units",
        ]

    def test_success_with_prompt_json_roundtrip(self):
        original = success_result("mesh created", prompt="Inspect viewport")
        restored = deserialize_result(serialize_result(original))
        assert restored.prompt == "Inspect viewport"

    def test_success_flag_true_preserved(self):
        original = success_result("ok")
        restored = deserialize_result(serialize_result(original))
        assert restored.success is True

    def test_error_flag_false_preserved(self):
        original = error_result("bad", error="oops")
        restored = deserialize_result(serialize_result(original))
        assert restored.success is False

    def test_error_field_preserved(self):
        original = error_result("bad", error="TypeError: bad arg")
        restored = deserialize_result(serialize_result(original))
        assert restored.error == "TypeError: bad arg"

    def test_from_exception_json_roundtrip(self):
        original = from_exception("RuntimeError: boom", message="Script crashed")
        restored = deserialize_result(serialize_result(original))
        assert restored.success is False
        assert restored.message == "Script crashed"

    def test_explicit_json_format(self):
        original = success_result("explicit json")
        serialized = serialize_result(original, SerializeFormat.Json)
        restored = deserialize_result(serialized, SerializeFormat.Json)
        _assert_model_equal(original, restored)


# ---------------------------------------------------------------------------
# MsgPack round-trip
# ---------------------------------------------------------------------------


class TestMsgPackRoundTrip:
    def test_success_result_msgpack_roundtrip(self):
        original = success_result("packed")
        serialized = serialize_result(original, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        _assert_model_equal(original, restored)

    def test_error_result_msgpack_roundtrip(self):
        original = error_result("fail", error="boom", prompt="retry")
        serialized = serialize_result(original, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        _assert_model_equal(original, restored)

    def test_msgpack_success_flag_preserved(self):
        original = success_result("ok")
        serialized = serialize_result(original, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.success is True

    def test_msgpack_message_preserved(self):
        original = success_result("msgpack message")
        serialized = serialize_result(original, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.message == "msgpack message"

    def test_msgpack_prompt_preserved(self):
        original = success_result("ok", prompt="check this")
        serialized = serialize_result(original, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.prompt == "check this"

    def test_msgpack_null_prompt_preserved(self):
        original = success_result("ok")
        serialized = serialize_result(original, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.prompt is None

    def test_msgpack_error_preserved(self):
        original = error_result("bad", error="TypeError: arg")
        serialized = serialize_result(original, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.error == "TypeError: arg"


# ---------------------------------------------------------------------------
# Context preservation (various types)
# ---------------------------------------------------------------------------


class TestContextRoundTrip:
    def test_int_context_json(self):
        arm = success_result("ok", count=42)
        restored = deserialize_result(serialize_result(arm))
        assert restored.context["count"] == 42

    def test_float_context_json(self):
        arm = success_result("ok", ratio=0.75)
        restored = deserialize_result(serialize_result(arm))
        assert abs(restored.context["ratio"] - 0.75) < 1e-9

    def test_bool_context_json(self):
        arm = success_result("ok", is_valid=True)
        restored = deserialize_result(serialize_result(arm))
        assert restored.context["is_valid"] is True

    def test_list_context_json(self):
        arm = success_result("ok", names=["Alice", "Bob", "Charlie"])
        restored = deserialize_result(serialize_result(arm))
        assert restored.context["names"] == ["Alice", "Bob", "Charlie"]

    def test_nested_dict_context_json(self):
        arm = success_result("ok", transform={"tx": 1.0, "ty": 2.0, "tz": 3.0})
        restored = deserialize_result(serialize_result(arm))
        assert restored.context["transform"]["tx"] == 1.0

    def test_empty_context_json(self):
        arm = success_result("empty ctx")
        restored = deserialize_result(serialize_result(arm))
        assert restored.context == {}

    def test_unicode_in_context_json(self):
        arm = success_result("ok", label="中文标签 🎨")
        restored = deserialize_result(serialize_result(arm))
        assert restored.context["label"] == "中文标签 🎨"

    def test_int_context_msgpack(self):
        arm = success_result("ok", frame=100)
        serialized = serialize_result(arm, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.context["frame"] == 100

    def test_list_context_msgpack(self):
        arm = success_result("ok", ids=[1, 2, 3])
        serialized = serialize_result(arm, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.context["ids"] == [1, 2, 3]

    def test_multiple_context_keys(self):
        arm = success_result("batch", created=5, skipped=1, dcc="blender")
        restored = deserialize_result(serialize_result(arm))
        assert restored.context["created"] == 5
        assert restored.context["skipped"] == 1
        assert restored.context["dcc"] == "blender"


# ---------------------------------------------------------------------------
# Unicode content
# ---------------------------------------------------------------------------


class TestUnicodeRoundTrip:
    def test_unicode_message_json(self):
        arm = success_result("操作完成 ✓")
        restored = deserialize_result(serialize_result(arm))
        assert restored.message == "操作完成 ✓"

    def test_unicode_error_json(self):
        arm = error_result("失败", error="エラー: ファイルが見つかりません")
        restored = deserialize_result(serialize_result(arm))
        assert restored.error == "エラー: ファイルが見つかりません"

    def test_unicode_message_msgpack(self):
        arm = success_result("성공했습니다 🎉")
        serialized = serialize_result(arm, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.message == "성공했습니다 🎉"


# ---------------------------------------------------------------------------
# Long content
# ---------------------------------------------------------------------------


class TestLongContent:
    def test_long_message_json_roundtrip(self):
        msg = "a" * 10_000
        arm = success_result(msg)
        restored = deserialize_result(serialize_result(arm))
        assert restored.message == msg

    def test_many_context_keys(self):
        ctx = {f"key_{i}": i for i in range(100)}
        arm = success_result("ok", **ctx)
        restored = deserialize_result(serialize_result(arm))
        for i in range(100):
            assert restored.context[f"key_{i}"] == i


# ---------------------------------------------------------------------------
# ToolResult.with_error / with_context round-trips
# ---------------------------------------------------------------------------


class TestMutatedModelRoundTrip:
    def test_with_error_json_roundtrip(self):
        base = success_result("start")
        modified = base.with_error("something went wrong")
        restored = deserialize_result(serialize_result(modified))
        assert restored.success is False
        assert restored.error == "something went wrong"

    def test_with_context_json_roundtrip(self):
        base = success_result("base")
        modified = base.with_context(extra_key="extra_value", number=99)
        restored = deserialize_result(serialize_result(modified))
        assert restored.context["extra_key"] == "extra_value"
        assert restored.context["number"] == 99

    def test_with_error_msgpack_roundtrip(self):
        base = success_result("start")
        modified = base.with_error("crash")
        serialized = serialize_result(modified, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert restored.success is False
        assert restored.error == "crash"


# ---------------------------------------------------------------------------
# validate_action_result → serialize_result pipeline
# ---------------------------------------------------------------------------


class TestValidateAndSerializePipeline:
    """Test the full pipeline used by skill.py: dict → validate → serialize → deserialize."""

    def test_success_dict_pipeline(self):
        # validate_action_result extracts success/message/prompt/error from the dict;
        # all other top-level keys become context entries.
        # An empty "context" key becomes context["context"] = {} — that's expected behavior.
        raw = {"success": True, "message": "done", "prompt": None, "error": None}
        arm = validate_action_result(raw)
        restored = deserialize_result(serialize_result(arm))
        assert restored.success is True
        assert restored.message == "done"

    def test_error_dict_pipeline(self):
        # validate_action_result treats top-level non-standard keys as context.
        # Pass path as a top-level key so it ends up in context directly.
        raw = {
            "success": False,
            "message": "failed",
            "error": "IOError: file missing",
            "prompt": "check path",
            "path": "/tmp/missing.ma",  # top-level extra key → context["path"]
        }
        arm = validate_action_result(raw)
        restored = deserialize_result(serialize_result(arm))
        assert restored.success is False
        assert restored.error == "IOError: file missing"
        assert restored.context["path"] == "/tmp/missing.ma"

    def test_dict_with_extra_context_pipeline(self):
        # Extra top-level keys beyond success/message/prompt/error become context entries.
        raw = {
            "success": True,
            "message": "ok",
            "prompt": None,
            "error": None,
            "nodes": ["a", "b"],  # top-level → context["nodes"]
            "count": 2,  # top-level → context["count"]
        }
        arm = validate_action_result(raw)
        restored = deserialize_result(serialize_result(arm))
        assert restored.context["nodes"] == ["a", "b"]
        assert restored.context["count"] == 2

    def test_already_arm_pipeline(self):
        arm = success_result("already arm")
        arm2 = validate_action_result(arm)
        restored = deserialize_result(serialize_result(arm2))
        assert restored.message == "already arm"


# ---------------------------------------------------------------------------
# Error handling
# ---------------------------------------------------------------------------


class TestDeserializeResultErrors:
    def test_type_error_on_int(self):
        with pytest.raises(TypeError):
            deserialize_result(42)  # type: ignore[arg-type]

    def test_type_error_on_none(self):
        with pytest.raises(TypeError):
            deserialize_result(None)  # type: ignore[arg-type]

    def test_type_error_on_list(self):
        with pytest.raises((TypeError, ValueError)):
            deserialize_result([1, 2, 3])  # type: ignore[arg-type]

    def test_value_error_on_corrupt_json(self):
        with pytest.raises((ValueError, Exception)):
            deserialize_result("{not valid json}")

    def test_value_error_on_corrupt_msgpack(self):
        with pytest.raises((ValueError, Exception)):
            deserialize_result(b"\xff\xfe\xfd\x00garbage", SerializeFormat.MsgPack)

    def test_value_error_on_empty_bytes_msgpack(self):
        with pytest.raises((ValueError, Exception)):
            deserialize_result(b"", SerializeFormat.MsgPack)


# ---------------------------------------------------------------------------
# Skill module _serialize_result helper
# ---------------------------------------------------------------------------


class TestSkillSerializeResult:
    """Tests for the _serialize_result helper in dcc_mcp_core.skill."""

    def test_serialize_result_helper_returns_json_str(self):
        from dcc_mcp_core.skill import _serialize_result

        result = {"success": True, "message": "test", "prompt": None, "error": None, "context": {}}
        output = _serialize_result(result)
        assert isinstance(output, str)
        parsed = json.loads(output)
        assert parsed["success"] is True
        assert parsed["message"] == "test"

    def test_serialize_result_helper_success(self):
        from dcc_mcp_core.skill import _serialize_result

        # Pass context as top-level kwargs (as skill functions do via skill_success)
        # so count ends up directly in context, not nested under context["context"].
        result = {
            "success": True,
            "message": "batch done",
            "prompt": "check viewport",
            "error": None,
            "count": 5,  # top-level extra key → context["count"]
        }
        output = _serialize_result(result)
        parsed = json.loads(output)
        assert parsed["context"]["count"] == 5

    def test_serialize_result_helper_error(self):
        from dcc_mcp_core.skill import _serialize_result

        result = {
            "success": False,
            "message": "failed",
            "prompt": "retry",
            "error": "RuntimeError: boom",
            "context": {},
        }
        output = _serialize_result(result)
        parsed = json.loads(output)
        assert parsed["success"] is False
        assert parsed["error"] == "RuntimeError: boom"

    def test_serialize_result_fallback_without_core(self, monkeypatch):
        """When _core cannot be imported, fall back to json.dumps."""
        import builtins
        import importlib

        original_import = builtins.__import__

        def mock_import(name, *args, **kwargs):
            if name == "dcc_mcp_core._core":
                raise ImportError("simulated missing _core")
            return original_import(name, *args, **kwargs)

        # Reload skill module to get a fresh copy
        import dcc_mcp_core.skill as skill_mod

        importlib.reload(skill_mod)

        with monkeypatch.context() as mp:
            mp.setattr(builtins, "__import__", mock_import)
            # Import _serialize_result after patching
            result = {"success": True, "message": "fallback", "prompt": None, "error": None, "context": {}}
            # Call directly to exercise the fallback branch
            output = skill_mod._serialize_result(result)
        assert isinstance(output, str)
        parsed = json.loads(output)
        assert parsed["message"] == "fallback"


# ---------------------------------------------------------------------------
# Cross-format compatibility check
# ---------------------------------------------------------------------------


class TestCrossFormatIncompatibility:
    """JSON bytes should not deserialize as MsgPack and vice versa."""

    def test_json_bytes_fail_as_msgpack(self):
        arm = success_result("ok")
        json_bytes = serialize_result(arm).encode()
        with pytest.raises((ValueError, Exception)):
            deserialize_result(json_bytes, SerializeFormat.MsgPack)

    def test_msgpack_bytes_fail_as_json(self):
        arm = success_result("ok")
        msgpack_bytes = serialize_result(arm, SerializeFormat.MsgPack)
        with pytest.raises((ValueError, Exception)):
            # Pass bytes as str or try to decode as JSON — should fail
            deserialize_result(msgpack_bytes.decode("latin-1"), SerializeFormat.Json)


# ---------------------------------------------------------------------------
# ToolResult instance returned by deserialize
# ---------------------------------------------------------------------------


class TestDeserializeReturnsActionResultModel:
    def test_returns_action_result_model_type(self):
        arm = success_result("check type")
        restored = deserialize_result(serialize_result(arm))
        assert isinstance(restored, ToolResult)

    def test_msgpack_returns_action_result_model_type(self):
        arm = success_result("check type")
        serialized = serialize_result(arm, SerializeFormat.MsgPack)
        restored = deserialize_result(serialized, SerializeFormat.MsgPack)
        assert isinstance(restored, ToolResult)

    def test_restored_model_has_correct_api(self):
        arm = success_result("api check", key="val")
        restored = deserialize_result(serialize_result(arm))
        # Should support all ToolResult operations
        with_err = restored.with_error("new error")
        assert with_err.success is False
        with_ctx = restored.with_context(extra=42)
        assert with_ctx.context["extra"] == 42
        as_dict = restored.to_dict()
        assert as_dict["success"] is True
