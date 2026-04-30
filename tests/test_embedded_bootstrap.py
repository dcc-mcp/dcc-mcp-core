"""Tests for embedded DCC adapter bootstrap helpers."""

from __future__ import annotations

import threading
from typing import Any
from typing import ClassVar

import dcc_mcp_core
from dcc_mcp_core.factory import create_dcc_server
from dcc_mcp_core.factory import make_start_stop
from dcc_mcp_core.factory import start_embedded_dcc_server


class _FakeServer:
    events: ClassVar[list[str]] = []

    def __init__(self, port: int = 8765, **kwargs: Any) -> None:
        self.port = port
        self.kwargs = kwargs
        self.is_running = False
        self.events.append("construct")
        if kwargs.get("dispatcher") is not None:
            self.events.append("dispatcher")

    def register_builtin_actions(self, **_kwargs: Any) -> None:
        self.events.append("register_builtin_actions")

    def start(self) -> str:
        self.is_running = True
        self.events.append("start")
        return "handle"

    def stop(self) -> None:
        self.is_running = False
        self.events.append("stop")


def test_start_embedded_dcc_server_creates_dispatcher_before_skill_registration() -> None:
    _FakeServer.events = []
    holder: list[Any | None] = [None]
    lock = threading.Lock()
    dispatcher = object()

    handle = start_embedded_dcc_server(
        dcc_name="blender",
        instance_holder=holder,
        lock=lock,
        server_class=_FakeServer,
        dispatcher_factory=lambda: dispatcher,
        register_builtins=True,
    )

    assert handle == "handle"
    assert holder[0].kwargs["dispatcher"] is dispatcher
    assert _FakeServer.events == ["construct", "dispatcher", "register_builtin_actions", "start"]


def test_create_dcc_server_does_not_recreate_dispatcher_for_running_singleton() -> None:
    _FakeServer.events = []
    holder: list[Any | None] = [None]
    lock = threading.Lock()
    calls = 0

    def _factory() -> object:
        nonlocal calls
        calls += 1
        return object()

    create_dcc_server(
        instance_holder=holder,
        lock=lock,
        server_class=_FakeServer,
        dispatcher_factory=_factory,
    )
    create_dcc_server(
        instance_holder=holder,
        lock=lock,
        server_class=_FakeServer,
        dispatcher_factory=_factory,
    )

    assert calls == 1
    assert _FakeServer.events == ["construct", "dispatcher", "register_builtin_actions", "start", "start"]


def test_make_start_stop_accepts_dispatcher_factory() -> None:
    _FakeServer.events = []
    dispatcher = object()
    start_server, stop_server = make_start_stop(_FakeServer, dispatcher_factory=lambda: dispatcher)

    assert start_server() == "handle"
    assert _FakeServer.events[:4] == ["construct", "dispatcher", "register_builtin_actions", "start"]
    stop_server()
    assert _FakeServer.events[-1] == "stop"


def test_start_embedded_dcc_server_exported_from_top_level() -> None:
    assert dcc_mcp_core.start_embedded_dcc_server is start_embedded_dcc_server
    assert "start_embedded_dcc_server" in dcc_mcp_core.__all__
