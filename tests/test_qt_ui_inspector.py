"""Tests for the default-capability Qt UI inspector (issue #1332)."""

from __future__ import annotations

import sys

import pytest

from dcc_mcp_core import (
    qt_describe_widget,
    qt_find_widgets,
    qt_list_windows,
    qt_snapshot_tree,
    qt_wait_for_widget,
    register_qt_ui_inspector,
)


@pytest.fixture(autouse=True)
def _hide_qt_bindings(monkeypatch):
    """Force every supported Qt binding to be unimportable.

    Tests that exercise live Qt require a real Qt event loop and are
    out of scope for unit tests; this fixture verifies the structured
    ``qt-binding-unavailable`` envelope, which is the contract every
    adapter must rely on when Qt is absent.
    """
    real_import = __import__

    def fake_import(name, globals=None, locals=None, fromlist=(), level=0):
        for binding in ("qtpy", "PySide6", "PySide2", "PyQt6", "PyQt5"):
            if name == binding or name.startswith(f"{binding}."):
                raise ImportError(f"forced unavailable: {name}")
        return real_import(name, globals, locals, fromlist, level)

    monkeypatch.setattr("builtins.__import__", fake_import)
    # Wipe any pre-cached Qt modules
    for mod in list(sys.modules):
        if any(mod == b or mod.startswith(f"{b}.") for b in ("qtpy", "PySide6", "PySide2", "PyQt6", "PyQt5")):
            sys.modules.pop(mod, None)


def _assert_unavailable(result: dict) -> None:
    assert result["success"] is False
    assert result["error"] == "qt-binding-unavailable"
    assert "install" in result["context"]["hint"].lower()


class TestStructuredFailureWhenQtMissing:
    def test_list_windows_returns_unavailable(self) -> None:
        _assert_unavailable(qt_list_windows())

    def test_find_widgets_returns_unavailable(self) -> None:
        _assert_unavailable(qt_find_widgets(object_name="anything"))

    def test_describe_widget_returns_unavailable_when_id_supplied(self) -> None:
        _assert_unavailable(qt_describe_widget(widget_id="QPushButton::abc:00000001"))

    def test_snapshot_tree_returns_unavailable(self) -> None:
        _assert_unavailable(qt_snapshot_tree())

    def test_wait_for_widget_returns_unavailable(self) -> None:
        _assert_unavailable(qt_wait_for_widget(object_name="dialog"))


class TestInputValidationRunsBeforeBindingLoad:
    """Input validation must reject malformed calls without touching Qt."""

    def test_describe_widget_rejects_empty_id(self) -> None:
        r = qt_describe_widget(widget_id="")
        assert r["success"] is False
        assert r["error"] == "invalid_input"

    def test_find_widgets_rejects_no_criteria(self) -> None:
        r = qt_find_widgets()
        assert r["success"] is False
        assert r["error"] == "invalid_input"

    def test_wait_for_widget_rejects_no_criteria(self) -> None:
        r = qt_wait_for_widget()
        assert r["success"] is False
        assert r["error"] == "invalid_input"


class _FakeRegistry:
    def __init__(self) -> None:
        self.tools: list[str] = []

    def register(self, **kwargs) -> None:
        self.tools.append(kwargs.get("name"))


class _FakeServer:
    def __init__(self) -> None:
        self.registry = _FakeRegistry()
        self.handlers: dict[str, object] = {}

    def register_handler(self, name: str, handler) -> None:
        self.handlers[name] = handler


class TestRegisterQtUiInspector:
    def test_registers_all_five_tools(self) -> None:
        server = _FakeServer()
        register_qt_ui_inspector(server, dcc_name="maya")
        expected = {
            "qt_ui_inspector__list_windows",
            "qt_ui_inspector__find_widgets",
            "qt_ui_inspector__describe_widget",
            "qt_ui_inspector__snapshot_tree",
            "qt_ui_inspector__wait_for_widget",
        }
        assert expected.issubset(set(server.handlers))

    def test_handlers_accept_json_string_and_dict(self) -> None:
        server = _FakeServer()
        register_qt_ui_inspector(server)
        handler = server.handlers["qt_ui_inspector__list_windows"]
        # dict form
        r_dict = handler({"include_hidden": True})
        # json-string form
        r_json = handler('{"include_hidden": true}')
        # Both must fail with qt-binding-unavailable (Qt is forced missing)
        _assert_unavailable(r_dict)
        _assert_unavailable(r_json)
