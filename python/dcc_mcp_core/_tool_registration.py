"""Internal helper for registering MCP tools with consistent error handling.

Five Python modules previously implemented near-identical boilerplate to
register MCP tools on a server's registry — registry lookup, per-tool
``registry.register(...)`` + ``server.register_handler(...)`` with three
nested ``try/except`` blocks. This helper consolidates that pattern so that
each call site shrinks to a flat list of :class:`ToolSpec` plus a single
:func:`register_tools` call.

The module is underscore-prefixed because it is an implementation detail —
public callers should keep using the existing ``register_*_tools`` /
``register_*_tool`` functions in ``introspect``, ``recipes``, ``feedback``,
and ``workflow_yaml``.
"""

from __future__ import annotations

from dataclasses import dataclass
import logging
from typing import Any
from typing import Callable

from dcc_mcp_core import json_dumps

_DEFAULT_LOGGER = logging.getLogger(__name__)


@dataclass
class ToolSpec:
    """Declarative description of a single MCP tool to register.

    Parameters
    ----------
    name:
        Fully-qualified MCP tool name (e.g. ``"recipes__list"``).
    description:
        Human-readable description shown in ``tools/list``.
    input_schema:
        JSON Schema for the tool's input arguments. Will be serialised via
        :func:`dcc_mcp_core.json_dumps` before being passed to the registry.
    handler:
        Callable invoked by the server with the raw ``params`` argument
        (either a JSON string or a Python object, depending on transport).
    category:
        Tool category tag; defaults to ``"general"``.
    version:
        Tool version string; defaults to ``"1.0.0"``.
    output_schema:
        Optional JSON Schema for the tool's return payload (MCP 2025-06-18
        ``outputSchema``). When provided, the registry publishes it so
        clients on 2025-06-18 sessions can validate ``structuredContent``;
        the gateway drops it for 2025-03-26 sessions automatically (see
        ``crates/dcc-mcp-http/src/handler/dispatch.rs``).

    """

    name: str
    description: str
    input_schema: dict[str, Any]
    handler: Callable[[Any], Any]
    category: str = "general"
    version: str = "1.0.0"
    output_schema: dict[str, Any] | None = None


def register_tools(
    server: Any,
    specs: list[ToolSpec],
    *,
    dcc_name: str = "dcc",
    log_prefix: str = "register_tools",
    logger: logging.Logger | None = None,
) -> int:
    """Register a list of MCP tools on *server*'s registry.

    For each spec the function calls ``server.registry.register(...)`` and
    then ``server.register_handler(name, handler)``. Both calls are guarded
    so that a single tool failing to register does not abort the rest of the
    batch — partial availability is preferred over a hard failure.

    Parameters
    ----------
    server:
        MCP server compatible with ``server.registry`` (a ``ToolRegistry``)
        and ``server.register_handler(name, handler)``.
    specs:
        Tools to register, in declaration order.
    dcc_name:
        DCC tag stored on each tool's metadata.
    log_prefix:
        Prefix used in warning messages (typically the calling public
        function's name, e.g. ``"register_introspect_tools"``).
    logger:
        Logger to emit warnings on; defaults to this module's logger.
        Callers normally pass their own module-level logger so warnings
        appear under the originating module's namespace.

    Returns
    -------
    int
        Number of tools whose *handlers* were successfully attached. A spec
        whose ``registry.register`` call fails is skipped entirely and not
        counted.

    """
    log = logger if logger is not None else _DEFAULT_LOGGER
    try:
        registry = server.registry
    except Exception as exc:
        log.warning("%s: server.registry unavailable: %s", log_prefix, exc)
        return 0

    attached = 0
    for spec in specs:
        register_kwargs: dict[str, Any] = {
            "name": spec.name,
            "description": spec.description,
            "input_schema": json_dumps(spec.input_schema),
            "dcc": dcc_name,
            "category": spec.category,
            "version": spec.version,
        }
        if spec.output_schema is not None:
            register_kwargs["output_schema"] = json_dumps(spec.output_schema)
        try:
            registry.register(**register_kwargs)
        except TypeError as exc:
            # Older ToolRegistry builds may not accept ``output_schema`` yet.
            # Drop it and retry once rather than losing the whole registration.
            if spec.output_schema is not None and "output_schema" in str(exc):
                log.warning(
                    "%s: registry.register(%s) does not accept output_schema; retrying without it (%s)",
                    log_prefix,
                    spec.name,
                    exc,
                )
                register_kwargs.pop("output_schema", None)
                try:
                    registry.register(**register_kwargs)
                except Exception as exc2:  # pragma: no cover - defensive
                    log.warning("%s: register(%s) failed: %s", log_prefix, spec.name, exc2)
                    continue
            else:
                log.warning("%s: register(%s) failed: %s", log_prefix, spec.name, exc)
                continue
        except Exception as exc:
            log.warning("%s: register(%s) failed: %s", log_prefix, spec.name, exc)
            continue
        try:
            server.register_handler(spec.name, spec.handler)
            attached += 1
        except Exception as exc:
            log.warning("%s: register_handler(%s) failed: %s", log_prefix, spec.name, exc)
    return attached


__all__ = ["ToolSpec", "register_tools"]
