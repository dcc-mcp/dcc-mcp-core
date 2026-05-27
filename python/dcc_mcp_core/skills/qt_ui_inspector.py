"""Qt UI inspector as a default dcc-mcp-core capability (issue #1332).

Adapters get all five inspector tools with one line::

    from dcc_mcp_core import register_qt_ui_inspector
    register_qt_ui_inspector(server, dcc_name="maya")

The inspector is **DCC-agnostic** and **read-only** — it imports the Qt
binding lazily so this module never pulls Qt into the host on import.
Supported bindings, in priority order: ``qtpy``, ``PySide6``,
``PySide2``, ``PyQt6``, ``PyQt5``. When no binding is importable, every
tool returns a structured ``qt-binding-unavailable`` envelope rather than
crashing the host.
"""

from __future__ import annotations

from dataclasses import dataclass
import logging
import re
import time
from typing import Any

from dcc_mcp_core import json_loads
from dcc_mcp_core._tool_registration import ToolSpec, register_tools
from dcc_mcp_core.result_envelope import ToolResult

logger = logging.getLogger(__name__)

_PROPERTY_CAP = 32
_SUPPORTED_BINDINGS = ("qtpy", "PySide6", "PySide2", "PyQt6", "PyQt5")
_CATEGORY_QT_UI_INSPECTOR = "qt-ui-inspector"


# ── Lazy Qt-binding discovery ─────────────────────────────────────────


@dataclass
class _QtBinding:
    name: str
    widgets: Any
    core: Any


class _BindingUnavailable(RuntimeError):
    pass


def _load_qt() -> _QtBinding:
    errors: list[str] = []
    for name in _SUPPORTED_BINDINGS:
        try:
            __import__(name)
            qt_widgets = __import__(f"{name}.QtWidgets", fromlist=["QtWidgets"])
            qt_core = __import__(f"{name}.QtCore", fromlist=["QtCore"])
        except Exception as exc:  # noqa: BLE001
            errors.append(f"{name}: {exc.__class__.__name__}")
            continue
        return _QtBinding(name=name, widgets=qt_widgets, core=qt_core)
    raise _BindingUnavailable(
        "No Qt binding importable. Tried: " + ", ".join(_SUPPORTED_BINDINGS)
    )


def _binding_unavailable(exc: _BindingUnavailable) -> dict[str, Any]:
    return ToolResult.fail(
        str(exc),
        error="qt-binding-unavailable",
        hint=(
            "install one of qtpy, PySide6, PySide2, PyQt6, PyQt5 in the host "
            "Python environment, then reload the skill"
        ),
    ).to_dict()


def _no_application() -> dict[str, Any]:
    return ToolResult.fail(
        "QApplication.instance() returned None — host has no running Qt event loop.",
        error="qt-no-application",
        hint="this tool must run inside a Qt host (Maya/Houdini/Nuke/Substance/Katana/etc.)",
    ).to_dict()


# ── Widget identifier ─────────────────────────────────────────────────


def _widget_id(widget: Any) -> str:
    """Stable identifier ``class:object_name:fingerprint``."""
    try:
        obj_name = widget.objectName() or ""
    except Exception:  # noqa: BLE001
        obj_name = ""
    klass = widget.__class__.__name__
    fingerprint = id(widget) & 0xFFFFFFFF
    return f"{klass}:{obj_name}:{fingerprint:08x}"


def _safe_call(fn: Any, default: Any) -> Any:
    if fn is None or not callable(fn):
        return default
    try:
        return fn()
    except Exception:  # noqa: BLE001
        return default


def _widget_summary(widget: Any) -> dict[str, Any]:
    try:
        rect = widget.geometry()
        geometry: dict[str, Any] | None = {
            "x": int(rect.x()),
            "y": int(rect.y()),
            "width": int(rect.width()),
            "height": int(rect.height()),
        }
    except Exception:  # noqa: BLE001
        geometry = None
    try:
        children_count = sum(1 for _ in widget.children())
    except Exception:  # noqa: BLE001
        children_count = 0
    out: dict[str, Any] = {
        "widget_id": _widget_id(widget),
        "class": widget.__class__.__name__,
        "object_name": _safe_call(widget.objectName, ""),
        "visible": _safe_call(widget.isVisible, False),
        "enabled": _safe_call(widget.isEnabled, False),
        "children_count": children_count,
        "accessible_name": _safe_call(getattr(widget, "accessibleName", None), ""),
        "accessible_description": _safe_call(
            getattr(widget, "accessibleDescription", None), ""
        ),
    }
    if geometry is not None:
        out["geometry"] = geometry
    return out


def _find_by_id(app: Any, target_id: str) -> Any | None:
    for widget in app.allWidgets():
        try:
            if _widget_id(widget) == target_id:
                return widget
        except Exception:  # noqa: BLE001
            continue
    return None



# ── Public tool functions ──────────────────────────────────────────────


def qt_list_windows(*, include_hidden: bool = False, max_results: int = 64) -> dict[str, Any]:
    """List top-level Qt windows. See ``register_qt_ui_inspector`` docstring."""
    cap = max(1, min(int(max_results), 256))
    try:
        binding = _load_qt()
    except _BindingUnavailable as exc:
        return _binding_unavailable(exc)
    app = binding.widgets.QApplication.instance()
    if app is None:
        return _no_application()
    windows: list[dict[str, Any]] = []
    for w in app.topLevelWidgets():
        try:
            if not include_hidden and not w.isVisible():
                continue
            windows.append(_widget_summary(w))
            if len(windows) >= cap:
                break
        except Exception as exc:  # noqa: BLE001
            windows.append({"widget_id": None, "class": w.__class__.__name__, "error": str(exc)})
    return ToolResult.ok(
        "listed Qt top-level windows",
        binding=binding.name,
        window_count=len(windows),
        include_hidden=include_hidden,
        truncated=len(windows) >= cap,
        windows=windows,
    ).to_dict()


def qt_find_widgets(
    *,
    object_name: str | None = None,
    object_name_match: str = "exact",
    class_name: str | None = None,
    visible_only: bool = True,
    max_results: int = 64,
) -> dict[str, Any]:
    if object_name is None and class_name is None:
        return ToolResult.invalid_input(
            "supply at least one of object_name or class_name"
        ).to_dict()
    cap = max(1, min(int(max_results), 1024))
    try:
        binding = _load_qt()
    except _BindingUnavailable as exc:
        return _binding_unavailable(exc)
    app = binding.widgets.QApplication.instance()
    if app is None:
        return _no_application()
    hits: list[dict[str, Any]] = []
    for w in app.allWidgets():
        try:
            if visible_only and not w.isVisible():
                continue
            if object_name is not None and not _name_matches(
                w.objectName() or "", object_name, object_name_match
            ):
                continue
            if class_name is not None and class_name not in w.__class__.__name__:
                continue
            hits.append(_widget_summary(w))
            if len(hits) >= cap:
                break
        except Exception:  # noqa: BLE001
            continue
    return ToolResult.ok(
        "found matching Qt widgets",
        binding=binding.name,
        match_count=len(hits),
        truncated=len(hits) >= cap,
        widgets=hits,
    ).to_dict()


def qt_describe_widget(*, widget_id: str) -> dict[str, Any]:  # noqa: A002
    if not widget_id:
        return ToolResult.invalid_input("widget_id is required").to_dict()
    try:
        binding = _load_qt()
    except _BindingUnavailable as exc:
        return _binding_unavailable(exc)
    app = binding.widgets.QApplication.instance()
    if app is None:
        return _no_application()
    widget = _find_by_id(app, widget_id)
    if widget is None:
        return ToolResult.not_found("Widget", widget_id).to_dict()
    summary = _widget_summary(widget)
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
    return ToolResult.ok(
        "described Qt widget",
        binding=binding.name,
        widget=summary,
    ).to_dict()


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


def _coerce_property(value: Any) -> Any:
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, (list, tuple)):
        return [_coerce_property(v) for v in list(value)[:32]]
    return str(value)


def qt_snapshot_tree(
    *,
    root_widget_id: str | None = None,
    max_depth: int = 4,
    max_nodes: int = 256,
) -> dict[str, Any]:
    depth = max(0, min(int(max_depth), 16))
    cap = max(1, min(int(max_nodes), 4096))
    try:
        binding = _load_qt()
    except _BindingUnavailable as exc:
        return _binding_unavailable(exc)
    app = binding.widgets.QApplication.instance()
    if app is None:
        return _no_application()
    budget = [cap]
    if root_widget_id is None:
        trees = [_walk(w, 0, depth, budget) for w in app.topLevelWidgets() if budget[0] > 0]
        return ToolResult.ok(
            "snapshotted Qt top-level widget trees",
            binding=binding.name,
            root_widget_id=None,
            max_depth=depth,
            nodes_remaining=budget[0],
            trees=trees,
        ).to_dict()
    root = _find_by_id(app, root_widget_id)
    if root is None:
        return ToolResult.not_found("Widget", root_widget_id).to_dict()
    tree = _walk(root, 0, depth, budget)
    return ToolResult.ok(
        "snapshotted Qt widget tree",
        binding=binding.name,
        root_widget_id=root_widget_id,
        max_depth=depth,
        nodes_remaining=budget[0],
        tree=tree,
    ).to_dict()


def _walk(widget: Any, depth: int, max_depth: int, budget: list[int]) -> dict[str, Any]:
    if budget[0] <= 0:
        return {
            "widget_id": _widget_id(widget),
            "class": widget.__class__.__name__,
            "truncated": True,
        }
    budget[0] -= 1
    node = _widget_summary(widget)
    if depth >= max_depth:
        try:
            has_children = any(True for _ in widget.children())
        except Exception:  # noqa: BLE001
            has_children = False
        node["children"] = []
        node["truncated"] = has_children
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


def qt_wait_for_widget(
    *,
    object_name: str | None = None,
    class_name: str | None = None,
    visible: bool = True,
    enabled: bool = True,
    timeout_ms: int = 5000,
    poll_interval_ms: int = 100,
) -> dict[str, Any]:
    if object_name is None and class_name is None:
        return ToolResult.invalid_input(
            "supply at least one of object_name or class_name"
        ).to_dict()
    timeout = max(0, min(int(timeout_ms), 60_000)) / 1000.0
    poll = max(25, int(poll_interval_ms)) / 1000.0
    try:
        binding = _load_qt()
    except _BindingUnavailable as exc:
        return _binding_unavailable(exc)
    app = binding.widgets.QApplication.instance()
    if app is None:
        return _no_application()
    deadline = time.monotonic() + timeout
    polls = 0
    while True:
        polls += 1
        for w in app.allWidgets():
            try:
                if object_name is not None and (w.objectName() or "") != object_name:
                    continue
                if class_name is not None and class_name not in w.__class__.__name__:
                    continue
                if visible and not w.isVisible():
                    continue
                if enabled and not w.isEnabled():
                    continue
                return ToolResult.ok(
                    "Qt widget appeared",
                    binding=binding.name,
                    polls=polls,
                    elapsed_secs=round(timeout - max(0.0, deadline - time.monotonic()), 3),
                    widget=_widget_summary(w),
                ).to_dict()
            except Exception:  # noqa: BLE001
                continue
        if time.monotonic() >= deadline:
            return ToolResult.fail(
                "timed out waiting for matching widget",
                error="timeout",
                polls=polls,
                object_name=object_name,
                class_name=class_name,
                visible=visible,
                enabled=enabled,
            ).to_dict()
        time.sleep(poll)



# ── MCP registration ───────────────────────────────────────────────────


_LIST_WINDOWS_SCHEMA = {
    "type": "object",
    "properties": {
        "include_hidden": {"type": "boolean", "default": False},
        "max_results": {"type": "integer", "default": 64, "minimum": 1, "maximum": 256},
    },
}

_FIND_WIDGETS_SCHEMA = {
    "type": "object",
    "properties": {
        "object_name": {"type": "string"},
        "object_name_match": {
            "type": "string",
            "enum": ["exact", "substring", "regex"],
            "default": "exact",
        },
        "class_name": {"type": "string"},
        "visible_only": {"type": "boolean", "default": True},
        "max_results": {"type": "integer", "default": 64, "minimum": 1, "maximum": 1024},
    },
}

_DESCRIBE_WIDGET_SCHEMA = {
    "type": "object",
    "properties": {"widget_id": {"type": "string"}},
    "required": ["widget_id"],
}

_SNAPSHOT_TREE_SCHEMA = {
    "type": "object",
    "properties": {
        "root_widget_id": {"type": "string"},
        "max_depth": {"type": "integer", "default": 4, "minimum": 0, "maximum": 16},
        "max_nodes": {"type": "integer", "default": 256, "minimum": 1, "maximum": 4096},
    },
}

_WAIT_FOR_WIDGET_SCHEMA = {
    "type": "object",
    "properties": {
        "object_name": {"type": "string"},
        "class_name": {"type": "string"},
        "visible": {"type": "boolean", "default": True},
        "enabled": {"type": "boolean", "default": True},
        "timeout_ms": {"type": "integer", "default": 5000, "minimum": 0, "maximum": 60000},
        "poll_interval_ms": {"type": "integer", "default": 100, "minimum": 25},
    },
}


def register_qt_ui_inspector(server: Any, *, dcc_name: str = "dcc") -> None:
    """Register the five ``qt_ui_inspector__*`` tools on *server*.

    All tools are read-only and lazy-import the Qt binding. Adapters
    should call this **before** ``server.start()``.

    Example::

        from dcc_mcp_core import McpHttpServer, McpHttpConfig
        from dcc_mcp_core import register_qt_ui_inspector

        server = McpHttpServer(registry, McpHttpConfig(port=8765))
        register_qt_ui_inspector(server, dcc_name="maya")
        handle = server.start()
    """

    def _handler(fn):
        def wrapper(params: Any) -> Any:
            args: dict[str, Any] = (
                json_loads(params) if isinstance(params, str) else (params or {})
            )
            return fn(**args)

        return wrapper

    specs = [
        ToolSpec(
            name="qt_ui_inspector__list_windows",
            description="List every top-level Qt window with object name, class, visibility, geometry, and child count.",
            input_schema=_LIST_WINDOWS_SCHEMA,
            handler=_handler(qt_list_windows),
            category=_CATEGORY_QT_UI_INSPECTOR,
        ),
        ToolSpec(
            name="qt_ui_inspector__find_widgets",
            description="Locate Qt widgets by object name (exact/substring/regex), class name, and visibility. Bounded result count.",
            input_schema=_FIND_WIDGETS_SCHEMA,
            handler=_handler(qt_find_widgets),
            category=_CATEGORY_QT_UI_INSPECTOR,
        ),
        ToolSpec(
            name="qt_ui_inspector__describe_widget",
            description="Return a single Qt widget's structured state - class, geometry, flags, accessible name/description, bounded property snapshot.",
            input_schema=_DESCRIBE_WIDGET_SCHEMA,
            handler=_handler(qt_describe_widget),
            category=_CATEGORY_QT_UI_INSPECTOR,
        ),
        ToolSpec(
            name="qt_ui_inspector__snapshot_tree",
            description="Walk the Qt widget tree under a root and return it as a JSON-safe tree with depth and node-count budgets.",
            input_schema=_SNAPSHOT_TREE_SCHEMA,
            handler=_handler(qt_snapshot_tree),
            category=_CATEGORY_QT_UI_INSPECTOR,
        ),
        ToolSpec(
            name="qt_ui_inspector__wait_for_widget",
            description="Poll for a Qt widget by name/class with visible/enabled gates and bounded timeout.",
            input_schema=_WAIT_FOR_WIDGET_SCHEMA,
            handler=_handler(qt_wait_for_widget),
            category=_CATEGORY_QT_UI_INSPECTOR,
        ),
    ]
    register_tools(
        server,
        specs,
        dcc_name=dcc_name,
        log_prefix="register_qt_ui_inspector",
        logger=logger,
    )


__all__ = [
    "qt_describe_widget",
    "qt_find_widgets",
    "qt_list_windows",
    "qt_snapshot_tree",
    "qt_wait_for_widget",
    "register_qt_ui_inspector",
]
