r"""docs:// MCP resource provider for agent-facing format specs and usage guides.

This module implements the ``docs://`` URI scheme proposed in issue #435,
inspired by the Notion MCP pattern: instead of embedding full format
specifications in tool descriptions (which consumes tokens on every
``tools/list`` call), tool descriptions contain a brief pointer like
``"For the full output schema, fetch docs://output-format/call-action"``.

Agents fetch only the specifications they actually need, when they need them.

Built-in docs:// resources
--------------------------

| URI | Content |
|-----|---------|
| ``docs://output-format/call-action`` | ``tools/call`` return value schema |
| ``docs://output-format/list-actions`` | ``tools/list`` response structure |
| ``docs://skill-authoring/tools-yaml`` | ``tools.yaml`` schema + conventions |
| ``docs://skill-authoring/annotations`` | ``ToolAnnotations`` reference |
| ``docs://skill-authoring/sibling-files`` | SKILL.md sibling-file pattern (v0.15+) |
| ``docs://skill-authoring/thin-harness`` | thin-harness layer guide pointer |

Custom resources
----------------

Skills and adapters can register additional ``docs://`` resources via
:func:`register_docs_resource` or :func:`register_docs_resources_from_dir`.

Example::

    from dcc_mcp_core.docs_resources import (
        register_docs_resource,
        register_docs_server,
    )

    server = create_skill_server("maya", McpHttpConfig(port=8765))
    register_docs_server(server)           # registers all built-ins
    register_docs_resource(               # register a custom resource
        server,
        uri="docs://maya/scene-format",
        name="Maya Scene Format",
        description="Maya ASCII and binary scene format reference.",
        content="# Maya Scene Format\n\n...",
        mime="text/markdown",
    )
    handle = server.start()

"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

# ── Built-in docs content ─────────────────────────────────────────────────

_DOCS: dict[str, dict[str, str]] = {
    "docs://output-format/call-action": {
        "name": "tools/call Return Value Schema",
        "description": "Schema and field descriptions for the tools/call response envelope.",
        "mime": "text/markdown",
        "content": """\
# tools/call Return Value Schema

Every ``tools/call`` response from dcc-mcp-core returns a structured result.

## Success response

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"success\": true, \"message\": \"...\", \"prompt\": \"...\", \"error\": null, \"context\": {}}"
    }
  ],
  "structuredContent": {
    "success": true,
    "message": "Human-readable summary.",
    "prompt": "Optional next-action hint for the agent.",
    "error": null,
    "context": {
      "key": "value"
    }
  },
  "isError": false
}
```

## Error response

```json
{
  "content": [{"type": "text", "text": "{\"success\": false, \"message\": \"...\", \"error\": \"...\"}"}],
  "structuredContent": {
    "success": false,
    "message": "What went wrong.",
    "prompt": "How to recover.",
    "error": "ExceptionType: detail string",
    "context": {}
  },
  "isError": true
}
```

## Async (pending) response

When ``_meta.dcc.async=true`` or a ``progressToken`` is present:

```json
{
  "structuredContent": {
    "job_id": "<uuid>",
    "status": "pending",
    "parent_job_id": null
  }
}
```

Poll status via the ``jobs.get_status`` built-in tool.

## _meta.dcc.raw_trace (when enable_error_raw_trace=True)

On error, the envelope may include:

```json
{
  "_meta": {
    "dcc.raw_trace": {
      "underlying_call": "maya.cmds.polySphere(...)",
      "traceback": "Traceback ...",
      "recipe_hint": "references/RECIPES.md#create_sphere",
      "introspect_hint": "dcc_introspect__signature(qualname='maya.cmds.polySphere')"
    }
  }
}
```

## _meta.dcc.next_tools

On success or error, the server may append:

```json
{
  "_meta": {
    "dcc.next_tools": {
      "on_success": ["maya_geometry__bevel_edges"],
      "on_failure": ["dcc_diagnostics__screenshot"]
    }
  }
}
```
""",
    },
    "docs://output-format/list-actions": {
        "name": "tools/list Response Structure",
        "description": "Structure of the tools/list response and how to read tool metadata.",
        "mime": "text/markdown",
        "content": """\
# tools/list Response Structure

``tools/list`` returns a list of tool descriptors. Each descriptor:

```json
{
  "name": "create_sphere",
  "description": "Create a polygon sphere. When to use: ...",
  "inputSchema": {
    "type": "object",
    "properties": {
      "radius": {"type": "number", "description": "Sphere radius > 0."}
    },
    "required": ["radius"]
  },
  "annotations": {
    "readOnlyHint": false,
    "destructiveHint": false,
    "idempotentHint": true,
    "openWorldHint": false
  }
}
```

## Skill stubs (deferred loading)

Tools from unloaded skills appear as:

```json
{
  "name": "__skill__maya-geometry",
  "description": "Unloaded skill stub. Call load_skill to activate tools.",
  "_meta": {"dcc.deferred_hint": true}
}
```

Call ``load_skill(skill_name="maya-geometry")`` to replace the stub with real tools.

## Tool groups (inactive tools)

Tools in groups with ``default_active: false`` are hidden from ``tools/list``.
Activate via ``activate_tool_group(group=...)``.
""",
    },
    "docs://skill-authoring/tools-yaml": {
        "name": "tools.yaml Schema Reference",
        "description": "Schema and conventions for the tools.yaml sibling file.",
        "mime": "text/markdown",
        "content": """\
# tools.yaml Schema Reference

``tools.yaml`` is the sibling file that carries tool declarations for a SKILL.md.
Referenced via ``metadata.dcc-mcp.tools: tools.yaml`` in SKILL.md.

## Full schema

```yaml
tools:
  - name: create_sphere           # required; SEP-986 lowercase + underscores
    description: >-               # required; 3-layer structure (≤500 chars)
      Create a polygon sphere at the origin.
      When to use: when the user asks to add a sphere.
      How to use: pass radius > 0.
    annotations:                  # optional MCP ToolAnnotations
      read_only_hint: false
      destructive_hint: false
      idempotent_hint: true
      open_world_hint: false
    next-tools:                   # optional dcc-mcp-core extension
      on-success:
        - maya_geometry__bevel_edges
      on-failure:
        - dcc_diagnostics__screenshot
        - dcc_diagnostics__audit_log

groups:                           # optional progressive exposure
  - name: advanced
    default_active: false
    tools: [create_sphere]
```

## Tool description structure (3-layer, ≤500 chars)

```
<One-sentence what> (present tense).
When to use: <contrast with sibling tools>.
How to use: <preconditions, common pitfalls, follow-up tools>.
```

## next-tools

``next-tools`` is a dcc-mcp-core extension. It surfaces in the response as
``_meta.dcc.next_tools.on_success`` / ``on_failure``.
Both fields accept lists of fully-qualified tool names (``skill__tool``).
""",
    },
    "docs://skill-authoring/annotations": {
        "name": "ToolAnnotations Declaration Reference",
        "description": "How to declare MCP ToolAnnotations in tools.yaml.",
        "mime": "text/markdown",
        "content": """\
# ToolAnnotations Declaration Reference

MCP ``ToolAnnotations`` go in the ``annotations:`` map on each tool entry
in ``tools.yaml`` (never at the SKILL.md top level).

## Fields

| Field | Type | Meaning |
|-------|------|---------|
| ``read_only_hint`` | bool | Tool only reads data; no side effects |
| ``destructive_hint`` | bool | Tool may cause irreversible changes |
| ``idempotent_hint`` | bool | Repeated calls produce the same result |
| ``open_world_hint`` | bool | Tool may interact with external systems |
| ``deferred_hint`` | bool | **dcc-mcp-core extension** — schema deferred until load_skill |

## Example

```yaml
tools:
  - name: delete_scene_objects
    annotations:
      read_only_hint: false
      destructive_hint: true    # ← AI clients will confirm before calling
      idempotent_hint: false
      open_world_hint: false
```

## Flat vs nested form

Both forms are accepted. Nested wins whole-map when both are present.

```yaml
# Nested (canonical):
annotations:
  read_only_hint: true

# Flat shorthand (legacy, still parsed):
read_only_hint: true
```

``deferred_hint`` is a dcc-mcp-core extension; it rides in
``_meta["dcc.deferred_hint"]`` on ``tools/list`` and is never inside
the spec ``annotations`` map.
""",
    },
    "docs://skill-authoring/sibling-files": {
        "name": "SKILL.md Sibling-File Pattern (v0.15+)",
        "description": "The architectural rule for SKILL.md extensions: all payloads in sibling files.",
        "mime": "text/markdown",
        "content": """\
# SKILL.md Sibling-File Pattern (v0.15+ / issue #356)

Every dcc-mcp-core extension to SKILL.md MUST be expressed as:

1. A ``metadata.dcc-mcp.<feature>`` key (nested or flat form).
2. The key's value is a **glob or filename** pointing at a sibling file.
3. The payload lives in the sibling file, never inlined in SKILL.md.

## Canonical SKILL.md structure

```yaml
---
name: maya-animation
description: >-
  Domain skill — Maya animation keyframes and timeline.
  Use when the user asks to set/query keyframes.
  Not for geometry — use maya-geometry for that.
license: MIT
metadata:
  dcc-mcp:
    dcc: maya
    layer: domain
    tools: tools.yaml
    workflows: "workflows/*.workflow.yaml"
    prompts: "prompts/*.prompt.yaml"
    recipes: references/RECIPES.md
---
```

## Allowed top-level keys (agentskills.io 1.0)

Only: ``name``, ``description``, ``license``, ``compatibility``,
``metadata``, ``allowed-tools``.

Any other key at the top level is legacy and emits a deprecation warning.

## Why this is non-negotiable

- **Token efficiency**: agents pay only for sibling files they need.
- **Diffability**: one PR per workflow/prompt, not buried in SKILL.md.
- **Forward-compatible**: new extensions add a new key + schema, no re-negotiation.
- **Spec-compliant**: ``skills-ref validate`` passes.
""",
    },
    "docs://skill-authoring/thin-harness": {
        "name": "Thin-Harness Skill Layer",
        "description": "When to use thin-harness vs domain skills, and how to author them.",
        "mime": "text/markdown",
        "content": """\
# Thin-Harness Skill Layer

A thin-harness skill hands the agent a raw script executor + recipe book
instead of wrapper tools. Use it when the LLM training corpus already
contains thousands of examples of the native DCC API call.

## When to use thin-harness vs domain skill

| Signal | Use |
|--------|-----|
| Operation is 1:1 with a well-known DCC API call | **thin-harness** |
| Operation requires multi-step pipeline logic | **domain skill** |
| You're wrapping ``maya.cmds``, ``bpy.ops``, ``hou.*`` one-to-one | **thin-harness** |

## Layer value

```yaml
metadata:
  dcc-mcp:
    layer: thin-harness
    tools: tools.yaml
    recipes: references/RECIPES.md
    introspection: references/INTROSPECTION.md
```

## Template

See ``skills/templates/thin-harness/`` for a ready-to-copy starter.

## Full guide

Fetch the full guide from the repository:
``docs/guide/thin-harness.md``

Or read the ADR: ``docs/adr/003-thin-harness-skill-pattern.md``
""",
    },
}


# ── Registration helpers ───────────────────────────────────────────────────


def get_builtin_docs_uris() -> list[str]:
    """Return the list of built-in ``docs://`` resource URIs."""
    return list(_DOCS.keys())


def get_docs_content(uri: str) -> dict[str, str] | None:
    """Return the content dict for a ``docs://`` URI, or ``None`` if unknown.

    Parameters
    ----------
    uri:
        A ``docs://`` URI string (e.g. ``"docs://output-format/call-action"``).

    Returns
    -------
    dict with keys ``name``, ``description``, ``mime``, ``content``, or ``None``.

    """
    return _DOCS.get(uri)


def register_docs_resource(
    server: Any,
    *,
    uri: str,
    name: str,
    description: str,
    content: str,
    mime: str = "text/markdown",
) -> None:
    """Register a single ``docs://`` resource on *server*.

    The resource is added to the in-process docs registry and served via
    ``resources/list`` + ``resources/read``.

    Parameters
    ----------
    server:
        An ``McpHttpServer`` or compatible object exposing
        ``server.add_docs_resource(uri, name, description, content, mime)``.
        If the method is not present, this function logs a warning and returns
        gracefully so callers do not need to guard against older server versions.
    uri:
        Full URI string starting with ``docs://``.
    name:
        Short human-readable name shown in ``resources/list``.
    description:
        One-sentence description of what this resource contains.
    content:
        The document body (Markdown text or JSON string).
    mime:
        MIME type (default ``"text/markdown"``).

    """
    if not uri.startswith("docs://"):
        logger.warning("register_docs_resource: URI must start with 'docs://' — got %r", uri)
        return
    _DOCS[uri] = {"name": name, "description": description, "mime": mime, "content": content}
    try:
        server.add_docs_resource(uri=uri, name=name, description=description, content=content, mime=mime)
    except AttributeError:
        logger.debug("register_docs_resource: server.add_docs_resource not available (Rust-side pending)")
    except Exception as exc:
        logger.warning("register_docs_resource: %s", exc)


def register_docs_resources_from_dir(
    server: Any,
    *,
    directory: str | Path,
    uri_prefix: str = "docs://custom",
    glob: str = "**/*.md",
) -> list[str]:
    """Register all Markdown files under *directory* as ``docs://`` resources.

    Parameters
    ----------
    server:
        An ``McpHttpServer`` compatible object.
    directory:
        Root directory to scan for Markdown files.
    uri_prefix:
        URI prefix for registered resources (default ``"docs://custom"``).
    glob:
        Glob pattern relative to *directory* (default ``"**/*.md"``).

    Returns
    -------
    list[str]
        URIs of successfully registered resources.

    """
    root = Path(directory)
    if not root.is_dir():
        logger.warning("register_docs_resources_from_dir: directory not found: %s", root)
        return []

    registered: list[str] = []
    for path in sorted(root.glob(glob)):
        rel = path.relative_to(root)
        uri = f"{uri_prefix}/{rel.as_posix().replace('.md', '')}"
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as exc:
            logger.warning("register_docs_resources_from_dir: could not read %s: %s", path, exc)
            continue
        first_line = next((ln.lstrip("# ").strip() for ln in text.splitlines() if ln.strip()), str(rel))
        register_docs_resource(
            server,
            uri=uri,
            name=first_line[:80],
            description=f"Documentation: {rel}",
            content=text,
        )
        registered.append(uri)

    return registered


def register_docs_server(server: Any) -> None:
    """Register all built-in ``docs://`` resources on *server*.

    Call this **before** ``server.start()``.

    Parameters
    ----------
    server:
        An ``McpHttpServer`` compatible object.

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core import create_skill_server, McpHttpConfig
        from dcc_mcp_core.docs_resources import register_docs_server

        server = create_skill_server("maya", McpHttpConfig(port=8765))
        register_docs_server(server)
        handle = server.start()
        # Agents can now: resources/read docs://output-format/call-action

    """
    for uri, meta in _DOCS.items():
        register_docs_resource(
            server,
            uri=uri,
            name=meta["name"],
            description=meta["description"],
            content=meta["content"],
            mime=meta.get("mime", "text/markdown"),
        )


# ── Public API ────────────────────────────────────────────────────────────

__all__ = [
    "get_builtin_docs_uris",
    "get_docs_content",
    "register_docs_resource",
    "register_docs_resources_from_dir",
    "register_docs_server",
]
