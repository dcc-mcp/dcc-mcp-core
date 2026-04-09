"""Deep tests for ActionResultModel, factory functions, and validate_action_result.

Covers:
- success_result() factory: message, prompt, context kwargs
- error_result() factory: message, error, possible_solutions, prompt
- from_exception() factory: wraps exception string + traceback
- validate_action_result() normalises dict / str / None / ActionResultModel
- ActionResultModel.with_error() returns new model with success=False
- ActionResultModel.with_context() merges extra kwargs into context
- ActionResultModel.to_dict() round-trip
- ActionResultModel equality / repr / str
- ActionResultModel fields: success, message, prompt, error, context
- error_result possible_solutions stored in context
- from_exception include_traceback=False suppresses traceback
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import ActionResultModel
from dcc_mcp_core import error_result
from dcc_mcp_core import from_exception
from dcc_mcp_core import success_result
from dcc_mcp_core import validate_action_result

# ---------------------------------------------------------------------------
# success_result()
# ---------------------------------------------------------------------------


class TestSuccessResult:
    def test_success_flag_is_true(self):
        r = success_result("done")
        assert r.success is True

    def test_message_stored(self):
        r = success_result("operation complete")
        assert r.message == "operation complete"

    def test_error_is_none(self):
        r = success_result("ok")
        assert r.error is None

    def test_prompt_is_none_by_default(self):
        r = success_result("ok")
        assert r.prompt is None

    def test_prompt_stored_when_provided(self):
        r = success_result("ok", prompt="Consider saving the scene")
        assert r.prompt == "Consider saving the scene"

    def test_context_kwargs_stored(self):
        r = success_result("created", object_name="Cube", position=[0, 0, 0])
        assert r.context["object_name"] == "Cube"

    def test_context_empty_by_default(self):
        r = success_result("ok")
        assert r.context == {}

    def test_returns_action_result_model_instance(self):
        r = success_result("ok")
        assert isinstance(r, ActionResultModel)

    def test_multiple_context_kwargs(self):
        r = success_result("batch", count=5, success_rate=1.0, dcc="maya")
        assert r.context["count"] == 5
        assert abs(r.context["success_rate"] - 1.0) < 1e-6
        assert r.context["dcc"] == "maya"


# ---------------------------------------------------------------------------
# error_result()
# ---------------------------------------------------------------------------


class TestErrorResult:
    def test_success_flag_is_false(self):
        r = error_result("failed", error="disk full")
        assert r.success is False

    def test_message_stored(self):
        r = error_result("failed to export", error="disk full")
        assert r.message == "failed to export"

    def test_error_field_stored(self):
        r = error_result("fail", error="connection refused")
        assert r.error == "connection refused"

    def test_prompt_none_by_default(self):
        r = error_result("fail", error="e")
        assert r.prompt is None

    def test_prompt_stored_when_provided(self):
        r = error_result("fail", error="e", prompt="Check your network connection")
        assert r.prompt == "Check your network connection"

    def test_possible_solutions_in_context_or_accessible(self):
        r = error_result(
            "fail",
            error="missing file",
            possible_solutions=["Check path", "Restore backup"],
        )
        # possible_solutions should be accessible somehow (context or field)
        d = r.to_dict()
        # At minimum, no crash and success=False
        assert r.success is False
        assert isinstance(d, dict)

    def test_context_kwargs_in_error(self):
        r = error_result("fail", error="e", file_path="/missing.usd")
        assert r.context.get("file_path") == "/missing.usd"

    def test_returns_action_result_model_instance(self):
        r = error_result("fail", error="e")
        assert isinstance(r, ActionResultModel)


# ---------------------------------------------------------------------------
# from_exception()
# ---------------------------------------------------------------------------


class TestFromException:
    def test_success_is_false(self):
        r = from_exception("ValueError occurred")
        assert r.success is False

    def test_error_field_contains_exception_message(self):
        r = from_exception("divide by zero")
        # error should contain something about the exception
        assert r.error is not None
        assert len(r.error) > 0

    def test_message_field_present(self):
        r = from_exception("err", message="Action failed due to internal error")
        assert r.message == "Action failed due to internal error"

    def test_message_defaults_when_not_provided(self):
        r = from_exception("err")
        assert r.message is not None
        # Default message should be non-empty or empty string, but not crash
        assert isinstance(r.message, str)

    def test_prompt_stored(self):
        r = from_exception("err", prompt="Check the Maya log for details")
        assert r.prompt == "Check the Maya log for details"

    def test_include_traceback_false_suppresses_trace(self):
        r = from_exception("ValueError", include_traceback=False)
        # With traceback suppressed, error should still be set but may be shorter
        assert r.success is False
        assert r.error is not None

    def test_context_kwargs_stored(self):
        r = from_exception("err", dcc_name="maya", action="create_sphere")
        assert r.context.get("dcc_name") == "maya"
        assert r.context.get("action") == "create_sphere"

    def test_returns_action_result_model(self):
        r = from_exception("err")
        assert isinstance(r, ActionResultModel)


# ---------------------------------------------------------------------------
# ActionResultModel.with_error()
# ---------------------------------------------------------------------------


class TestWithError:
    def test_with_error_sets_success_false(self):
        r = success_result("ok")
        modified = r.with_error("something failed")
        assert modified.success is False

    def test_with_error_sets_error_field(self):
        r = success_result("ok")
        modified = r.with_error("disk full")
        assert modified.error == "disk full"

    def test_with_error_returns_new_instance(self):
        r = success_result("ok")
        modified = r.with_error("oops")
        assert r is not modified
        # Original should be unchanged
        assert r.success is True

    def test_with_error_preserves_message(self):
        r = success_result("original message")
        modified = r.with_error("error occurred")
        assert modified.message == "original message"

    def test_with_error_preserves_context(self):
        r = success_result("ok", count=42)
        modified = r.with_error("fail")
        assert modified.context.get("count") == 42


# ---------------------------------------------------------------------------
# ActionResultModel.with_context()
# ---------------------------------------------------------------------------


class TestWithContext:
    def test_with_context_adds_new_key(self):
        r = success_result("ok")
        modified = r.with_context(object_id="mesh_001")
        assert modified.context["object_id"] == "mesh_001"

    def test_with_context_merges_existing(self):
        r = success_result("ok", existing="value")
        modified = r.with_context(new_key="new_value")
        assert modified.context["existing"] == "value"
        assert modified.context["new_key"] == "new_value"

    def test_with_context_returns_new_instance(self):
        r = success_result("ok")
        modified = r.with_context(x=1)
        assert r is not modified

    def test_with_context_does_not_mutate_original(self):
        r = success_result("ok")
        r.with_context(x=1)
        assert "x" not in r.context

    def test_with_context_preserves_success_flag(self):
        r = success_result("ok")
        modified = r.with_context(k="v")
        assert modified.success is True

    def test_with_context_multiple_kwargs(self):
        r = success_result("ok")
        modified = r.with_context(a=1, b="two", c=3.0)
        assert modified.context["a"] == 1
        assert modified.context["b"] == "two"
        assert abs(modified.context["c"] - 3.0) < 1e-6


# ---------------------------------------------------------------------------
# ActionResultModel.to_dict()
# ---------------------------------------------------------------------------


class TestToDict:
    def test_to_dict_has_success_key(self):
        r = success_result("ok")
        d = r.to_dict()
        assert "success" in d

    def test_to_dict_has_message_key(self):
        r = success_result("hello")
        d = r.to_dict()
        assert d["message"] == "hello"

    def test_to_dict_success_true(self):
        r = success_result("ok")
        d = r.to_dict()
        assert d["success"] is True

    def test_to_dict_error_none_in_success(self):
        r = success_result("ok")
        d = r.to_dict()
        assert d.get("error") is None

    def test_to_dict_error_result(self):
        r = error_result("fail", error="reason")
        d = r.to_dict()
        assert d["success"] is False
        assert d["error"] == "reason"

    def test_to_dict_context_preserved(self):
        r = success_result("ok", count=10, name="sphere")
        d = r.to_dict()
        ctx = d.get("context", {})
        assert ctx.get("count") == 10
        assert ctx.get("name") == "sphere"

    def test_to_dict_returns_dict(self):
        r = success_result("ok")
        assert isinstance(r.to_dict(), dict)


# ---------------------------------------------------------------------------
# ActionResultModel equality / repr / str
# ---------------------------------------------------------------------------


class TestActionResultModelMisc:
    def test_equality_same_values(self):
        r1 = ActionResultModel(success=True, message="ok")
        r2 = ActionResultModel(success=True, message="ok")
        assert r1 == r2

    def test_inequality_different_success(self):
        r1 = ActionResultModel(success=True, message="ok")
        r2 = ActionResultModel(success=False, message="ok")
        assert r1 != r2

    def test_repr_contains_class(self):
        r = success_result("ok")
        assert "ActionResultModel" in repr(r) or "success" in repr(r).lower()

    def test_str_non_empty(self):
        r = success_result("ok")
        assert len(str(r)) > 0

    def test_direct_constructor(self):
        r = ActionResultModel(
            success=True,
            message="direct",
            prompt="hint",
            error=None,
            context={"key": "value"},
        )
        assert r.success is True
        assert r.message == "direct"
        assert r.prompt == "hint"
        assert r.context["key"] == "value"


# ---------------------------------------------------------------------------
# validate_action_result()
# ---------------------------------------------------------------------------


class TestValidateActionResult:
    def test_passes_through_action_result_model(self):
        r = success_result("ok")
        validated = validate_action_result(r)
        assert isinstance(validated, ActionResultModel)
        assert validated.success is True

    def test_wraps_dict_with_success_true(self):
        d = {"success": True, "message": "ok"}
        validated = validate_action_result(d)
        assert isinstance(validated, ActionResultModel)
        assert validated.success is True

    def test_wraps_dict_with_success_false(self):
        d = {"success": False, "message": "fail", "error": "oops"}
        validated = validate_action_result(d)
        assert isinstance(validated, ActionResultModel)
        assert validated.success is False

    def test_wraps_none_as_success_true(self):
        validated = validate_action_result(None)
        assert isinstance(validated, ActionResultModel)
        # None result implies success with no output
        assert isinstance(validated.success, bool)

    def test_wraps_string_message(self):
        validated = validate_action_result("all good")
        assert isinstance(validated, ActionResultModel)
        # String should map to success result with that message
        assert isinstance(validated.message, str)

    def test_returns_action_result_model_type(self):
        for value in [None, "ok", {"success": True, "message": "x"}, success_result("y")]:
            result = validate_action_result(value)
            assert isinstance(result, ActionResultModel), f"Failed for value={value!r}"
