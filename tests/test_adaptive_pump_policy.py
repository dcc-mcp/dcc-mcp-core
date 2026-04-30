"""Tests for the reusable adaptive DCC pump policy (#606)."""

from __future__ import annotations

import pytest

import dcc_mcp_core
from dcc_mcp_core import AdaptivePumpPolicy
from dcc_mcp_core import AdaptivePumpStats


class _Clock:
    def __init__(self) -> None:
        self.now = 0.0

    def __call__(self) -> float:
        return self.now

    def advance(self, seconds: float) -> None:
        self.now += seconds


def test_adaptive_pump_policy_exported() -> None:
    assert dcc_mcp_core.AdaptivePumpPolicy is AdaptivePumpPolicy
    assert dcc_mcp_core.AdaptivePumpStats is AdaptivePumpStats
    assert "AdaptivePumpPolicy" in dcc_mcp_core.__all__
    assert "AdaptivePumpStats" in dcc_mcp_core.__all__


def test_policy_starts_active_then_backs_off_after_idle_delay() -> None:
    clock = _Clock()
    policy = AdaptivePumpPolicy(
        active_interval_secs=0.05,
        idle_interval_secs=1.0,
        idle_delay_secs=5.0,
        clock=clock,
    )

    assert policy.next_interval() == 0.05
    clock.advance(4.9)
    assert policy.next_interval() == 0.05
    clock.advance(0.2)
    assert policy.next_interval() == 1.0
    assert policy.mode == "idle"
    assert policy.stats.idle_transitions == 1


def test_mark_work_done_returns_to_active_and_updates_stats() -> None:
    clock = _Clock()
    policy = AdaptivePumpPolicy(
        active_interval_secs=0.1,
        idle_interval_secs=2.0,
        idle_delay_secs=1.0,
        clock=clock,
    )
    clock.advance(2.0)
    assert policy.next_interval() == 2.0

    policy.mark_work_done(drained=3, overrun=True)
    assert policy.next_interval() == 0.1
    assert policy.stats.ticks == 1
    assert policy.stats.drained_jobs == 3
    assert policy.stats.overrun_cycles == 1
    assert policy.stats.active_transitions == 1


def test_pending_and_deferred_jobs_keep_policy_active() -> None:
    clock = _Clock()
    policy = AdaptivePumpPolicy(
        active_interval_secs=0.05,
        idle_interval_secs=1.0,
        idle_delay_secs=0.0,
        clock=clock,
    )
    clock.advance(10.0)

    assert policy.next_interval(has_pending=True) == 0.05
    assert policy.next_interval(has_pending=False, deferred_pending=True) == 0.05

    assert policy.next_interval() == 1.0


def test_client_idle_limit_allows_backoff_even_inside_idle_delay() -> None:
    clock = _Clock()
    policy = AdaptivePumpPolicy(
        active_interval_secs=0.05,
        idle_interval_secs=1.0,
        idle_delay_secs=60.0,
        max_client_idle_secs=10.0,
        clock=clock,
    )
    clock.advance(11.0)

    assert policy.next_interval() == 1.0

    policy.mark_client_activity()
    assert policy.next_interval() == 0.05


def test_policy_validates_configuration() -> None:
    with pytest.raises(ValueError, match="active_interval_secs"):
        AdaptivePumpPolicy(active_interval_secs=0)
    with pytest.raises(ValueError, match="idle_interval_secs"):
        AdaptivePumpPolicy(idle_interval_secs=0)
    with pytest.raises(ValueError, match="idle_delay_secs"):
        AdaptivePumpPolicy(idle_delay_secs=-1)
    with pytest.raises(ValueError, match="max_client_idle_secs"):
        AdaptivePumpPolicy(max_client_idle_secs=-1)


def test_record_tick_rejects_negative_drained_count() -> None:
    policy = AdaptivePumpPolicy()
    with pytest.raises(ValueError, match="drained"):
        policy.record_tick(drained=-1)
