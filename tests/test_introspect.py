"""Tests for the dcc_introspect__* built-in tools (issue #426)."""

from __future__ import annotations

import json
import logging
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from dcc_mcp_core.introspect import introspect_eval
from dcc_mcp_core.introspect import introspect_list_module
from dcc_mcp_core.introspect import introspect_search
from dcc_mcp_core.introspect import introspect_signature
from dcc_mcp_core.introspect import register_introspect_tools

# ── introspect_list_module ────────────────────────────────────────────────


class TestIntrospectListModule:
    def test_known_module_returns_names(self) -> None:
        result = introspect_list_module("math")
        assert result["success"] is True
        assert "sin" in result["context"]["names"]
        assert "cos" in result["context"]["names"]
        assert result["context"]["count"] > 0

    def test_names_are_sorted(self) -> None:
        result = introspect_list_module("math")
        names = result["context"]["names"]
        assert names == sorted(names)

    def test_limit_is_honoured(self) -> None:
        result = introspect_list_module("math", limit=5)
        assert len(result["context"]["names"]) <= 5
        assert result["context"]["truncated"] is True

    def test_unknown_module_returns_failure(self) -> None:
        result = introspect_list_module("definitely_not_a_real_module_xyz")
        assert result["success"] is False
        assert "import" in result["message"].lower()

    def test_no_private_names(self) -> None:
        result = introspect_list_module("math")
        assert not any(n.startswith("_") for n in result["context"]["names"])

    def test_truncated_false_when_under_limit(self) -> None:
        result = introspect_list_module("math", limit=10000)
        assert result["context"]["truncated"] is False


# ── introspect_signature ──────────────────────────────────────────────────


class TestIntrospectSignature:
    def test_known_callable(self) -> None:
        result = introspect_signature("math.sqrt")
        assert result["success"] is True
        assert "sqrt" in result["context"]["signature"]

    def test_doc_is_populated(self) -> None:
        result = introspect_signature("math.sqrt")
        assert len(result["context"]["doc"]) > 0

    def test_source_file_present_or_none(self) -> None:
        result = introspect_signature("math.sqrt")
        # math.sqrt may be a C extension; source_file may be None
        assert "source_file" in result["context"]

    def test_unknown_module_returns_failure(self) -> None:
        result = introspect_signature("no_such_module_xyz.func")
        assert result["success"] is False

    def test_unknown_attr_returns_failure(self) -> None:
        result = introspect_signature("math.totally_not_a_real_function_abc")
        assert result["success"] is False
        assert "not found" in result["message"]

    def test_kind_field_present(self) -> None:
        result = introspect_signature("math.sqrt")
        assert "kind" in result["context"]

    def test_doc_truncated_when_long(self) -> None:
        # Create a fake object with a very long docstring
        class _LongDoc:
            """X""" + "a" * 2000

        with patch("importlib.import_module") as mock_import:
            mock_mod = MagicMock()
            mock_mod.long_doc_obj = _LongDoc
            mock_import.return_value = mock_mod
            result = introspect_signature("fake_mod.long_doc_obj")
        assert result["success"] is True
        assert len(result["context"]["doc"]) <= 850  # _DOC_MAX_CHARS + "(truncated)"


# ── introspect_search ─────────────────────────────────────────────────────


class TestIntrospectSearch:
    def test_finds_matching_names(self) -> None:
        result = introspect_search("^sqrt$", "math")
        assert result["success"] is True
        assert any(h["qualname"] == "math.sqrt" for h in result["context"]["hits"])

    def test_case_insensitive(self) -> None:
        result = introspect_search("SQRT", "math")
        assert result["success"] is True
        assert len(result["context"]["hits"]) > 0

    def test_limit_is_honoured(self) -> None:
        result = introspect_search(".", "math", limit=3)
        assert len(result["context"]["hits"]) <= 3

    def test_no_matches_returns_empty(self) -> None:
        result = introspect_search("__absolutely_no_match_xyz__", "math")
        assert result["success"] is True
        assert result["context"]["hits"] == []

    def test_invalid_regex_returns_failure(self) -> None:
        result = introspect_search("[invalid", "math")
        assert result["success"] is False
        assert "regex" in result["message"].lower()

    def test_unknown_module_returns_failure(self) -> None:
        result = introspect_search(".*", "no_such_module_xyz")
        assert result["success"] is False

    def test_hits_have_qualname_and_summary(self) -> None:
        result = introspect_search("sin", "math")
        assert result["success"] is True
        for hit in result["context"]["hits"]:
            assert "qualname" in hit
            assert "summary" in hit
            assert hit["qualname"].startswith("math.")


# ── introspect_eval ───────────────────────────────────────────────────────


class TestIntrospectEval:
    def test_arithmetic_expression(self) -> None:
        result = introspect_eval("1 + 2")
        assert result["success"] is True
        assert "3" in result["context"]["repr"]

    def test_type_call(self) -> None:
        result = introspect_eval("type(42)")
        assert result["success"] is True
        assert "int" in result["context"]["repr"]

    def test_list_literal(self) -> None:
        result = introspect_eval("[1, 2, 3]")
        assert result["success"] is True
        assert "[1, 2, 3]" in result["context"]["repr"]

    def test_repr_truncated_when_long(self) -> None:
        # Force a long repr
        result = introspect_eval("list(range(1000))")
        assert result["success"] is True
        assert len(result["context"]["repr"]) <= 520  # _REPR_MAX_CHARS + "...(truncated)"

    def test_assignment_is_rejected(self) -> None:
        result = introspect_eval("x = 5")
        assert result["success"] is False

    def test_import_is_rejected(self) -> None:
        result = introspect_eval("import os")
        assert result["success"] is False

    def test_exec_call_is_rejected(self) -> None:
        result = introspect_eval("exec('pass')")
        assert result["success"] is False

    def test_syntax_error_returns_failure(self) -> None:
        result = introspect_eval("def (")
        assert result["success"] is False

    def test_runtime_error_returns_failure(self) -> None:
        result = introspect_eval("1/0")
        assert result["success"] is False
        assert "traceback" in result.get("context", {})


# ── register_introspect_tools ─────────────────────────────────────────────


class TestRegisterIntrospectTools:
    def _make_server(self) -> tuple[MagicMock, dict]:
        server = MagicMock()
        registry = MagicMock()
        server.registry = registry
        handlers: dict = {}
        server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
        return server, handlers

    def test_registers_four_tools(self) -> None:
        server, _handlers = self._make_server()
        register_introspect_tools(server, dcc_name="maya")
        names = {c.kwargs["name"] for c in server.registry.register.call_args_list}
        assert names == {
            "dcc_introspect__list_module",
            "dcc_introspect__signature",
            "dcc_introspect__search",
            "dcc_introspect__eval",
        }

    def test_list_module_handler_works(self) -> None:
        server, handlers = self._make_server()
        register_introspect_tools(server)
        result = handlers["dcc_introspect__list_module"](json.dumps({"module": "math"}))
        assert result["success"] is True
        assert "sin" in result["context"]["names"]

    def test_signature_handler_works(self) -> None:
        server, handlers = self._make_server()
        register_introspect_tools(server)
        result = handlers["dcc_introspect__signature"](json.dumps({"qualname": "math.sqrt"}))
        assert result["success"] is True

    def test_search_handler_works(self) -> None:
        server, handlers = self._make_server()
        register_introspect_tools(server)
        result = handlers["dcc_introspect__search"](
            json.dumps({"pattern": "sqrt", "module": "math"})
        )
        assert result["success"] is True

    def test_eval_handler_works(self) -> None:
        server, handlers = self._make_server()
        register_introspect_tools(server)
        result = handlers["dcc_introspect__eval"](json.dumps({"expression": "2 ** 10"}))
        assert result["success"] is True
        assert "1024" in result["context"]["repr"]

    def test_no_registry_logs_warning(self) -> None:
        class _BadServer:
            @property
            def registry(self):
                raise AttributeError("no registry")

        with patch.object(logging.getLogger("dcc_mcp_core.introspect"), "warning") as mock_warn:
            register_introspect_tools(_BadServer())
        mock_warn.assert_called_once()

    def test_handler_accepts_dict_params(self) -> None:
        server, handlers = self._make_server()
        register_introspect_tools(server)
        result = handlers["dcc_introspect__list_module"]({"module": "math"})
        assert result["success"] is True
