"""Tests for :class:`dcc_mcp_core.McpHttpConfig` Python bindings.

Issue #314 — the gateway's per-backend fan-out timeout must be configurable
from Python so downstream DCC adapters (Maya/Blender/Houdini…) can extend
it for workflows that legitimately run longer than the legacy 10-second
ceiling (scene import, simulation bake, USD composition).
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import McpHttpConfig


def test_backend_timeout_ms_has_sensible_default() -> None:
    """Default raised from 10 s → 120 s (issue #314 follow-up).

    DCC scene operations (mesh import, simulation bake, render, complex
    keyframe setup) regularly take tens of seconds. The previous 10-second
    default caused the gateway to cancel legitimate tool calls while the
    backend was still working. For truly long operations prefer async dispatch
    (``_meta.dcc.async=true``) which returns a ``job_id`` immediately.
    """
    cfg = McpHttpConfig(port=8765)
    assert cfg.backend_timeout_ms == 120_000


def test_backend_timeout_ms_constructor_kwarg() -> None:
    """Long-running DCC tools (scene import, sim bake) need a larger budget."""
    cfg = McpHttpConfig(port=8765, backend_timeout_ms=120_000)
    assert cfg.backend_timeout_ms == 120_000


def test_backend_timeout_ms_setter_round_trips() -> None:
    """The property must be mutable so config objects can be tuned after
    construction (e.g. from a user-supplied TOML/JSON config file).
    """
    cfg = McpHttpConfig(port=8765)
    cfg.backend_timeout_ms = 45_000
    assert cfg.backend_timeout_ms == 45_000


@pytest.mark.parametrize("value", [0, 1, 10_000, 120_000, 3_600_000])
def test_backend_timeout_ms_accepts_wide_range(value: int) -> None:
    """Guard against accidental upper-bound clamps. ``0`` disables the
    per-request timeout entirely (reqwest semantics); very large values
    are valid for batch-style backends.
    """
    cfg = McpHttpConfig(port=8765, backend_timeout_ms=value)
    assert cfg.backend_timeout_ms == value
