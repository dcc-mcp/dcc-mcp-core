"""qt_ui_inspector__list_windows tool (issue #1332)."""

from __future__ import annotations

from typing import Any

from _qt import (  # type: ignore[import-not-found]
    BindingUnavailable,
    binding_unavailable_error,
    load_qt_widgets,
    widget_summary,
)


def run(*, include_hidden: bool = False, max_results: int = 64) -> dict[str, Any]:
    cap = max(1, min(int(max_results), 256))
    try:
        binding = load_qt_widgets()
    except BindingUnavailable as exc:
        return binding_unavailable_error(exc)

    app = binding.QtWidgets.QApplication.instance()
    if app is None:
        return {
            "success": False,
            "error": "qt-no-application",
            "message": "QApplication.instance() returned None — host has no running Qt event loop.",
            "hint": "this skill must run inside a Qt host (Maya/Houdini/Nuke/Substance/Katana/etc.)",
        }

    windows: list[dict[str, Any]] = []
    for w in app.topLevelWidgets():
        try:
            if not include_hidden and not w.isVisible():
                continue
            windows.append(widget_summary(w))
            if len(windows) >= cap:
                break
        except Exception as exc:  # noqa: BLE001
            windows.append(
                {
                    "widget_id": None,
                    "class": w.__class__.__name__,
                    "error": str(exc),
                }
            )

    return {
        "success": True,
        "context": {
            "binding": binding.name,
            "window_count": len(windows),
            "include_hidden": include_hidden,
            "truncated": len(windows) >= cap,
        },
        "windows": windows,
    }
