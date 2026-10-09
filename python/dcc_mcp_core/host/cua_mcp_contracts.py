"""Exact JSON contract comparison shared by opt-in MCP capabilities."""

from __future__ import annotations

from typing import Any


def typed_equal(actual: Any, expected: Any) -> bool:
    """Compare closed JSON values without treating booleans as integers."""
    if type(actual) is not type(expected):
        return False
    if isinstance(expected, dict):
        return actual.keys() == expected.keys() and all(typed_equal(actual[k], v) for k, v in expected.items())
    if isinstance(expected, list):
        return len(actual) == len(expected) and all(typed_equal(a, b) for a, b in zip(actual, expected))
    return actual == expected
