"""Recipes system for dcc-mcp-core skills (issue #428).

Formalizes the ``metadata.dcc-mcp.recipes`` sibling-file key per the #356
sibling-file pattern. A skill recipe file is a flat Markdown file with
anchored ``##`` sections, each containing a short, copy-pasteable Python
snippet.

This module provides:

- :func:`get_recipes_path` — extract the recipes file path from a skill's metadata
- :func:`parse_recipe_anchors` — list anchor names from a RECIPES.md file
- :func:`get_recipe_content` — fetch content of a specific anchor section
- :func:`register_recipes_tools` — register ``recipes__list`` and ``recipes__get``
  as MCP tools on a server

Example SKILL.md metadata::

    metadata:
      dcc-mcp:
        layer: thin-harness
        tools: tools.yaml
        recipes: references/RECIPES.md

Example RECIPES.md file::

    ## create_polygon_cube

    Create a named polygon cube at the origin.

    ```python
    cube = cmds.polyCube(name="myCube", w=1, h=1, d=1)[0]
    ```

    ## set_world_translation

    Set absolute world-space translation.

    ```python
    cmds.xform("myCube", translation=(1, 2, 3), worldSpace=True)
    ```

Usage::

    from dcc_mcp_core.recipes import (
        get_recipes_path,
        parse_recipe_anchors,
        get_recipe_content,
        register_recipes_tools,
    )

    # Get recipes path from SkillMetadata
    recipes_path = get_recipes_path(metadata)

    # List all anchors
    anchors = parse_recipe_anchors(recipes_path)
    # ["create_polygon_cube", "set_world_translation"]

    # Get content for an anchor
    content = get_recipe_content(recipes_path, "create_polygon_cube")

"""

from __future__ import annotations

import logging
from pathlib import Path
import re
from typing import Any

from dcc_mcp_core import json_loads
from dcc_mcp_core._tool_registration import ToolSpec
from dcc_mcp_core._tool_registration import register_tools

logger = logging.getLogger(__name__)

_ANCHOR_PATTERN = re.compile(r"^##\s+(\S.+)$", re.MULTILINE)


# ── Core parsing utilities ─────────────────────────────────────────────────


def get_recipes_path(metadata: Any) -> str | None:
    """Extract the recipes file path from a ``SkillMetadata`` object.

    Reads ``metadata.dcc-mcp.recipes`` from the skill's metadata dict.
    Supports both flat (``"dcc-mcp.recipes": "recipes.md"``) and nested
    (``"dcc-mcp": {"recipes": "recipes.md"}``) forms.

    The returned path is absolute (resolved relative to the skill's
    ``skill_path`` when that attribute is available and the path is relative).

    Parameters
    ----------
    metadata:
        A ``SkillMetadata`` instance or any object with a ``metadata`` dict
        and optionally a ``skill_path`` str attribute.

    Returns
    -------
    str | None
        Absolute or relative path string to the recipes file, or ``None``
        if the skill does not declare a recipes file.

    """
    meta_dict: dict[str, Any] = getattr(metadata, "metadata", {}) or {}

    # Flat form: "dcc-mcp.recipes": "path"
    recipes_rel = meta_dict.get("dcc-mcp.recipes")

    # Nested form: {"dcc-mcp": {"recipes": "path"}}
    if recipes_rel is None:
        dcc_mcp_nested = meta_dict.get("dcc-mcp")
        if isinstance(dcc_mcp_nested, dict):
            recipes_rel = dcc_mcp_nested.get("recipes")

    if not recipes_rel:
        return None

    # Resolve relative to skill_path if possible
    skill_path = getattr(metadata, "skill_path", None)
    if skill_path and not Path(recipes_rel).is_absolute():
        return str(Path(skill_path) / recipes_rel)
    return str(recipes_rel)


def parse_recipe_anchors(recipes_path: str) -> list[str]:
    """Return the list of anchor names from a RECIPES.md file.

    Anchors are ``##`` headings. The anchor name is the heading text
    stripped of leading whitespace and rendered as-is (no slug conversion).

    Parameters
    ----------
    recipes_path:
        Absolute path to the RECIPES.md file.

    Returns
    -------
    list[str]
        Ordered list of anchor names, or ``[]`` if the file does not exist
        or contains no ``##`` headings.

    """
    path = Path(recipes_path)
    if not path.is_file():
        logger.debug("parse_recipe_anchors: file not found: %s", path)
        return []
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        logger.warning("parse_recipe_anchors: could not read %s: %s", path, exc)
        return []
    return [m.group(1).strip() for m in _ANCHOR_PATTERN.finditer(text)]


def get_recipe_content(recipes_path: str, anchor: str) -> str | None:
    """Return the Markdown content of a specific anchor section.

    Returns all text from the ``## <anchor>`` heading up to (but not
    including) the next ``##`` heading, or to end-of-file.

    Parameters
    ----------
    recipes_path:
        Absolute path to the RECIPES.md file.
    anchor:
        The anchor name (as returned by :func:`parse_recipe_anchors`).

    Returns
    -------
    str | None
        Markdown content of the section (including the heading line),
        or ``None`` if the anchor is not found.

    """
    path = Path(recipes_path)
    if not path.is_file():
        return None
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return None

    lines = text.splitlines(keepends=True)
    start_idx: int | None = None

    for i, line in enumerate(lines):
        if line.rstrip().startswith("## ") and line.strip()[3:].strip() == anchor:
            start_idx = i
            break

    if start_idx is None:
        return None

    end_idx = len(lines)
    for i in range(start_idx + 1, len(lines)):
        if lines[i].startswith("## "):
            end_idx = i
            break

    return "".join(lines[start_idx:end_idx]).rstrip()


# ── MCP tool registration ─────────────────────────────────────────────────

_LIST_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "skill": {
            "type": "string",
            "description": "Skill name to list recipes for.",
        },
    },
    "required": ["skill"],
    "additionalProperties": False,
}

_GET_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "skill": {
            "type": "string",
            "description": "Skill name.",
        },
        "anchor": {
            "type": "string",
            "description": "Recipe anchor name (from recipes__list).",
        },
    },
    "required": ["skill", "anchor"],
    "additionalProperties": False,
}

_RECIPES_LIST_DESCRIPTION = (
    "List available recipe anchors for a skill's RECIPES.md file. "
    "When to use: before calling execute_python — check if a recipe covers "
    "the operation. "
    "How to use: pass skill name, then call recipes__get for the matching anchor."
)

_RECIPES_GET_DESCRIPTION = (
    "Fetch the Markdown content of a specific recipe anchor from a skill's "
    "RECIPES.md file. "
    "When to use: when recipes__list returns an anchor matching your intent. "
    "How to use: pass skill name and anchor name; copy the code snippet."
)


def register_recipes_tools(
    server: Any,
    *,
    skills: list[Any],
    dcc_name: str = "dcc",
) -> None:
    """Register ``recipes__list`` and ``recipes__get`` MCP tools on *server*.

    The tools operate on the loaded skill list — they look up the skill by
    name, find the recipes file via :func:`get_recipes_path`, and serve the
    content.

    Parameters
    ----------
    server:
        An ``McpHttpServer`` compatible object with ``server.registry``
        and ``server.register_handler(name, handler)``.
    skills:
        List of ``SkillMetadata`` objects (e.g. from ``scan_and_load()``).
    dcc_name:
        DCC name string for tool metadata.

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core import create_skill_server, McpHttpConfig, scan_and_load
        from dcc_mcp_core.recipes import register_recipes_tools

        loaded, _ = scan_and_load(dcc_name="maya")
        server = create_skill_server("maya", McpHttpConfig(port=8765))
        register_recipes_tools(server, skills=loaded, dcc_name="maya")
        handle = server.start()

    """
    skill_map: dict[str, Any] = {getattr(s, "name", ""): s for s in skills}

    def _handle_list(params: Any) -> Any:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
        skill_name = args.get("skill", "")
        skill_md = skill_map.get(skill_name)
        if skill_md is None:
            return {
                "success": False,
                "message": f"Skill '{skill_name}' not found.",
                "context": {"skill": skill_name, "anchors": []},
            }
        rp = get_recipes_path(skill_md)
        if not rp:
            return {
                "success": True,
                "message": f"Skill '{skill_name}' has no recipes file.",
                "context": {"skill": skill_name, "anchors": [], "path": None},
            }
        anchors = parse_recipe_anchors(rp)
        return {
            "success": True,
            "message": f"Found {len(anchors)} recipes.",
            "context": {"skill": skill_name, "anchors": anchors, "path": rp},
        }

    def _handle_get(params: Any) -> Any:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
        skill_name = args.get("skill", "")
        anchor = args.get("anchor", "")
        skill_md = skill_map.get(skill_name)
        if skill_md is None:
            return {"success": False, "message": f"Skill '{skill_name}' not found."}
        rp = get_recipes_path(skill_md)
        if not rp:
            return {"success": False, "message": f"Skill '{skill_name}' has no recipes file."}
        content = get_recipe_content(rp, anchor)
        if content is None:
            return {
                "success": False,
                "message": f"Anchor '{anchor}' not found in {rp}.",
                "context": {"available_anchors": parse_recipe_anchors(rp)},
            }
        return {
            "success": True,
            "message": f"Recipe '{anchor}'",
            "context": {"skill": skill_name, "anchor": anchor, "content": content},
        }

    specs = [
        ToolSpec(
            name="recipes__list",
            description=_RECIPES_LIST_DESCRIPTION,
            input_schema=_LIST_SCHEMA,
            handler=_handle_list,
            category="recipes",
        ),
        ToolSpec(
            name="recipes__get",
            description=_RECIPES_GET_DESCRIPTION,
            input_schema=_GET_SCHEMA,
            handler=_handle_get,
            category="recipes",
        ),
    ]
    register_tools(
        server,
        specs,
        dcc_name=dcc_name,
        log_prefix="register_recipes_tools",
        logger=logger,
    )


# ── Public API ─────────────────────────────────────────────────────────────

__all__ = [
    "get_recipe_content",
    "get_recipes_path",
    "parse_recipe_anchors",
    "register_recipes_tools",
]
