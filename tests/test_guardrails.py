"""Tests for adapter-extensible weak execution guardrails (#605)."""

from __future__ import annotations

import sys
from types import SimpleNamespace

import pytest

import dcc_mcp_core
from dcc_mcp_core import DccBlockedCall
from dcc_mcp_core import DccGuardrailError
from dcc_mcp_core import DccWeakSandbox


def test_guardrail_symbols_exported() -> None:
    assert dcc_mcp_core.DccBlockedCall is DccBlockedCall
    assert dcc_mcp_core.DccGuardrailError is DccGuardrailError
    assert dcc_mcp_core.DccWeakSandbox is DccWeakSandbox
    assert "DccBlockedCall" in dcc_mcp_core.__all__
    assert "DccGuardrailError" in dcc_mcp_core.__all__
    assert "DccWeakSandbox" in dcc_mcp_core.__all__


def test_blocked_call_raises_clear_error_and_restores() -> None:
    host = SimpleNamespace()

    def original_quit() -> str:
        return "ok"

    host.quit = original_quit

    with DccWeakSandbox(
        blocked_calls=[
            DccBlockedCall(
                "host.quit",
                reason="terminates the host process",
                target=host,
                attribute="quit",
            )
        ]
    ):
        with pytest.raises(DccGuardrailError, match=r"host\.quit"):
            host.quit()
        with pytest.raises(DccGuardrailError, match="terminates the host process"):
            host.quit()

    assert host.quit is original_quit
    assert host.quit() == "ok"


def test_attr_overrides_are_scoped_and_restored() -> None:
    class _Host:
        value = 1

    host = _Host()

    with DccWeakSandbox(attr_overrides={host: {"value": 2, "temporary": "added"}}):
        assert host.value == 2
        assert host.temporary == "added"

    assert host.value == 1
    assert not hasattr(host, "temporary")


def test_restores_original_on_exception() -> None:
    host = SimpleNamespace(exit=lambda: "ok")

    with pytest.raises(RuntimeError, match="boom"):
        with DccWeakSandbox(blocked_calls=[DccBlockedCall("host.exit", "terminates the process", host, "exit")]):
            raise RuntimeError("boom")

    assert host.exit() == "ok"


def test_metadata_only_blocked_calls_are_allowed() -> None:
    with DccWeakSandbox(blocked_calls=[DccBlockedCall("maya.cmds.file(new=True)", "resets the scene")]):
        pass


def test_custom_override_can_wrap_sys_exit() -> None:
    original_exit = sys.exit

    with DccWeakSandbox(
        attr_overrides={sys: {"exit": DccWeakSandbox.blocked_callable("sys.exit", "terminates embedded Python")}}
    ):
        with pytest.raises(DccGuardrailError, match=r"sys\.exit"):
            sys.exit(0)

    assert sys.exit is original_exit
