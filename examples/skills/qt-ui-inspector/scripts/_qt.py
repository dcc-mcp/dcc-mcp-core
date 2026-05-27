"""Lazy Qt-binding discovery + stable widget identifiers (issue #1332).

This module is the single boundary between the Qt-UI-inspector tools and
whatever Qt binding the host actually has. Every tool calls
:func:`load_qt_widgets` once at the top, and treats a `BindingUnavailable`
result as a clean structured error instead of crashing.

Supported bindings, in order: ``qtpy`` → ``PySide6`` → ``PySide2`` →
``PyQt6`` → ``PyQt5``.

Widget identifiers are stable strings built from ``(class name,
object name, id() fingerprint)`` so an agent can pass an id back to a
follow-up call within the same DCC session without holding a raw
``PyObject*`` handle.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass
class QtBinding:
    name: str
    QtWidgets: Any
    QtCore: Any


class BindingUnavailable(RuntimeError):
    """Raised by :func:`load_qt_widgets` when no Qt binding is importable."""


_SUPPORTED = ("qtpy", "PySide6", "PySide2", "PyQt6", "PyQt5")


def load_qt_widgets() -> QtBinding:
    """Discover an importable Qt binding. Raises ``BindingUnavailable`` if none."""
    errors: list[str] = []
    for name in _SUPPORTED:
        try:
            module = __import__(name)
        except Exception as exc:  # noqa: BLE001
            errors.append(f"{name}: {exc.__class__.__name__}")
            continue
        try:
            qt_widgets = __import__(f"{name}.QtWidgets", fromlist=["QtWidgets"])
            qt_core = __import__(f"{name}.QtCore", fromlist=["QtCore"])
        except Exception as exc:  # noqa: BLE001
            errors.append(f"{name}.QtWidgets/QtCore: {exc.__class__.__name__}")
            continue
        return QtBinding(name=name, QtWidgets=qt_widgets, QtCore=qt_core)
    raise BindingUnavailable(
        "No Qt binding importable. Tried: " + ", ".join(_SUPPORTED) + ". Errors: " + "; ".join(errors)
    )


def widget_id(widget: Any) -> str:
    """Stable identifier for ``widget``.

    The fingerprint piece is ``id(widget) & 0xFFFFFFFF`` so consecutive
    calls within the same session round-trip while not leaking the full
    process address space.
    """
    try:
        obj_name = widget.objectName() or ""
    except Exception:  # noqa: BLE001
        obj_name = ""
    klass = widget.__class__.__name__
    fingerprint = id(widget) & 0xFFFFFFFF
    return f"{klass}:{obj_name}:{fingerprint:08x}"


def widget_summary(widget: Any) -> dict[str, Any]:
    """Bounded JSON-safe summary of a widget."""
    try:
        rect = widget.geometry()
        geometry = {
            "x": int(rect.x()),
            "y": int(rect.y()),
            "width": int(rect.width()),
            "height": int(rect.height()),
        }
    except Exception:  # noqa: BLE001
        geometry = None
    try:
        children_count = len(list(widget.children()))
    except Exception:  # noqa: BLE001
        children_count = 0
    out: dict[str, Any] = {
        "widget_id": widget_id(widget),
        "class": widget.__class__.__name__,
        "object_name": _safe_call(widget.objectName, ""),
        "visible": _safe_call(widget.isVisible, False),
        "enabled": _safe_call(widget.isEnabled, False),
        "children_count": children_count,
    }
    if geometry is not None:
        out["geometry"] = geometry
    # accessible name / description are optional on some widgets
    out["accessible_name"] = _safe_call(getattr(widget, "accessibleName", None), "")
    out["accessible_description"] = _safe_call(
        getattr(widget, "accessibleDescription", None), ""
    )
    return out


def _safe_call(fn: Any, default: Any) -> Any:
    if fn is None or not callable(fn):
        return default
    try:
        return fn()
    except Exception:  # noqa: BLE001
        return default


def binding_unavailable_error(exc: BindingUnavailable) -> dict[str, Any]:
    """Build the structured ``qt-binding-unavailable`` error envelope."""
    return {
        "success": False,
        "error": "qt-binding-unavailable",
        "message": str(exc),
        "hint": (
            "install one of qtpy, PySide6, PySide2, PyQt6, PyQt5 in the host "
            "Python environment, then reload the skill"
        ),
    }


__all__ = [
    "BindingUnavailable",
    "QtBinding",
    "binding_unavailable_error",
    "load_qt_widgets",
    "widget_id",
    "widget_summary",
]
