"""qt_ui_inspector__find_widgets tool (issue #1332)."""

from __future__ import annotations

import re
from typing import Any

from _qt import (  # type: ignore[import-not-found]
    BindingUnavailable,
    binding_unavailable_error,
    load_qt_widgets,
    widget_summary,
)


def _name_matches(name: str, target: str, mode: str) -> bool:
    if mode == "exact":
        return name == target
    if mode == "substring":
        return target in name
    if mode == "regex":
        try:
            return re.search(target, name) is not None
        except re.error:
            return False
    return False


def run(
    *,
    object_name: str | None = None,
    object_name_match: str = "exact",
    class_name: str | None = None,
    visible_only: bool = True,
    max_results: int = 64,
) -> dict[str, Any]:
    cap = max(1, min(int(max_results), 1024))
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

    if object_name is None and class_name is None:
        return {
            "success": False,
            "error": "invalid-input",
            "message": "supply at least one of object_name or class_name",
        }

    hits: list[dict[str, Any]] = []
    for w in app.allWidgets():
        try:
            if visible_only and not w.isVisible():
                continue
            if object_name is not None:
                if not _name_matches(w.objectName() or "", object_name, object_name_match):
                    continue
            if class_name is not None and class_name not in w.__class__.__name__:
                continue
            hits.append(widget_summary(w))
            if len(hits) >= cap:
                break
        except Exception:  # noqa: BLE001
            continue

    return {
        "success": True,
        "context": {
            "binding": binding.name,
            "match_count": len(hits),
            "truncated": len(hits) >= cap,
            "criteria": {
                "object_name": object_name,
                "object_name_match": object_name_match,
                "class_name": class_name,
                "visible_only": visible_only,
            },
        },
        "widgets": hits,
    }
