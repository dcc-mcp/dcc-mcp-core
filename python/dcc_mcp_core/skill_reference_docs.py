"""Skill reference documentation — list/read arbitrary sibling text files (#616+).

Agents need Markdown or notes beside ``scripts/`` without hard-coding one
filename. Skills declare explicit globs under
``metadata.dcc-mcp.skill-reference-docs``; otherwise we fall back to
scanning ``references/`` for common text types.

The legacy ``metadata.dcc-mcp.introspection`` single-file key is still
surfaced for backwards-compatibility with skills authored before #616,
with a ``DeprecationWarning`` to nudge authors onto the new key.

Public entry point: :func:`register_skill_reference_docs_tools`.
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any
import warnings

from dcc_mcp_core import json_loads
from dcc_mcp_core._tool_registration import ToolSpec
from dcc_mcp_core._tool_registration import register_tools
from dcc_mcp_core.constants import CATEGORY_DOCS
from dcc_mcp_core.constants import METADATA_DCC_MCP
from dcc_mcp_core.constants import METADATA_SKILL_REFERENCE_DOCS_KEY
from dcc_mcp_core.result_envelope import ToolResult

logger = logging.getLogger(__name__)

_TEXT_SUFFIXES = frozenset({".md", ".markdown", ".txt", ".rst"})

_MAX_LIST_FILES = 300
_MAX_READ_BYTES = 512 * 1024

_LIST_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "skill": {
            "type": "string",
            "description": "Skill name (directory / SKILL.md ``name``).",
        },
    },
    "required": ["skill"],
}

_READ_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "skill": {"type": "string", "description": "Skill name."},
        "path": {
            "type": "string",
            "description": "Relative path returned by ``skill_refs__list`` (POSIX separators).",
        },
    },
    "required": ["skill", "path"],
}

_LIST_DESCRIPTION = (
    "List readable reference documents for a skill (Markdown/text under declared globs or "
    "``references/``). When to use: after ``search_skills`` / ``get_skill_info`` when you need "
    "long-form notes not present in tool schemas. How to use: pass ``skill``, then "
    "``skill_refs__read`` with a ``path`` from this list."
)

_READ_DESCRIPTION = (
    "Read one reference document for a skill. When to use: after ``skill_refs__list``. "
    "Pass the exact ``path`` string from the list output."
)


def _parse_params(params: Any) -> dict[str, Any]:
    if isinstance(params, str):
        return dict(json_loads(params) or {})
    return dict(params or {})


def _flat_metadata_dict(metadata: Any) -> dict[str, Any]:
    raw = getattr(metadata, "metadata", None)
    if isinstance(raw, dict):
        return raw
    return {}


def _skill_reference_doc_globs(metadata: Any) -> list[str]:
    """Return glob patterns relative to the skill root, newest config wins over defaults."""
    md = _flat_metadata_dict(metadata)
    raw = md.get(METADATA_SKILL_REFERENCE_DOCS_KEY)
    nested = md.get(METADATA_DCC_MCP)
    if raw is None and isinstance(nested, dict):
        raw = nested.get("skill-reference-docs")

    globs: list[str] = []
    if isinstance(raw, list):
        globs = [str(x).strip().replace("\\", "/") for x in raw if str(x).strip()]
    elif isinstance(raw, str) and raw.strip():
        globs = [raw.strip().replace("\\", "/")]

    legacy = getattr(metadata, "introspection_file", None)
    if isinstance(legacy, str) and legacy.strip():
        # Pre-#616 skills used ``metadata.dcc-mcp.introspection: <path>``;
        # surface that path through ``skill_refs__*`` one more release so
        # existing studio skills keep working, but flag the rename so
        # authors migrate to ``skill-reference-docs: [<path>]``.
        skill_name = getattr(metadata, "name", None) or getattr(metadata, "skill_path", "<unknown>")
        warnings.warn(
            (
                f"Skill {skill_name!r} declares deprecated 'metadata.dcc-mcp.introspection'; "
                "rename it to 'metadata.dcc-mcp.skill-reference-docs: [<path>]'. "
                "The legacy key will stop being honoured in a future minor release."
            ),
            DeprecationWarning,
            stacklevel=2,
        )
        globs.append(legacy.strip().replace("\\", "/"))

    if globs:
        return _dedupe_preserve_order(globs)

    skill_path = getattr(metadata, "skill_path", "") or ""
    base = Path(skill_path)
    ref = base / "references"
    if ref.is_dir():
        return [
            "references/*.md",
            "references/**/*.md",
            "references/*.txt",
            "references/**/*.txt",
        ]
    return []


def _dedupe_preserve_order(items: list[str]) -> list[str]:
    seen: set[str] = set()
    out: list[str] = []
    for it in items:
        if it not in seen:
            seen.add(it)
            out.append(it)
    return out


def _collect_reference_files(skill_root: Path, globs: list[str]) -> list[dict[str, Any]]:
    root = skill_root.resolve()
    found: dict[str, Path] = {}
    for pattern in globs:
        if not pattern or pattern.startswith("/"):
            continue
        try:
            for hit in root.glob(pattern):
                if not hit.is_file():
                    continue
                suf = hit.suffix.lower()
                if suf not in _TEXT_SUFFIXES:
                    continue
                try:
                    rel = hit.resolve().relative_to(root).as_posix()
                except ValueError:
                    continue
                found[rel] = hit.resolve()
                if len(found) >= _MAX_LIST_FILES:
                    return _entries_from_map(found)
        except OSError as exc:
            logger.debug("skill_refs glob %r under %s: %s", pattern, root, exc)
    return _entries_from_map(found)


def _entries_from_map(found: dict[str, Path]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for rel in sorted(found.keys()):
        p = found[rel]
        try:
            size = p.stat().st_size
        except OSError:
            size = -1
        rows.append({"path": rel, "size_bytes": size})
    return rows


def _resolve_safe_path(skill_root: Path, relative: str) -> Path | None:
    root = skill_root.resolve()
    rel = relative.strip().replace("\\", "/").lstrip("/")
    if not rel or ".." in Path(rel).parts:
        return None
    try:
        cand = (skill_root / rel).resolve()
        cand.relative_to(root)
    except (OSError, ValueError):
        return None
    return cand if cand.is_file() else None


def _handle_list(skill_map: dict[str, Any], params: Any) -> dict[str, Any]:
    args = _parse_params(params)
    skill_name = str(args.get("skill") or "")
    md = skill_map.get(skill_name)
    if md is None:
        return ToolResult(success=False, message=f"Skill '{skill_name}' not found.").to_dict()
    skill_path = getattr(md, "skill_path", "") or ""
    if not skill_path:
        return ToolResult.fail("Skill has no skill_path.", error="invalid_skill").to_dict()
    root = Path(skill_path)
    if not root.is_dir():
        return ToolResult.fail(f"Skill directory missing: {skill_path}", error="invalid_skill").to_dict()
    globs = _skill_reference_doc_globs(md)
    if not globs:
        return ToolResult.ok(
            f"Skill '{skill_name}' has no reference-doc globs and no references/ directory.",
            skill=skill_name,
            globs=[],
            files=[],
        ).to_dict()
    files = _collect_reference_files(root, globs)
    return ToolResult.ok(
        f"Found {len(files)} reference file(s) for '{skill_name}'.",
        skill=skill_name,
        globs=globs,
        files=files,
    ).to_dict()


def _handle_read(skill_map: dict[str, Any], params: Any) -> dict[str, Any]:
    args = _parse_params(params)
    skill_name = str(args.get("skill") or "")
    rel_path = str(args.get("path") or "")
    md = skill_map.get(skill_name)
    if md is None:
        return ToolResult(success=False, message=f"Skill '{skill_name}' not found.").to_dict()
    skill_path = getattr(md, "skill_path", "") or ""
    root = Path(skill_path)
    if not root.is_dir():
        return ToolResult.fail(f"Skill directory missing: {skill_path}", error="invalid_skill").to_dict()

    target = _resolve_safe_path(root, rel_path)
    if target is None:
        return ToolResult.invalid_input(
            "Invalid path — use a relative path from skill_refs__list with no '..'.",
        ).to_dict()

    suf = target.suffix.lower()
    if suf not in _TEXT_SUFFIXES:
        return ToolResult.invalid_input("Only text reference types are readable (.md, .txt, …).").to_dict()

    allowed = {e["path"] for e in _collect_reference_files(root, _skill_reference_doc_globs(md))}
    rel_posix = target.resolve().relative_to(root.resolve()).as_posix()
    if rel_posix not in allowed:
        return ToolResult.invalid_input(
            "Path is not among indexed reference files; call skill_refs__list first.",
        ).to_dict()

    try:
        raw = target.read_bytes()
    except OSError as exc:
        return ToolResult.fail(f"Cannot read file: {exc}", error="io_error").to_dict()
    if len(raw) > _MAX_READ_BYTES:
        return ToolResult.invalid_input(
            f"File exceeds max read size ({_MAX_READ_BYTES} bytes).",
        ).to_dict()
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        return ToolResult.invalid_input("File is not valid UTF-8.").to_dict()

    return ToolResult.ok(
        f"Read '{rel_posix}'",
        skill=skill_name,
        path=rel_posix,
        content=text,
    ).to_dict()


def register_skill_reference_docs_tools(
    server: Any,
    *,
    skills: list[Any],
    dcc_name: str = "dcc",
) -> None:
    """Register ``skill_refs__list`` and ``skill_refs__read`` on *server*.

    Parameters
    ----------
    server:
        ``McpHttpServer``-compatible object with ``registry`` and ``register_handler``.
    skills:
        ``SkillMetadata`` instances (same source as ``register_recipes_tools``).
    dcc_name:
        DCC tag for tool registration.

    """
    skill_map: dict[str, Any] = {getattr(s, "name", ""): s for s in skills}
    specs = [
        ToolSpec(
            name="skill_refs__list",
            description=_LIST_DESCRIPTION,
            input_schema=_LIST_SCHEMA,
            handler=lambda params: _handle_list(skill_map, params),
            category=CATEGORY_DOCS,
        ),
        ToolSpec(
            name="skill_refs__read",
            description=_READ_DESCRIPTION,
            input_schema=_READ_SCHEMA,
            handler=lambda params: _handle_read(skill_map, params),
            category=CATEGORY_DOCS,
        ),
    ]
    register_tools(
        server,
        specs,
        dcc_name=dcc_name,
        log_prefix="register_skill_reference_docs_tools",
        logger=logger,
    )


__all__ = [
    "register_skill_reference_docs_tools",
]
