"""Shared progressive query helpers for mock, CDP, and Windows UIA backends.

Each backend supplies a snapshot dictionary and its own find_by_id callback;
the core observe/expand/inspect logic is backend-agnostic.
"""

from __future__ import annotations

from typing import Any
from typing import Callable
from typing import Dict
from typing import List
from typing import Optional


def _find_by_id(snapshot: Dict[str, Any], control_id: str) -> Optional[Dict[str, Any]]:
    """Walk a UiSnapshot dict breadth-first and return the first node with a matching id."""
    root = snapshot.get("root")
    if not isinstance(root, dict):
        return None
    queue: List[Dict[str, Any]] = [root]
    while queue:
        node = queue.pop(0)
        if node.get("id") == control_id:
            return node
        for child in node.get("children", []) or []:
            if isinstance(child, dict):
                queue.append(child)
    return None


def _strip_children(node: Dict[str, Any]) -> Dict[str, Any]:
    """Return a copy of node with children collapsed to child_count."""
    stripped = dict(node)
    child_count = len(stripped.get("children", []) or [])
    stripped["children"] = []
    stripped["child_count"] = child_count
    return stripped


def observe_from_snapshot(
    snapshot: Dict[str, Any],
    max_roots: int,
) -> Dict[str, Any]:
    """Return root-level controls without expanding children.

    Returns (roots, total_roots, truncated).
    """
    all_roots = list(snapshot.get("root", {}).get("children", []) or [])
    total_roots = len(all_roots)
    truncated = total_roots > max_roots

    roots = [_strip_children(child) for child in all_roots[:max_roots]]
    return {"roots": roots, "total_roots": total_roots, "truncated": truncated}


def expand_from_snapshot(
    snapshot: Dict[str, Any],
    control_id: str,
    max_children: int,
) -> Optional[Dict[str, Any]]:
    """Return direct children of a specific control node.

    Returns None when control_id is not found.
    """
    parent = _find_by_id(snapshot, control_id)
    if not parent:
        return None

    all_children = list(parent.get("children", []) or [])
    total_children = len(all_children)
    truncated = total_children > max_children

    children = [_strip_children(child) for child in all_children[:max_children]]
    return {
        "control_id": control_id,
        "children": children,
        "total_children": total_children,
        "truncated": truncated,
    }


def _compute_tree_path(snapshot_root: Dict[str, Any], target_id: str) -> str:
    """Compute a dot-separated index path from root to target_id."""
    queue: List[tuple] = [(snapshot_root, "")]
    while queue:
        node, prefix = queue.pop(0)
        children = node.get("children", []) or []
        for idx, child in enumerate(children):
            if not isinstance(child, dict):
                continue
            path = f"{prefix}{idx}" if prefix else str(idx)
            if child.get("id") == target_id:
                return path
            queue.append((child, path + "."))
    return ""


def _classify_role(role: str) -> Dict[str, Any]:
    """Return patterns, actions, and keyboard focusability for a role."""
    role = str(role).lower()
    if role == "text_field":
        return {
            "patterns": ["ValuePattern", "TextPattern"],
            "actions": ["set_text", "focus"],
            "is_keyboard_focusable": True,
        }
    if role == "checkbox":
        return {
            "patterns": ["TogglePattern"],
            "actions": ["toggle", "set_checked", "click"],
            "is_keyboard_focusable": True,
        }
    if role == "button":
        return {
            "patterns": ["InvokePattern"],
            "actions": ["click"],
            "is_keyboard_focusable": True,
        }
    if role in ("link", "menu_item", "combo_box", "list_item"):
        return {
            "patterns": ["SelectionItemPattern"],
            "actions": ["click", "focus"],
            "is_keyboard_focusable": True,
        }
    if role == "password":
        return {
            "patterns": ["ValuePattern"],
            "actions": ["set_text", "focus"],
            "is_keyboard_focusable": True,
            "is_password": True,
        }
    return {
        "patterns": [],
        "actions": ["click", "focus"],
        "is_keyboard_focusable": False,
    }


def build_control_detail(
    node: Dict[str, Any],
    snapshot: Dict[str, Any],
    backend_name: str,
    override_fields: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """Build a UiControlDetail dict from a snapshot node.

    Args:
        node: The raw control node from the snapshot.
        snapshot: The full snapshot dict (for focus_id lookup).
        backend_name: Label for the metadata.backend field.
        override_fields: Optional dict of field name mappings for backend-specific
            keys (e.g. windows-uia uses 'name' instead of 'label').
    """
    override = override_fields or {}

    role = str(
        node.get(override.get("role_key", "role"))
        or node.get("control_type")
        or "unknown"
    )
    classified = _classify_role(role)

    detail: Dict[str, Any] = {
        "id": node.get(override.get("id_key", "id"), node.get("id", "")),
        "role": role,
        "enabled": bool(node.get(override.get("enabled_key", "enabled"), True)),
        "visible": bool(node.get(override.get("visible_key", "visible"), True)),
        "focused": node.get(override.get("focused_key", "focused"))
        or node.get("has_focus")
        or snapshot.get("focus_id") == node.get("id")
        or False,
        "label": node.get(override.get("label_key", "label"))
        or node.get("name"),
        "text": node.get(override.get("text_key", "text")),
        "object_name": node.get(override.get("object_name_key", "object_name"))
        or node.get("automation_id"),
        "tooltip": node.get(override.get("tooltip_key", "tooltip"))
        or node.get("help_text"),
        "bounds": node.get(override.get("bounds_key", "bounds")),
        "value": node.get(override.get("value_key", "value")),
        "checked": node.get(override.get("checked_key", "checked")),
        "child_count": len(node.get("children", []) or []),
        "supported_patterns": classified.get("patterns", []),
        "supported_actions": classified.get("actions", []),
        "is_keyboard_focusable": classified.get("is_keyboard_focusable", False),
        "is_password": node.get("is_password") or role == "password" or False,
        "tree_path": _compute_tree_path(snapshot.get("root", {}), node.get("id", "")),
        "metadata": {"backend": backend_name},
    }

    return detail
