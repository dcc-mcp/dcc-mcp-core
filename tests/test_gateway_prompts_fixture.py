"""Controlled ownership regressions for the prompts-SSE test fixture."""

from __future__ import annotations

from types import SimpleNamespace

import pytest

import test_e2e_gateway_prompts_sse as cases


def _handle(winner):
    handle = SimpleNamespace(is_gateway=winner, port=12345, closed=False)

    def shutdown():
        handle.closed = True

    handle.shutdown = shutdown
    return handle


def _fixture(monkeypatch, tmp_path, outcomes):
    remaining = iter(outcomes)
    ports = iter(range(24000, 24016))
    started = []

    def create_server(*args, **kwargs):
        def start():
            outcome = next(remaining)
            started.append(outcome)
            if isinstance(outcome, Exception):
                raise outcome
            return outcome

        return SimpleNamespace(start=start)

    monkeypatch.setattr(cases, "McpHttpConfig", lambda **kwargs: SimpleNamespace(**kwargs))
    monkeypatch.setattr(cases, "create_skill_server", create_server)
    monkeypatch.setattr(cases, "allocate_gateway_port", lambda: next(ports))
    monkeypatch.setattr(cases, "wait_tcp_reachable", lambda *args, **kwargs: True)
    return cases.gateway_with_prompts_skill.__wrapped__(tmp_path), started


def test_fixture_closes_lost_election_before_yielding_owned_gateway(monkeypatch, tmp_path):
    loser, winner = _handle(False), _handle(True)
    fixture, started = _fixture(monkeypatch, tmp_path, [loser, winner])
    try:
        result = next(fixture)
        assert result["handle"] is winner
        assert result["gateway_url"] == "http://127.0.0.1:24001/mcp"
        assert loser.closed
        assert not winner.closed
        assert len(started) == 2
    finally:
        fixture.close()
    assert winner.closed


def test_fixture_fails_closed_after_bounded_lost_elections(monkeypatch, tmp_path):
    losers = [_handle(False) for _ in range(8)]
    fixture, started = _fixture(monkeypatch, tmp_path, losers)
    try:
        with pytest.raises(pytest.fail.Exception, match="single-backend gateway"):
            next(fixture)
    finally:
        fixture.close()
    assert len(started) == 8
    assert all(handle.closed for handle in losers)


def test_fixture_retries_only_known_port_collision(monkeypatch, tmp_path):
    winner = _handle(True)
    fixture, started = _fixture(monkeypatch, tmp_path, [RuntimeError("os error 10048"), winner])
    try:
        assert next(fixture)["handle"] is winner
        assert len(started) == 2
    finally:
        fixture.close()
    assert winner.closed


def test_fixture_does_not_hide_other_startup_errors(monkeypatch, tmp_path):
    fixture, started = _fixture(monkeypatch, tmp_path, [RuntimeError("invalid configuration")])
    with pytest.raises(RuntimeError, match="invalid configuration"):
        next(fixture)
    assert len(started) == 1


@pytest.mark.parametrize("readiness", [(False,), (True, False)])
def test_fixture_closes_owned_gateway_when_readiness_fails(monkeypatch, tmp_path, readiness):
    winner = _handle(True)
    fixture, _ = _fixture(monkeypatch, tmp_path, [winner])
    remaining = iter(readiness)
    monkeypatch.setattr(cases, "wait_tcp_reachable", lambda *args, **kwargs: next(remaining))
    try:
        with pytest.raises(AssertionError, match="unreachable after start"):
            next(fixture)
    finally:
        fixture.close()
    assert winner.closed
