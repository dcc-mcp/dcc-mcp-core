"""Tests for the default-capability Qt UI inspector (issue #1332)."""

from __future__ import annotations

import os

# Force headless Qt platform before any binding is loaded by pytest-qt.
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

import pytest

from dcc_mcp_core import (
    qt_describe_widget,
    qt_find_widgets,
    qt_list_windows,
    qt_snapshot_tree,
    qt_wait_for_widget,
    register_qt_ui_inspector,
)


@pytest.fixture
def _hide_qt_bindings(monkeypatch):
    """Force every supported Qt binding to be unimportable.

    Opt-in fixture (NOT ``autouse``) so live-Qt tests can run in the
    same session without their ``QApplication`` / module cache being
    poisoned. Used by the "Qt missing" contract tests below.
    """
    real_import = __import__
    bindings = ("qtpy", "PySide6", "PySide2", "PyQt6", "PyQt5")

    def fake_import(name, globals=None, locals=None, fromlist=(), level=0):
        for binding in bindings:
            if name == binding or name.startswith(f"{binding}."):
                raise ImportError(f"forced unavailable: {name}")
        return real_import(name, globals, locals, fromlist, level)

    monkeypatch.setattr("builtins.__import__", fake_import)


def _assert_unavailable(result: dict) -> None:
    assert result["success"] is False
    assert result["error"] == "qt-binding-unavailable"
    assert "install" in result["context"]["hint"].lower()


@pytest.mark.usefixtures("_hide_qt_bindings")
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


@pytest.mark.usefixtures("_hide_qt_bindings")
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


# ── Live-Qt tests (pytest-qt + PySide6) ────────────────────────────────
# These cover the six business surfaces of the inspector against a real
# QApplication. Skipped when pytest-qt or any Qt binding is missing.

pytest.importorskip("pytestqt", reason="pytest-qt not installed")
pytest.importorskip("PySide6.QtWidgets", reason="PySide6 not installed")


@pytest.mark.qt
class TestWithLiveQt:
    """Exercise the inspector against a real Qt event loop via pytest-qt."""

    def test_widget_id_is_stable_and_unique(self, qtbot) -> None:
        from PySide6.QtWidgets import QWidget

        from dcc_mcp_core.skills.qt_ui_inspector import _widget_id

        w1 = QWidget()
        w2 = QWidget()
        w1.setObjectName("alpha")
        w2.setObjectName("alpha")
        qtbot.addWidget(w1)
        qtbot.addWidget(w2)

        # Same widget → identical id; different widgets with same name → distinct
        assert _widget_id(w1) == _widget_id(w1)
        assert _widget_id(w1) != _widget_id(w2)
        # Class + object-name appear in the id
        assert _widget_id(w1).startswith("QWidget:alpha:")

    def test_list_windows_finds_visible_top_level_only(self, qtbot) -> None:
        from PySide6.QtWidgets import QWidget

        shown = QWidget()
        shown.setObjectName("shown_win")
        shown.show()
        qtbot.addWidget(shown)
        qtbot.waitExposed(shown)

        hidden = QWidget()
        hidden.setObjectName("hidden_win")
        qtbot.addWidget(hidden)  # never shown

        visible = qt_list_windows(include_hidden=False)
        assert visible["success"] is True
        names = {w["object_name"] for w in visible["context"]["windows"]}
        assert "shown_win" in names
        assert "hidden_win" not in names

        every = qt_list_windows(include_hidden=True)
        names_all = {w["object_name"] for w in every["context"]["windows"]}
        assert "shown_win" in names_all and "hidden_win" in names_all

    def test_find_widgets_matches_exact_substring_regex(self, qtbot) -> None:
        from PySide6.QtWidgets import QPushButton, QWidget

        root = QWidget()
        a = QPushButton("a", parent=root)
        a.setObjectName("button_save")
        b = QPushButton("b", parent=root)
        b.setObjectName("button_cancel")
        c = QPushButton("c", parent=root)
        c.setObjectName("other")
        root.show()
        qtbot.addWidget(root)
        qtbot.waitExposed(root)

        exact = qt_find_widgets(object_name="button_save", object_name_match="exact")
        assert exact["context"]["match_count"] == 1

        sub = qt_find_widgets(object_name="button_", object_name_match="substring")
        names = sorted(w["object_name"] for w in sub["context"]["widgets"])
        assert names == ["button_cancel", "button_save"]

        rx = qt_find_widgets(object_name="^button_(save|cancel)$", object_name_match="regex")
        assert rx["context"]["match_count"] == 2

        by_class = qt_find_widgets(class_name="QPushButton", visible_only=True)
        assert by_class["context"]["match_count"] >= 3

    def test_describe_widget_returns_capped_property_snapshot(self, qtbot) -> None:
        from PySide6.QtWidgets import QPushButton

        from dcc_mcp_core.skills.qt_ui_inspector import _PROPERTY_CAP, _widget_id

        btn = QPushButton("Click me")
        btn.setObjectName("describable")
        btn.show()
        qtbot.addWidget(btn)
        qtbot.waitExposed(btn)

        desc = qt_describe_widget(widget_id=_widget_id(btn))
        assert desc["success"] is True
        widget = desc["context"]["widget"]
        assert widget["class"] == "QPushButton"
        assert widget["object_name"] == "describable"
        assert "geometry" in widget and widget["geometry"]["width"] > 0
        # cap is enforced and reported when exceeded (QPushButton has >32 meta-properties)
        assert len(widget["properties"]) == _PROPERTY_CAP
        assert widget["properties_truncated"] is True
        # QObject-level property is included (it is enumerated first by metaObject)
        assert "objectName" in widget["properties"]

    def test_describe_widget_returns_not_found_for_stale_id(self, qtbot) -> None:
        r = qt_describe_widget(widget_id="QWidget:ghost:deadbeef")
        assert r["success"] is False
        assert r["error"] == "not_found"

    def test_snapshot_tree_respects_depth_and_node_budget(self, qtbot) -> None:
        from PySide6.QtWidgets import QWidget

        from dcc_mcp_core.skills.qt_ui_inspector import _widget_id

        # 4-level nesting: root → a → b → c
        root = QWidget()
        a = QWidget(parent=root)
        b = QWidget(parent=a)
        QWidget(parent=b)
        root.show()
        qtbot.addWidget(root)
        qtbot.waitExposed(root)

        shallow = qt_snapshot_tree(root_widget_id=_widget_id(root), max_depth=1)
        assert shallow["success"] is True
        tree = shallow["context"]["tree"]
        # depth=1 → root has children but their children are pruned
        assert tree["children"] and "children" not in tree["children"][0] \
            or tree["children"][0]["children"] == []

        capped = qt_snapshot_tree(root_widget_id=_widget_id(root), max_depth=8, max_nodes=2)
        assert capped["success"] is True
        # budget exhausted → nodes_remaining is 0
        assert capped["context"]["nodes_remaining"] == 0

    def test_wait_for_widget_returns_immediately_when_present(self, qtbot) -> None:
        from PySide6.QtWidgets import QWidget

        already = QWidget()
        already.setObjectName("already_present")
        already.show()
        qtbot.addWidget(already)
        qtbot.waitExposed(already)

        r = qt_wait_for_widget(
            object_name="already_present",
            timeout_ms=1000,
            poll_interval_ms=25,
        )
        assert r["success"] is True
        assert r["context"]["widget"]["object_name"] == "already_present"
        # First scan finds it; no extra polling required.
        assert r["context"]["polls"] == 1

    def test_wait_for_widget_times_out_when_missing(self, qtbot) -> None:
        r = qt_wait_for_widget(
            object_name="never_existing_widget_xyzzy",
            timeout_ms=150,
            poll_interval_ms=25,
        )
        assert r["success"] is False
        assert r["error"] == "timeout"
        assert r["context"]["polls"] >= 1

