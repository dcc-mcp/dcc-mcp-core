"""Tests for ActionResultModel and factory functions."""

# Import local modules
import dcc_mcp_core


class TestActionResultModel:
    def test_create_default(self):
        r = dcc_mcp_core.ActionResultModel()
        assert r.success is True
        assert r.message == ""
        assert r.prompt is None
        assert r.error is None

    def test_create_with_all_args(self):
        r = dcc_mcp_core.ActionResultModel(
            success=False,
            message="failed",
            prompt="try again",
            error="oops",
            context={"key": "val"},
        )
        assert r.success is False
        assert r.message == "failed"
        assert r.prompt == "try again"
        assert r.error == "oops"
        assert r.context["key"] == "val"

    def test_message_setter(self):
        r = dcc_mcp_core.ActionResultModel(message="old")
        r.message = "new"
        assert r.message == "new"

    def test_with_error(self):
        r = dcc_mcp_core.ActionResultModel(message="ok")
        r2 = r.with_error("bad")
        assert r2.success is False
        assert r2.error == "bad"
        assert r.success is True  # original unchanged

    def test_with_context(self):
        r = dcc_mcp_core.ActionResultModel(message="ok")
        r2 = r.with_context(key="value", num=42)
        ctx = r2.context
        assert ctx["key"] == "value"
        assert ctx["num"] == 42

    def test_with_context_no_kwargs(self):
        r = dcc_mcp_core.ActionResultModel(message="ok")
        r2 = r.with_context()
        assert r2.context == {}

    def test_context_complex_types(self):
        r = dcc_mcp_core.ActionResultModel(
            message="test",
            context={
                "str_val": "hello",
                "int_val": 42,
                "float_val": 3.14,
                "bool_val": True,
                "none_val": None,
                "list_val": [1, 2, 3],
                "dict_val": {"nested": "data"},
            },
        )
        ctx = r.context
        assert ctx["str_val"] == "hello"
        assert ctx["int_val"] == 42
        assert ctx["float_val"] == 3.14
        assert ctx["bool_val"] is True
        assert ctx["none_val"] is None
        assert ctx["list_val"] == [1, 2, 3]
        assert ctx["dict_val"]["nested"] == "data"

    def test_to_dict(self):
        r = dcc_mcp_core.ActionResultModel(
            success=True, message="done", prompt="next"
        )
        d = r.to_dict()
        assert d["success"] is True
        assert d["message"] == "done"
        assert d["prompt"] == "next"
        assert d["error"] is None
        assert isinstance(d["context"], dict)

    def test_to_dict_with_error(self):
        r = dcc_mcp_core.ActionResultModel(success=False, message="fail", error="err")
        d = r.to_dict()
        assert d["success"] is False
        assert d["error"] == "err"

    def test_repr(self):
        r = dcc_mcp_core.ActionResultModel(success=True, message="hello")
        s = repr(r)
        assert "ActionResultModel" in s
        assert "hello" in s

    def test_str_success(self):
        r = dcc_mcp_core.ActionResultModel(message="done")
        assert "Success" in str(r)
        assert "done" in str(r)

    def test_str_error(self):
        r = dcc_mcp_core.ActionResultModel(success=False, error="oops")
        assert "Error" in str(r)
        assert "oops" in str(r)

    def test_str_error_fallback_to_message(self):
        r = dcc_mcp_core.ActionResultModel(success=False, message="fallback msg")
        assert "fallback msg" in str(r)


class TestFactoryFunctions:
    def test_success_result_minimal(self):
        r = dcc_mcp_core.success_result("done")
        assert r.success is True
        assert r.message == "done"
        assert r.prompt is None

    def test_success_result_with_prompt(self):
        r = dcc_mcp_core.success_result("done", prompt="next")
        assert r.prompt == "next"

    def test_success_result_with_context(self):
        r = dcc_mcp_core.success_result("done", count=5, name="test")
        ctx = r.context
        assert ctx["count"] == 5
        assert ctx["name"] == "test"

    def test_error_result_minimal(self):
        r = dcc_mcp_core.error_result("failed", "err msg")
        assert r.success is False
        assert r.message == "failed"
        assert r.error == "err msg"

    def test_error_result_with_prompt(self):
        r = dcc_mcp_core.error_result("failed", "err", prompt="retry")
        assert r.prompt == "retry"

    def test_error_result_with_possible_solutions(self):
        r = dcc_mcp_core.error_result(
            "failed",
            "err",
            possible_solutions=["fix A", "fix B"],
        )
        ctx = r.context
        assert "possible_solutions" in ctx
        assert ctx["possible_solutions"] == ["fix A", "fix B"]

    def test_error_result_with_extra_context(self):
        r = dcc_mcp_core.error_result("failed", "err", code=404)
        assert r.context["code"] == 404

    def test_from_exception_minimal(self):
        r = dcc_mcp_core.from_exception("ValueError: bad")
        assert r.success is False
        assert "ValueError" in r.error
        assert r.prompt is not None  # has default prompt
        assert "error_type" in r.context
        assert "traceback" in r.context  # include_traceback=True by default

    def test_from_exception_with_message(self):
        r = dcc_mcp_core.from_exception("err", message="Custom msg")
        assert r.message == "Custom msg"

    def test_from_exception_with_prompt(self):
        r = dcc_mcp_core.from_exception("err", prompt="do this")
        assert r.prompt == "do this"

    def test_from_exception_no_traceback(self):
        r = dcc_mcp_core.from_exception("err", include_traceback=False)
        assert "traceback" not in r.context

    def test_from_exception_with_solutions(self):
        r = dcc_mcp_core.from_exception(
            "err", possible_solutions=["sol1", "sol2"]
        )
        assert r.context["possible_solutions"] == ["sol1", "sol2"]

    def test_from_exception_with_extra_context(self):
        r = dcc_mcp_core.from_exception("err", module="core")
        assert r.context["module"] == "core"

    def test_validate_action_result_passthrough(self):
        orig = dcc_mcp_core.ActionResultModel(message="test")
        r = dcc_mcp_core.validate_action_result(orig)
        assert r.message == "test"

    def test_validate_action_result_from_dict(self):
        r = dcc_mcp_core.validate_action_result(
            {"success": True, "message": "from dict"}
        )
        assert r.success is True
        assert r.message == "from dict"

    def test_validate_action_result_from_dict_with_error(self):
        r = dcc_mcp_core.validate_action_result(
            {"success": False, "message": "fail", "error": "err"}
        )
        assert r.success is False
        assert r.error == "err"

    def test_validate_action_result_from_string(self):
        r = dcc_mcp_core.validate_action_result("hello")
        assert r.success is True
        assert r.context.get("value") == "hello"

    def test_validate_action_result_from_int(self):
        r = dcc_mcp_core.validate_action_result(42)
        assert r.success is True
