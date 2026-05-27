"""qt_ui_inspector__wait_for_widget tool (issue #1332)."""

from __future__ import annotations

import time
from typing import Any

from _qt import (  # type: ignore[import-not-found]
    BindingUnavailable,
    binding_unavailable_error,
    load_qt_widgets,
    widget_summary,
)


def _match(widget, object_name: str | None, class_name: str | None) -> bool:
    try:
        if object_name is not None and (widget.objectName() or "") != object_name:
            return False
        if class_name is not None and class_name not in widget.__class__.__name__:
            return False
        return True
    except Exception:  # noqa: BLE001
        return False


def run(
    *,
    object_name: str | None = None,
    class_name: str | None = None,
    visible: bool = True,
    enabled: bool = True,
    timeout_ms: int = 5000,
    poll_interval_ms: int = 100,
) -> dict[str, Any]:
    if object_name is None and class_name is None:
        return {
            "success": False,
            "error": "invalid-input",
            "message": "supply at least one of object_name or class_name",
        }
    timeout = max(0, min(int(timeout_ms), 60_000)) / 1000.0
    poll = max(25, int(poll_interval_ms)) / 1000.0

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

    deadline = time.monotonic() + timeout
    polls = 0
    while True:
        polls += 1
        for w in app.allWidgets():
            try:
                if not _match(w, object_name, class_name):
                    continue
                if visible and not w.isVisible():
                    continue
                if enabled and not w.isEnabled():
                    continue
                return {
                    "success": True,
                    "context": {
                        "binding": binding.name,
                        "polls": polls,
                        "elapsed_secs": round(timeout - max(0.0, deadline - time.monotonic()), 3),
                    },
                    "widget": widget_summary(w),
                }
            except Exception:  # noqa: BLE001
                continue
        if time.monotonic() >= deadline:
            return {
                "success": False,
                "error": "timeout",
                "message": "timed out waiting for matching widget",
                "context": {
                    "polls": polls,
                    "criteria": {
                        "object_name": object_name,
                        "class_name": class_name,
                        "visible": visible,
                        "enabled": enabled,
                    },
                },
            }
        time.sleep(poll)
