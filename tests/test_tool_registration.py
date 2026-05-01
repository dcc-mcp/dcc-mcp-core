"""Tests for dcc_mcp_core._tool_registration (ToolSpec + register_tools)."""

from __future__ import annotations

import json
from unittest.mock import MagicMock

from dcc_mcp_core._tool_registration import ToolSpec
from dcc_mcp_core._tool_registration import register_tools


def _make_fake_server() -> tuple[MagicMock, dict]:
    server = MagicMock()
    server.registry = MagicMock()
    handlers: dict = {}
    server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
    return server, handlers


def _handler(params: object) -> dict:
    return {"ok": True, "params": params}


class TestToolSpecDefaults:
    def test_output_schema_defaults_to_none(self) -> None:
        spec = ToolSpec(
            name="demo",
            description="x",
            input_schema={"type": "object"},
            handler=_handler,
        )
        assert spec.output_schema is None

    def test_output_schema_accepts_dict(self) -> None:
        spec = ToolSpec(
            name="demo",
            description="x",
            input_schema={"type": "object"},
            handler=_handler,
            output_schema={"type": "object", "properties": {"ok": {"type": "boolean"}}},
        )
        assert spec.output_schema["properties"]["ok"] == {"type": "boolean"}


class TestRegisterTools:
    def test_calls_registry_register_with_input_schema_only(self) -> None:
        server, handlers = _make_fake_server()
        spec = ToolSpec(
            name="demo",
            description="d",
            input_schema={"type": "object"},
            handler=_handler,
        )

        assert register_tools(server, [spec]) == 1

        call = server.registry.register.call_args
        assert call.kwargs["name"] == "demo"
        # Schema is serialised to a JSON string.
        assert json.loads(call.kwargs["input_schema"]) == {"type": "object"}
        # output_schema kwarg must NOT be present when spec.output_schema is None.
        assert "output_schema" not in call.kwargs
        assert "demo" in handlers

    def test_passes_output_schema_when_present(self) -> None:
        server, _handlers = _make_fake_server()
        spec = ToolSpec(
            name="demo",
            description="d",
            input_schema={"type": "object"},
            handler=_handler,
            output_schema={"type": "object", "properties": {"ok": {"type": "boolean"}}},
        )

        register_tools(server, [spec])

        call = server.registry.register.call_args
        assert "output_schema" in call.kwargs
        assert json.loads(call.kwargs["output_schema"]) == {
            "type": "object",
            "properties": {"ok": {"type": "boolean"}},
        }

    def test_retries_without_output_schema_on_typeerror(self) -> None:
        """If the registry rejects the output_schema kwarg, we retry without it
        rather than dropping the whole registration.  Logged as a warning.
        """
        server, handlers = _make_fake_server()

        # First call raises TypeError mentioning output_schema; second succeeds.
        call_count = {"n": 0}

        def fake_register(**kwargs: object) -> None:
            call_count["n"] += 1
            if "output_schema" in kwargs:
                raise TypeError("register() got an unexpected keyword argument 'output_schema'")

        server.registry.register.side_effect = fake_register

        spec = ToolSpec(
            name="demo",
            description="d",
            input_schema={"type": "object"},
            handler=_handler,
            output_schema={"type": "object"},
        )

        assert register_tools(server, [spec]) == 1
        # First call with output_schema, second retry without.
        assert call_count["n"] == 2
        # Handler still got attached.
        assert "demo" in handlers

    def test_no_registry_logs_warning(self) -> None:
        class _BadServer:
            @property
            def registry(self) -> object:
                raise AttributeError("no registry")

        assert register_tools(_BadServer(), []) == 0
