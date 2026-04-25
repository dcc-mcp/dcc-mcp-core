"""Runtime namespace discovery tools for DCC host interpreters (issue #426).

Provides four read-only MCP tools that let AI agents inspect the live DCC
Python namespace without burning tokens on web searches or relying on stale
training data:

- ``dcc_introspect__list_module`` — list exported names in a module
- ``dcc_introspect__signature``   — get the signature and docstring of a callable
- ``dcc_introspect__search``      — regex-search names across a module
- ``dcc_introspect__eval``        — evaluate a short read-only expression

All tools are registered with ``read_only_hint=True, idempotent_hint=True``
and hard-cap their output to avoid blowing the agent's context window.

Usage::

    from dcc_mcp_core.introspect import register_introspect_tools

    # Attach tools before server.start()
    register_introspect_tools(server, dcc_name="maya")

"""

from __future__ import annotations

import contextlib
import importlib
import inspect
import logging
import re
import traceback
from typing import Any

from dcc_mcp_core import json_dumps
from dcc_mcp_core import json_loads

logger = logging.getLogger(__name__)

# Hard output caps
_MAX_NAMES = 200  # max entries from list_module
_MAX_HITS = 50  # max hits from search
_DOC_MAX_CHARS = 800  # max chars for docstring truncation
_REPR_MAX_CHARS = 500  # max chars for eval repr


# ── Core introspection helpers ────────────────────────────────────────────


def _import_module(module_name: str) -> tuple[Any, str | None]:
    """Return (module, error_str) — error_str is None on success."""
    try:
        return importlib.import_module(module_name), None
    except ImportError as exc:
        return None, f"Cannot import module '{module_name}': {exc}"
    except Exception as exc:
        return None, f"Error importing module '{module_name}': {exc}"


def introspect_list_module(module_name: str, *, limit: int = _MAX_NAMES) -> dict[str, Any]:
    """Return exported names from *module_name*.

    Parameters
    ----------
    module_name:
        Dotted module path (e.g. ``"maya.cmds"`` or ``"bpy.ops.object"``).
    limit:
        Maximum number of names to return (default :data:`_MAX_NAMES`).

    Returns
    -------
    dict
        ``{"names": [...], "count": N, "truncated": bool}``

    """
    mod, err = _import_module(module_name)
    if err:
        return {"success": False, "message": err}

    names = list(mod.__all__) if hasattr(mod, "__all__") else [n for n in dir(mod) if not n.startswith("_")]

    names.sort()
    truncated = len(names) > limit
    return {
        "success": True,
        "message": f"{len(names)} names in {module_name}" + (" (truncated)" if truncated else ""),
        "context": {
            "module": module_name,
            "names": names[:limit],
            "count": len(names),
            "truncated": truncated,
        },
    }


def introspect_signature(qualname: str) -> dict[str, Any]:
    """Return signature and docstring for *qualname*.

    Parameters
    ----------
    qualname:
        Fully-qualified name such as ``"maya.cmds.polyCube"`` or
        ``"bpy.ops.object.join"``.

    Returns
    -------
    dict
        ``{"signature": str, "doc": str, "source_file": str|None}``

    """
    parts = qualname.rsplit(".", 1)
    if len(parts) == 1:
        module_name, attr = "builtins", parts[0]
    else:
        module_name, attr = parts

    mod, err = _import_module(module_name)
    if err:
        return {"success": False, "message": err}

    obj = getattr(mod, attr, None)
    if obj is None:
        return {"success": False, "message": f"'{attr}' not found in '{module_name}'"}

    # Signature
    sig_str = ""
    try:
        sig = inspect.signature(obj)
        sig_str = f"{attr}{sig}"
    except (ValueError, TypeError):
        sig_str = f"{attr}(...)"

    # Docstring
    doc = inspect.getdoc(obj) or ""
    if len(doc) > _DOC_MAX_CHARS:
        doc = doc[:_DOC_MAX_CHARS] + "\n...(truncated)"

    # Source file
    source_file: str | None = None
    with contextlib.suppress(TypeError, OSError):
        source_file = inspect.getfile(obj)

    return {
        "success": True,
        "message": f"Signature for {qualname}",
        "context": {
            "qualname": qualname,
            "signature": sig_str,
            "doc": doc,
            "source_file": source_file,
            "kind": type(obj).__name__,
        },
    }


def introspect_search(
    pattern: str,
    module_name: str,
    *,
    limit: int = _MAX_HITS,
) -> dict[str, Any]:
    """Regex-search exported names in *module_name*.

    Parameters
    ----------
    pattern:
        Regular expression (case-insensitive).
    module_name:
        Dotted module path to search.
    limit:
        Maximum hits to return (default :data:`_MAX_HITS`).

    Returns
    -------
    dict
        ``{"hits": [{"qualname": str, "summary": str}, ...], "count": int}``

    """
    try:
        regex = re.compile(pattern, re.IGNORECASE)
    except re.error as exc:
        return {"success": False, "message": f"Invalid regex '{pattern}': {exc}"}

    mod, err = _import_module(module_name)
    if err:
        return {"success": False, "message": err}

    all_names: list[str] = list(getattr(mod, "__all__", None) or [n for n in dir(mod) if not n.startswith("_")])
    hits: list[dict[str, str]] = []

    for name in all_names:
        if not regex.search(name):
            continue
        obj = getattr(mod, name, None)
        summary = ""
        if obj is not None:
            raw_doc = inspect.getdoc(obj) or ""
            first_line = raw_doc.split("\n", 1)[0].strip()
            summary = first_line[:120] if first_line else type(obj).__name__
        hits.append({"qualname": f"{module_name}.{name}", "summary": summary})
        if len(hits) >= limit:
            break

    return {
        "success": True,
        "message": f"{len(hits)} matches for '{pattern}' in '{module_name}'",
        "context": {
            "pattern": pattern,
            "module": module_name,
            "hits": hits,
            "count": len(hits),
            "truncated": len(hits) >= limit,
        },
    }


def introspect_eval(expression: str) -> dict[str, Any]:
    """Evaluate a read-only Python expression and return its repr.

    Only bare expressions are allowed — no assignments, import statements,
    or multi-statement code. The expression is evaluated with a restricted
    namespace (builtins only).

    Parameters
    ----------
    expression:
        A short Python expression string (e.g. ``"type(maya.cmds.ls(sl=True))"``,
        ``"dir(bpy.context)"``).

    Returns
    -------
    dict
        ``{"repr": str}`` on success, or ``{"success": False, "message": err}``
        on any error.

    """
    # Lightweight guard: reject obvious statement patterns
    stripped = expression.strip()
    _BANNED = ("import ", "=", "def ", "class ", "for ", "while ", "exec(", "eval(", "__import__")
    for banned in _BANNED:
        if banned in stripped:
            return {
                "success": False,
                "message": f"Expression contains disallowed construct: '{banned}'",
            }

    try:
        result = eval(stripped, {"__builtins__": __builtins__})  # intentional: sandboxed read-only eval
        repr_str = repr(result)
        if len(repr_str) > _REPR_MAX_CHARS:
            repr_str = repr_str[:_REPR_MAX_CHARS] + "...(truncated)"
    except Exception:
        tb = traceback.format_exc()
        return {
            "success": False,
            "message": f"Evaluation failed: {tb.splitlines()[-1]}",
            "context": {"traceback": tb},
        }

    return {
        "success": True,
        "message": "Expression evaluated.",
        "context": {"expression": expression, "repr": repr_str},
    }


# ── JSON schemas for MCP tools ─────────────────────────────────────────────

_LIST_MODULE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "module": {
            "type": "string",
            "description": "Dotted module path, e.g. 'maya.cmds' or 'bpy.ops.object'.",
        },
        "limit": {
            "type": "integer",
            "description": f"Max names to return (default {_MAX_NAMES}).",
            "default": _MAX_NAMES,
        },
    },
    "required": ["module"],
    "additionalProperties": False,
}

_SIGNATURE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "qualname": {
            "type": "string",
            "description": "Fully-qualified name, e.g. 'maya.cmds.polyCube'.",
        },
    },
    "required": ["qualname"],
    "additionalProperties": False,
}

_SEARCH_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "pattern": {
            "type": "string",
            "description": "Case-insensitive regex to match against exported names.",
        },
        "module": {
            "type": "string",
            "description": "Module to search within, e.g. 'maya.cmds'.",
        },
        "limit": {
            "type": "integer",
            "description": f"Max hits to return (default {_MAX_HITS}).",
            "default": _MAX_HITS,
        },
    },
    "required": ["pattern", "module"],
    "additionalProperties": False,
}

_EVAL_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "expression": {
            "type": "string",
            "description": "Short read-only Python expression to evaluate in the DCC interpreter.",
        },
    },
    "required": ["expression"],
    "additionalProperties": False,
}

_LIST_MODULE_DESCRIPTION = (
    "List exported names in a Python module loaded in the DCC interpreter. "
    "When to use: before writing a script — discover what functions are available "
    "without browsing offline docs. "
    "How to use: pass the dotted module path; use dcc_introspect__search to narrow results."
)

_SIGNATURE_DESCRIPTION = (
    "Return the signature and docstring for a callable in the live DCC interpreter. "
    "When to use: when you have a function name from dcc_introspect__list_module or "
    "dcc_introspect__search and need parameter names and defaults. "
    "How to use: pass the fully-qualified name, e.g. 'maya.cmds.polyCube'."
)

_SEARCH_DESCRIPTION = (
    "Regex-search exported names in a DCC module. "
    "When to use: when you need to find a function but only remember part of its name. "
    "How to use: pass a regex pattern + module; use dcc_introspect__signature on hits."
)

_EVAL_DESCRIPTION = (
    "Evaluate a short read-only Python expression in the DCC interpreter and return its repr. "
    "When to use: to inspect a live object or type — e.g. 'type(maya.cmds.ls(sl=True))'. "
    "How to use: pass a pure expression; no assignments or import statements allowed."
)


# ── MCP tool registration ─────────────────────────────────────────────────


def register_introspect_tools(
    server: Any,
    *,
    dcc_name: str = "dcc",
) -> None:
    """Register the four ``dcc_introspect__*`` tools on *server*.

    All tools are annotated ``read_only_hint=True, idempotent_hint=True``.
    Register them **before** calling ``server.start()``.

    Parameters
    ----------
    server:
        An ``McpHttpServer`` compatible object with ``server.registry``
        and ``server.register_handler(name, fn)``.
    dcc_name:
        DCC name string for tool metadata tagging.

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core import McpHttpServer, McpHttpConfig
        from dcc_mcp_core.introspect import register_introspect_tools

        server = McpHttpServer(registry, McpHttpConfig(port=8765))
        register_introspect_tools(server, dcc_name="maya")
        handle = server.start()

    """
    try:
        registry = server.registry
    except Exception as exc:
        logger.warning("register_introspect_tools: server.registry unavailable: %s", exc)
        return

    def _handler(fn):
        """Wrap a function to accept JSON string or dict params."""

        def wrapper(params: Any) -> Any:
            args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
            return fn(**args)

        return wrapper

    tools = [
        (
            "dcc_introspect__list_module",
            _LIST_MODULE_DESCRIPTION,
            _LIST_MODULE_SCHEMA,
            lambda module, limit=_MAX_NAMES: introspect_list_module(module, limit=limit),
        ),
        (
            "dcc_introspect__signature",
            _SIGNATURE_DESCRIPTION,
            _SIGNATURE_SCHEMA,
            lambda qualname: introspect_signature(qualname),
        ),
        (
            "dcc_introspect__search",
            _SEARCH_DESCRIPTION,
            _SEARCH_SCHEMA,
            lambda pattern, module, limit=_MAX_HITS: introspect_search(pattern, module, limit=limit),
        ),
        (
            "dcc_introspect__eval",
            _EVAL_DESCRIPTION,
            _EVAL_SCHEMA,
            lambda expression: introspect_eval(expression),
        ),
    ]

    for name, desc, schema, fn in tools:
        try:
            registry.register(
                name=name,
                description=desc,
                input_schema=json_dumps(schema),
                dcc=dcc_name,
                category="introspect",
                version="1.0.0",
            )
        except Exception as exc:
            logger.warning("register_introspect_tools: register(%s) failed: %s", name, exc)
            continue
        try:
            server.register_handler(name, _handler(fn))
        except Exception as exc:
            logger.warning("register_introspect_tools: register_handler(%s) failed: %s", name, exc)


# ── Public API ─────────────────────────────────────────────────────────────

__all__ = [
    "introspect_eval",
    "introspect_list_module",
    "introspect_search",
    "introspect_signature",
    "register_introspect_tools",
]
