"""Readiness publication must preserve backend rejection, including sidecars."""

from __future__ import annotations

from types import SimpleNamespace

import pytest

from _support import make_test_server
from dcc_mcp_core import AdapterReadinessBinder
from dcc_mcp_core import ReadinessProbe
from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer


@pytest.mark.parametrize("wrapped", [False, True])
@pytest.mark.parametrize("result", [None, True, False])
def test_publication_preserves_setter_result(wrapped: bool, result) -> None:
    accepted = []

    def setter(probe):
        accepted.append(probe)
        return result

    inner = SimpleNamespace(set_readiness_probe=setter)
    server = make_test_server(server=inner, dcc_name="python") if wrapped else inner
    binder = AdapterReadinessBinder.bind_inline(server)

    assert accepted == [binder.probe]
    assert binder.published is (result is not False)
    assert binder.publish() is (result is not False)


@pytest.mark.parametrize("wrapped", [False, True])
def test_real_sidecar_facade_rejects_in_process_probe(wrapped: bool) -> None:
    # Exercise the real facade, without launching a sidecar or claiming host QA.
    inner = SidecarBackedSkillServer("python", McpHttpConfig(), host_rpc="")
    server = make_test_server(server=inner, dcc_name="python") if wrapped else inner

    assert server.set_readiness_probe(ReadinessProbe()) is False
    binder = AdapterReadinessBinder.bind_inline(server)

    assert binder.published is False
    assert binder.publish() is False
    # A green local bitset is not evidence that the HTTP backend consumed it.
    assert binder.probe.is_ready() is True


@pytest.mark.parametrize("wrapped", [False, True])
@pytest.mark.parametrize("unavailable", ["missing", "raises", "rejects"])
def test_failed_republication_clears_acceptance(wrapped: bool, unavailable: str) -> None:
    inner = SimpleNamespace(set_readiness_probe=lambda probe: None)
    server = make_test_server(server=inner, dcc_name="python") if wrapped else inner
    binder = AdapterReadinessBinder.bind_inline(server)
    assert binder.published is True

    def fail(probe):
        raise RuntimeError("probe unavailable")

    if unavailable == "missing":
        del inner.set_readiness_probe
    elif unavailable == "raises":
        inner.set_readiness_probe = fail
    else:
        inner.set_readiness_probe = lambda probe: False

    assert binder.publish() is False
    assert binder.published is False
