"""qt_ui_inspector__snapshot_tree tool (issue #1332)."""

from __future__ import annotations

from typing import Any

from _qt import (  # type: ignore[import-not-found]
    BindingUnavailable,
    binding_unavailable_error,
    load_qt_widgets,
    widget_id,
    widget_summary,
)


def _find_by_id(app, target_id: str):
    for widget in app.allWidgets():
        try:
            if widget_id(widget) == target_id:
                return widget
        except Exception:  # noqa: BLE001
            continue
    return None


def _walk(widget, depth: int, max_depth: int, budget: list[int]) -> dict[str, Any]:
    if budget[0] <= 0:
        return {"widget_id": widget_id(widget), "class": widget.__class__.__name__, "truncated": True}
    budget[0] -= 1
    node = widget_summary(widget)
    if depth >= max_depth:
        node["children"] = []
        node["truncated"] = bool(list(widget.children()))
        return node
    children: list[dict[str, Any]] = []
    try:
        for child in widget.children():
            if not hasattr(child, "isWidgetType") or not child.isWidgetType():
                continue
            if budget[0] <= 0:
                children.append({"truncated": True})
                break
            children.append(_walk(child, depth + 1, max_depth, budget))
    except Exception:  # noqa: BLE001
        children = []
    node["children"] = children
    return node


def run(
    *,
    root_widget_id: str | None = None,
    max_depth: int = 4,
    max_nodes: int = 256,
) -> dict[str, Any]:
    depth = max(0, min(int(max_depth), 16))
    cap = max(1, min(int(max_nodes), 4096))
    try:
        binding = load_qt_widgets()
    except BindingUnavailable as exc:
        return binding_unavailable_error(exc)

    app = binding.QtWidgets.QApplication.instance()
    if app is None:
        return {
            "success": False,
            "error": "qt-no-application",
            "message": "QApplication.instance() returned None.",
        }

    budget = [cap]
    if root_widget_id is None:
        trees = [_walk(w, 0, depth, budget) for w in app.topLevelWidgets() if budget[0] > 0]
        return {
            "success": True,
            "context": {
                "binding": binding.name,
                "root_widget_id": None,
                "max_depth": depth,
                "nodes_remaining": budget[0],
            },
            "trees": trees,
        }

    root = _find_by_id(app, root_widget_id)
    if root is None:
        return {
            "success": False,
            "error": "widget-not-found",
            "message": f"no widget matches root_widget_id={root_widget_id!r}",
        }
    tree = _walk(root, 0, depth, budget)
    return {
        "success": True,
        "context": {
            "binding": binding.name,
            "root_widget_id": root_widget_id,
            "max_depth": depth,
            "nodes_remaining": budget[0],
        },
        "tree": tree,
    }
