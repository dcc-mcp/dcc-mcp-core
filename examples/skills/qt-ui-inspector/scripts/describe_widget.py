"""qt_ui_inspector__describe_widget tool (issue #1332)."""

from __future__ import annotations

from typing import Any

from _qt import (  # type: ignore[import-not-found]
    BindingUnavailable,
    binding_unavailable_error,
    load_qt_widgets,
    widget_id,
    widget_summary,
)


_PROPERTY_CAP = 32


def _find_by_id(app, target_id: str):
    for widget in app.allWidgets():
        try:
            if widget_id(widget) == target_id:
                return widget
        except Exception:  # noqa: BLE001
            continue
    return None


def run(*, widget_id: str) -> dict[str, Any]:  # noqa: A002
    if not widget_id:
        return {"success": False, "error": "invalid-input", "message": "widget_id is required"}
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

    widget = _find_by_id(app, widget_id)
    if widget is None:
        return {
            "success": False,
            "error": "widget-not-found",
            "message": f"no widget matches widget_id={widget_id!r} in this session",
            "hint": "call list_windows / find_widgets again — the widget may have been destroyed",
        }

    summary = widget_summary(widget)
    properties: dict[str, Any] = {}
    try:
        meta = widget.metaObject()
        for i in range(meta.propertyCount()):
            if len(properties) >= _PROPERTY_CAP:
                break
            prop = meta.property(i)
            try:
                name = prop.name()
                value = widget.property(name)
            except Exception:  # noqa: BLE001
                continue
            try:
                properties[str(name)] = _coerce_property(value)
            except Exception:  # noqa: BLE001
                properties[str(name)] = "<unserialisable>"
    except Exception:  # noqa: BLE001
        properties = {}

    summary["properties"] = properties
    summary["properties_truncated"] = len(properties) >= _PROPERTY_CAP

    return {
        "success": True,
        "context": {
            "binding": binding.name,
        },
        "widget": summary,
    }


def _coerce_property(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, (list, tuple)):
        return [_coerce_property(v) for v in list(value)[:32]]
    return str(value)
