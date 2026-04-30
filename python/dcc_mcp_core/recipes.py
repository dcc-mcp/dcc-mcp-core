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

from dataclasses import dataclass
import json
import logging
from pathlib import Path
import re
from typing import Any

from dcc_mcp_core import json_loads
from dcc_mcp_core import yaml_loads
from dcc_mcp_core._tool_registration import ToolSpec
from dcc_mcp_core._tool_registration import register_tools
from dcc_mcp_core.constants import CATEGORY_RECIPES
from dcc_mcp_core.constants import METADATA_DCC_MCP
from dcc_mcp_core.constants import METADATA_RECIPES_KEY
from dcc_mcp_core.result_envelope import ToolResult

logger = logging.getLogger(__name__)

_ANCHOR_PATTERN = re.compile(r"^##\s+(\S.+)$", re.MULTILINE)


@dataclass(frozen=True)
class RecipeDefinition:
    """Structured domain recipe loaded from a YAML recipe pack."""

    name: str
    dcc: str = ""
    description: str = ""
    inputs_schema: dict[str, Any] | None = None
    steps: list[Any] | None = None
    output_contract: str | dict[str, Any] | None = None
    toolset_profiles: list[str] | None = None
    provenance: dict[str, Any] | None = None

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-serialisable recipe payload."""
        return {
            "name": self.name,
            "dcc": self.dcc,
            "description": self.description,
            "inputs_schema": self.inputs_schema or {},
            "steps": self.steps or [],
            "output_contract": self.output_contract,
            "toolset_profiles": self.toolset_profiles or [],
            "provenance": self.provenance or {},
        }


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
    paths = get_recipes_paths(metadata)
    return paths[0] if paths else None


def get_recipes_paths(metadata: Any) -> list[str]:
    """Extract all recipe sibling file paths from a ``SkillMetadata`` object.

    ``metadata.dcc-mcp.recipes`` may be a filename, glob string, or list of
    filenames/globs. Relative paths are resolved under ``skill_path`` when
    present. Missing globs are preserved as the raw path so callers can report
    useful diagnostics instead of silently hiding configuration mistakes.
    """
    meta_dict: dict[str, Any] = getattr(metadata, "metadata", {}) or {}

    # Flat form: "dcc-mcp.recipes": "path"
    recipes_rel = meta_dict.get(METADATA_RECIPES_KEY)

    # Nested form: {"dcc-mcp": {"recipes": "path"}}
    if recipes_rel is None:
        dcc_mcp_nested = meta_dict.get(METADATA_DCC_MCP)
        if isinstance(dcc_mcp_nested, dict):
            recipes_rel = dcc_mcp_nested.get("recipes")

    if not recipes_rel:
        return []

    raw_values = recipes_rel if isinstance(recipes_rel, list) else [recipes_rel]
    skill_path = getattr(metadata, "skill_path", None)
    base = Path(skill_path) if skill_path else None
    paths: list[str] = []
    for raw in raw_values:
        if not raw:
            continue
        raw_path = Path(str(raw))
        candidate = raw_path if raw_path.is_absolute() or base is None else base / raw_path

        if any(ch in str(raw_path) for ch in "*?[]"):
            matches = _expand_recipe_glob(raw_path, base)
            paths.extend(str(p) for p in matches)
            if not matches:
                paths.append(str(candidate))
        else:
            paths.append(str(raw) if base is None and not raw_path.is_absolute() else str(candidate))

    return paths


def _expand_recipe_glob(raw_path: Path, base: Path | None) -> list[Path]:
    """Expand a recipe glob with pathlib so linted code stays path-safe."""
    if raw_path.is_absolute():
        root = Path(raw_path.anchor)
        pattern = str(raw_path.relative_to(root))
        return sorted(root.glob(pattern))
    return sorted((base or Path.cwd()).glob(str(raw_path)))


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


def load_recipe_pack(recipes_path: str, *, skill_name: str = "") -> list[RecipeDefinition]:
    """Load structured recipe definitions from a YAML recipe pack.

    Recipe packs use the convention proposed in issue #616::

        recipes:
          - name: build_pbr_material
            dcc: substance-designer
            inputs_schema: {...}
            steps: [...]
            output_contract: material_graph

    Non-YAML files and malformed packs return ``[]`` so legacy
    ``RECIPES.md`` anchors continue to be handled by the Markdown helpers.
    """
    path = Path(recipes_path)
    if path.suffix.lower() not in {".yaml", ".yml"} or not path.is_file():
        return []
    try:
        raw = yaml_loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        logger.warning("load_recipe_pack: could not parse %s: %s", path, exc)
        return []
    recipes = raw.get("recipes") if isinstance(raw, dict) else None
    if not isinstance(recipes, list):
        return []

    loaded: list[RecipeDefinition] = []
    for item in recipes:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name") or "").strip()
        if not name:
            continue
        profiles = item.get("toolset_profiles") or item.get("toolset_profile") or []
        if isinstance(profiles, str):
            profiles = [profiles]
        loaded.append(
            RecipeDefinition(
                name=name,
                dcc=str(item.get("dcc") or ""),
                description=str(item.get("description") or ""),
                inputs_schema=item.get("inputs_schema") if isinstance(item.get("inputs_schema"), dict) else {},
                steps=item.get("steps") if isinstance(item.get("steps"), list) else [],
                output_contract=item.get("output_contract"),
                toolset_profiles=[str(p) for p in profiles] if isinstance(profiles, list) else [],
                provenance={
                    "skill": skill_name,
                    "path": str(path),
                    "format": "recipe-pack",
                },
            ),
        )
    return loaded


def list_recipe_entries(skill_md: Any) -> list[dict[str, Any]]:
    """Return Markdown anchors and structured recipe pack entries for a skill."""
    entries: list[dict[str, Any]] = []
    skill_name = str(getattr(skill_md, "name", "") or "")
    for rp in get_recipes_paths(skill_md):
        pack = load_recipe_pack(rp, skill_name=skill_name)
        if pack:
            entries.extend(recipe.to_dict() for recipe in pack)
            continue
        for anchor in parse_recipe_anchors(rp):
            entries.append(
                {
                    "name": anchor,
                    "description": "",
                    "inputs_schema": {},
                    "steps": [],
                    "output_contract": "markdown_recipe",
                    "toolset_profiles": [],
                    "provenance": {
                        "skill": skill_name,
                        "path": rp,
                        "format": "markdown-anchor",
                    },
                },
            )
    return entries


def find_recipe_entry(skill_md: Any, recipe_name: str) -> dict[str, Any] | None:
    """Find a structured recipe entry or Markdown anchor by name."""
    for entry in list_recipe_entries(skill_md):
        if entry["name"] == recipe_name:
            return entry
    return None


def validate_recipe_inputs(recipe: dict[str, Any], inputs: dict[str, Any]) -> list[str]:
    """Validate inputs against the recipe's JSON-schema-like input shape.

    This intentionally implements a conservative subset (`required`,
    `properties`, and primitive `type`) so recipe packs remain zero-dep and
    useful even before adapters wire richer validation.
    """
    schema = recipe.get("inputs_schema") or {}
    if not isinstance(schema, dict):
        return []
    errors: list[str] = []
    required = schema.get("required") or []
    if isinstance(required, list):
        for name in required:
            if name not in inputs:
                errors.append(f"Missing required input: {name}")

    properties = schema.get("properties") or {}
    if not isinstance(properties, dict):
        return errors
    for name, spec in properties.items():
        if name not in inputs or not isinstance(spec, dict):
            continue
        expected = spec.get("type")
        if expected and not _matches_json_type(inputs[name], expected):
            errors.append(f"Input '{name}' expected {expected}, got {type(inputs[name]).__name__}")
    return errors


def _matches_json_type(value: Any, expected: Any) -> bool:
    expected_types = expected if isinstance(expected, list) else [expected]
    for item in expected_types:
        if item == "string" and isinstance(value, str):
            return True
        if item == "number" and isinstance(value, int | float) and not isinstance(value, bool):
            return True
        if item == "integer" and isinstance(value, int) and not isinstance(value, bool):
            return True
        if item == "boolean" and isinstance(value, bool):
            return True
        if item == "array" and isinstance(value, list):
            return True
        if item == "object" and isinstance(value, dict):
            return True
        if item == "null" and value is None:
            return True
    return False


def json_dumps_pretty(value: Any) -> str:
    """Render recipe data for legacy text/content clients."""
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True)


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

_SEARCH_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "query": {"type": "string", "description": "Case-insensitive recipe search text."},
        "dcc": {"type": "string", "description": "Optional DCC filter."},
        "skill": {"type": "string", "description": "Optional skill filter."},
    },
    "additionalProperties": False,
}

_VALIDATE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "skill": {"type": "string", "description": "Skill name."},
        "recipe": {"type": "string", "description": "Recipe name."},
        "inputs": {"type": "object", "description": "Candidate recipe inputs."},
    },
    "required": ["skill", "recipe", "inputs"],
    "additionalProperties": False,
}

_APPLY_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "skill": {"type": "string", "description": "Skill name."},
        "recipe": {"type": "string", "description": "Recipe name."},
        "inputs": {"type": "object", "description": "Recipe inputs."},
        "target": {"description": "Optional scene/document/graph/timeline target."},
    },
    "required": ["skill", "recipe", "inputs"],
    "additionalProperties": False,
}

_RECIPES_LIST_DESCRIPTION = (
    "List available recipe anchors or structured domain recipes for a skill. "
    "When to use: before calling execute_python — check if a recipe covers "
    "the operation. "
    "How to use: pass skill name, then call recipes__get for the matching recipe."
)

_RECIPES_GET_DESCRIPTION = (
    "Fetch the Markdown content of a specific recipe anchor from a skill's "
    "RECIPES.md file. "
    "When to use: when recipes__list returns an anchor matching your intent. "
    "How to use: pass skill name and anchor name; copy the code snippet."
)

_RECIPES_SEARCH_DESCRIPTION = (
    "Search structured domain recipes across loaded skills. "
    "When to use: when you know the creative intent but not the owning skill. "
    "How to use: pass query text and optional dcc/skill filters."
)

_RECIPES_VALIDATE_DESCRIPTION = (
    "Validate candidate inputs against a structured recipe pack input schema. "
    "When to use: before recipes__apply or before dispatching recipe steps."
)

_RECIPES_APPLY_DESCRIPTION = (
    "Build an application plan for a structured recipe pack entry. "
    "The core returns validated inputs, steps, output contract, and provenance; "
    "adapters or agents dispatch the returned steps through the relevant tools/workflows."
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
            return ToolResult(
                success=False,
                message=f"Skill '{skill_name}' not found.",
                context={"skill": skill_name, "anchors": []},
            ).to_dict()
        rp = get_recipes_path(skill_md)
        if not rp:
            return ToolResult.ok(
                f"Skill '{skill_name}' has no recipes file.",
                skill=skill_name,
                anchors=[],
                recipes=[],
                path=None,
            ).to_dict()
        entries = list_recipe_entries(skill_md)
        anchors = [entry["name"] for entry in entries if entry.get("provenance", {}).get("format") == "markdown-anchor"]
        return ToolResult.ok(
            f"Found {len(entries)} recipes.",
            skill=skill_name,
            anchors=anchors,
            recipes=entries,
            paths=get_recipes_paths(skill_md),
        ).to_dict()

    def _handle_get(params: Any) -> Any:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
        skill_name = args.get("skill", "")
        anchor = args.get("anchor", "")
        skill_md = skill_map.get(skill_name)
        if skill_md is None:
            return ToolResult(success=False, message=f"Skill '{skill_name}' not found.").to_dict()
        rp = get_recipes_path(skill_md)
        if not rp:
            return ToolResult(success=False, message=f"Skill '{skill_name}' has no recipes file.").to_dict()
        recipe = find_recipe_entry(skill_md, anchor)
        if recipe and recipe.get("provenance", {}).get("format") == "recipe-pack":
            return ToolResult.ok(
                f"Recipe '{anchor}'",
                skill=skill_name,
                anchor=anchor,
                recipe=recipe,
                content=json_dumps_pretty(recipe),
            ).to_dict()
        content_path = str(recipe.get("provenance", {}).get("path") or rp) if recipe else rp
        content = get_recipe_content(content_path, anchor)
        if content is None:
            return ToolResult(
                success=False,
                message=f"Anchor '{anchor}' not found in {rp}.",
                context={"available_anchors": [entry["name"] for entry in list_recipe_entries(skill_md)]},
            ).to_dict()
        return ToolResult.ok(
            f"Recipe '{anchor}'",
            skill=skill_name,
            anchor=anchor,
            content=content,
        ).to_dict()

    def _handle_search(params: Any) -> Any:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
        query = str(args.get("query") or "").lower()
        dcc_filter = str(args.get("dcc") or "").lower()
        skill_filter = str(args.get("skill") or "")
        matches: list[dict[str, Any]] = []
        for skill in skills:
            skill_name = str(getattr(skill, "name", "") or "")
            if skill_filter and skill_name != skill_filter:
                continue
            for entry in list_recipe_entries(skill):
                haystack = " ".join(
                    [
                        entry.get("name", ""),
                        entry.get("description", ""),
                        str(entry.get("output_contract") or ""),
                        " ".join(entry.get("toolset_profiles") or []),
                    ],
                ).lower()
                if query and query not in haystack:
                    continue
                if dcc_filter and str(entry.get("dcc") or "").lower() != dcc_filter:
                    continue
                matches.append(entry)
        return ToolResult.ok(f"Found {len(matches)} matching recipes.", query=query, recipes=matches).to_dict()

    def _handle_validate(params: Any) -> Any:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
        skill_name = args.get("skill", "")
        recipe_name = args.get("recipe", "")
        inputs = args.get("inputs") or {}
        skill_md = skill_map.get(skill_name)
        if skill_md is None:
            return ToolResult.not_found("Skill", skill_name).to_dict()
        recipe = find_recipe_entry(skill_md, recipe_name)
        if recipe is None:
            return ToolResult.not_found("Recipe", recipe_name).to_dict()
        errors = validate_recipe_inputs(recipe, inputs if isinstance(inputs, dict) else {})
        return ToolResult.ok(
            "Recipe inputs are valid." if not errors else "Recipe inputs are invalid.",
            valid=not errors,
            errors=errors,
            recipe=recipe_name,
            skill=skill_name,
        ).to_dict()

    def _handle_apply(params: Any) -> Any:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
        skill_name = args.get("skill", "")
        recipe_name = args.get("recipe", "")
        inputs = args.get("inputs") or {}
        skill_md = skill_map.get(skill_name)
        if skill_md is None:
            return ToolResult.not_found("Skill", skill_name).to_dict()
        recipe = find_recipe_entry(skill_md, recipe_name)
        if recipe is None:
            return ToolResult.not_found("Recipe", recipe_name).to_dict()
        if recipe.get("provenance", {}).get("format") != "recipe-pack":
            return ToolResult.invalid_input("recipes__apply requires a structured YAML recipe pack entry.").to_dict()
        errors = validate_recipe_inputs(recipe, inputs if isinstance(inputs, dict) else {})
        if errors:
            return ToolResult.invalid_input("Recipe inputs are invalid.", errors=errors).to_dict()
        return ToolResult.ok(
            f"Recipe '{recipe_name}' application plan ready.",
            skill=skill_name,
            recipe=recipe_name,
            inputs=inputs,
            target=args.get("target"),
            steps=recipe.get("steps", []),
            output_contract=recipe.get("output_contract"),
            provenance=recipe.get("provenance", {}),
        ).to_dict()

    specs = [
        ToolSpec(
            name="recipes__list",
            description=_RECIPES_LIST_DESCRIPTION,
            input_schema=_LIST_SCHEMA,
            handler=_handle_list,
            category=CATEGORY_RECIPES,
        ),
        ToolSpec(
            name="recipes__get",
            description=_RECIPES_GET_DESCRIPTION,
            input_schema=_GET_SCHEMA,
            handler=_handle_get,
            category=CATEGORY_RECIPES,
        ),
        ToolSpec(
            name="recipes__search",
            description=_RECIPES_SEARCH_DESCRIPTION,
            input_schema=_SEARCH_SCHEMA,
            handler=_handle_search,
            category=CATEGORY_RECIPES,
        ),
        ToolSpec(
            name="recipes__validate",
            description=_RECIPES_VALIDATE_DESCRIPTION,
            input_schema=_VALIDATE_SCHEMA,
            handler=_handle_validate,
            category=CATEGORY_RECIPES,
        ),
        ToolSpec(
            name="recipes__apply",
            description=_RECIPES_APPLY_DESCRIPTION,
            input_schema=_APPLY_SCHEMA,
            handler=_handle_apply,
            category=CATEGORY_RECIPES,
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
    "RecipeDefinition",
    "find_recipe_entry",
    "get_recipe_content",
    "get_recipes_path",
    "get_recipes_paths",
    "list_recipe_entries",
    "load_recipe_pack",
    "parse_recipe_anchors",
    "register_recipes_tools",
    "validate_recipe_inputs",
]
