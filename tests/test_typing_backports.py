"""Behavioral contract for standard and officially backported typing APIs."""

from __future__ import annotations

import sys
import typing

import pytest

from dcc_mcp_core._typing import Literal
from dcc_mcp_core._typing import Protocol
from dcc_mcp_core._typing import runtime_checkable
from dcc_mcp_core.schema import derive_schema


@runtime_checkable
class _Runnable(Protocol):
    name: str

    def run(self) -> int: ...


class _Runner:
    name = "runner"

    def run(self) -> int:
        return 1


class _BrokenRunner:
    name = "runner"
    run = None


def test_runtime_checkable_protocol_is_structural() -> None:
    assert isinstance(_Runner(), _Runnable)
    assert not isinstance(object(), _Runnable)
    assert not isinstance(_BrokenRunner(), _Runnable)


@pytest.mark.skipif(sys.version_info[:2] != (3, 7), reason="exercises the Python 3.7 Literal backport")
def test_literal_backport_preserves_origin_and_arguments() -> None:
    alias = Literal["fbx", "usd"]
    assert alias.__origin__ is Literal
    assert alias.__args__ == ("fbx", "usd")


def test_schema_recognizes_backported_literal_origin() -> None:
    assert derive_schema(Literal["fbx", "usd"]) == {
        "enum": ["fbx", "usd"],
        "type": "string",
    }


@pytest.mark.skipif(sys.version_info[:2] != (3, 7), reason="exercises Python 3.7 get_type_hints")
def test_literal_backport_round_trips_through_get_type_hints() -> None:
    class _ExportOptions:
        format: Literal["fbx", "usd"]  # noqa: UP037

    alias = typing.get_type_hints(_ExportOptions)["format"]
    assert alias.__origin__ is Literal
    assert alias.__args__ == ("fbx", "usd")
