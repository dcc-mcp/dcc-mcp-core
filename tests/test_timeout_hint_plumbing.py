"""Regression tests for issue #999 — timeout_hint_secs → dispatcher timeout_ms."""

from __future__ import annotations

import logging

from dcc_mcp_core._server.inprocess_executor import timeout_hint_secs_to_ms


def test_timeout_hint_secs_to_ms_converts_seconds() -> None:
    assert timeout_hint_secs_to_ms(5, warn_if_missing=False) == 5_000


def test_timeout_hint_secs_to_ms_none_returns_none() -> None:
    assert timeout_hint_secs_to_ms(None, warn_if_missing=False) is None


def test_timeout_hint_secs_to_ms_caps_overflow() -> None:
    assert timeout_hint_secs_to_ms(9_999, warn_if_missing=False) == 3_600_000


def test_timeout_hint_secs_to_ms_warns_on_async_main_without_hint(caplog) -> None:
    caplog.set_level(logging.WARNING)
    assert (
        timeout_hint_secs_to_ms(
            None,
            action_name="render_frames",
            skill_name="maya-render",
            thread_affinity="main",
            execution="async",
        )
        is None
    )
    assert any("timeout_hint_secs missing" in rec.message for rec in caplog.records)
