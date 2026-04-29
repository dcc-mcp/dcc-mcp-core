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


# ── Issue #567: job_recovery policy contract ─────────────────────────────


def test_job_recovery_default_is_drop() -> None:
    """Existing callers inherit today's behaviour without touching their
    config. ``"drop"`` is the only policy that's actually implemented.
    """
    cfg = McpHttpConfig(port=8765)
    assert cfg.job_recovery == "drop"


@pytest.mark.parametrize("policy", ["drop", "requeue"])
def test_job_recovery_setter_round_trips(policy: str) -> None:
    """Both wire identifiers round-trip through the setter. ``"requeue"``
    is accepted today (degrades to ``"drop"`` at server startup) so DCC
    adapters can plumb the knob now and pick up real requeue without a
    config-shape break when it ships.
    """
    cfg = McpHttpConfig(port=8765)
    cfg.job_recovery = policy
    assert cfg.job_recovery == policy


@pytest.mark.parametrize("raw", ["DROP", "Drop", "Requeue", "  requeue  "])
def test_job_recovery_setter_is_case_insensitive(raw: str) -> None:
    """Tolerates env-var plumbing such as ``DCC_MCP_*_JOB_RECOVERY=Requeue``
    where adapters may emit canonical-case strings.
    """
    cfg = McpHttpConfig(port=8765)
    cfg.job_recovery = raw
    assert cfg.job_recovery == raw.strip().lower()


def test_job_recovery_setter_rejects_unknown_value() -> None:
    """Unknown policies surface a descriptive ``ValueError`` that names
    the rejected value and the accepted set, so misconfigured adapters
    fail loudly instead of silently.
    """
    cfg = McpHttpConfig(port=8765)
    with pytest.raises(ValueError) as info:
        cfg.job_recovery = "retry"
    msg = str(info.value)
    assert "retry" in msg and "drop" in msg and "requeue" in msg
