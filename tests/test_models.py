"""Tests for ActionResultModel and factory functions."""

import dcc_mcp_core


class TestActionResultModel:
    def test_create_default(self):
        r = dcc_mcp_core.ActionResultModel()
        assert r.success is True
        assert r.message == ""

    def test_create_with_args(self):
        r = dcc_mcp_core.ActionResultModel(
            success=False, message="failed", error="oops"
        )
        assert r.success is False
        assert r.message == "failed"
        assert r.error == "oops"

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

    def test_to_dict(self):
        r = dcc_mcp_core.ActionResultModel(
            success=True, message="done", prompt="next"
        )
        d = r.to_dict()
        assert d["success"] is True
        assert d["message"] == "done"
        assert d["prompt"] == "next"
        assert "context" in d

    def test_repr_and_str(self):
        r = dcc_mcp_core.ActionResultModel(message="hello")
        assert "hello" in repr(r)
        assert "hello" in str(r)


class TestFactoryFunctions:
    def test_success_result(self):
        r = dcc_mcp_core.success_result("done", prompt="next")
        assert r.success is True
        assert r.message == "done"
        assert r.prompt == "next"

    def test_error_result(self):
        r = dcc_mcp_core.error_result("failed", "err msg", prompt="retry")
        assert r.success is False
        assert r.error == "err msg"

    def test_from_exception(self):
        r = dcc_mcp_core.from_exception("ValueError: bad")
        assert r.success is False
        assert "ValueError" in r.error

    def test_validate_action_result_passthrough(self):
        orig = dcc_mcp_core.ActionResultModel(message="test")
        r = dcc_mcp_core.validate_action_result(orig)
        assert r.message == "test"

    def test_validate_action_result_from_dict(self):
        r = dcc_mcp_core.validate_action_result(
            {"success": True, "message": "from dict"}
        )
        assert r.success is True

    def test_validate_action_result_from_string(self):
        r = dcc_mcp_core.validate_action_result("hello")
        assert r.success is True
