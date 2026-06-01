"""Agent-facing MCP tools for host-local script materialization (#1222)."""

from __future__ import annotations

from collections.abc import Mapping
import json
import logging
from pathlib import Path
from typing import Any

from dcc_mcp_core._tool_registration import ToolSpec
from dcc_mcp_core._tool_registration import register_tools
from dcc_mcp_core.script_materialization import materialize_script

logger = logging.getLogger(__name__)


_MATERIALIZE_INPUT_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "content": {
            "type": "string",
            "description": "Script source; never echoed.",
        },
        "code": {
            "type": "string",
            "description": "Alias for content.",
        },
        "language": {
            "type": "string",
            "default": "python",
            "description": "Language/MIME label.",
        },
        "suffix": {
            "type": "string",
            "default": ".py",
            "description": "Host-local file suffix.",
        },
        "display_name": {
            "type": "string",
            "description": "Optional filename prefix.",
        },
        "reuse": {
            "type": "boolean",
            "default": False,
            "description": "Reuse unexpired matching content.",
        },
        "reuse_key": {
            "type": "string",
            "description": "Optional reuse namespace.",
        },
        "ttl_secs": {
            "type": "integer",
            "minimum": 1,
            "description": "Expiry in seconds.",
        },
        "session_id": {
            "type": "string",
            "description": "Logical session id.",
        },
        "tool_call_id": {
            "type": "string",
            "description": "Optional audit call id.",
        },
        "correlation_id": {
            "type": "string",
            "description": "Optional trace id.",
        },
    },
}


_MATERIALIZE_OUTPUT_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "file_ref": {"type": "object"},
        "file_ref_uri": {"type": "string"},
        "file_path": {"type": "string"},
        "path": {"type": "string"},
        "language": {"type": "string"},
        "suffix": {"type": "string"},
        "sha256": {"type": "string"},
        "bytes": {"type": "integer"},
        "created_at": {"type": "string"},
        "expires_at": {"type": ["string", "null"]},
        "ttl_secs": {"type": ["integer", "null"]},
        "dcc_type": {"type": "string"},
        "instance_id": {"type": "string"},
        "session_id": {"type": "string"},
        "script_id": {"type": "string"},
        "tool_call_id": {"type": ["string", "null"]},
        "correlation_id": {"type": ["string", "null"]},
        "reused": {"type": "boolean"},
    },
    "required": ["file_ref", "file_path", "sha256", "bytes", "dcc_type", "instance_id", "session_id", "reused"],
}


def register_script_materialization_tools(
    server: Any,
    *,
    dcc_name: str,
    instance_id: str | None = None,
    session_id: str = "default",
    root: str | Path | None = None,
) -> int:
    """Register the agent-facing ``materialize_script`` MCP/REST tool.

    The tool writes source to the configured host-local materialization store
    and returns descriptor metadata only. It intentionally never echoes raw
    script source, so gateway traces and admin rows can record hash/path/TTL
    metadata without storing the payload.
    """
    default_instance_id = instance_id or f"{dcc_name}-local"

    def handle_materialize(params: Any) -> dict[str, Any]:
        args = _coerce_mapping(params)
        content = _first_string(args, "content", "code")
        descriptor = materialize_script(
            content,
            dcc_type=_optional_string(args, "dcc_type", default=dcc_name),
            instance_id=_optional_string(args, "instance_id", default=default_instance_id),
            session_id=_optional_string(args, "session_id", default=session_id),
            language=_optional_string(args, "language", default="python"),
            suffix=_optional_string(args, "suffix", default=".py"),
            display_name=_optional_string(args, "display_name"),
            reuse=_optional_bool(args, "reuse", default=False),
            reuse_key=_optional_string(args, "reuse_key"),
            ttl_secs=_optional_positive_int(args, "ttl_secs"),
            root=root,
            tool_call_id=_optional_string(args, "tool_call_id"),
            correlation_id=_optional_string(args, "correlation_id"),
        )
        return descriptor.to_dict()

    return register_tools(
        server,
        [
            ToolSpec(
                name="materialize_script",
                description=(
                    "Write script source to the DCC host and return FileRef/path/hash metadata. "
                    "Use before execute-python tools that accept file_path; raw source is never echoed."
                ),
                input_schema=_MATERIALIZE_INPUT_SCHEMA,
                output_schema=_MATERIALIZE_OUTPUT_SCHEMA,
                handler=handle_materialize,
                category="execution",
                tags=["script", "materialize", "file-backed"],
                search_aliases=[
                    "materialize script",
                    "write temp script",
                    "file backed script",
                    "host local script",
                ],
                version="1.0.0",
            )
        ],
        dcc_name=dcc_name,
        log_prefix="register_script_materialization_tools",
        logger=logger,
    )


def _coerce_mapping(params: Any) -> Mapping[str, Any]:
    if params is None:
        return {}
    if isinstance(params, Mapping):
        return params
    if isinstance(params, str):
        loaded = json.loads(params)
        if isinstance(loaded, Mapping):
            return loaded
    raise TypeError("tool parameters must be a JSON object")


def _first_string(params: Mapping[str, Any], *names: str) -> str:
    for name in names:
        value = params.get(name)
        if value is None:
            continue
        if not isinstance(value, str):
            raise TypeError(f"{name} must be a string")
        if value:
            return value
    raise ValueError("Missing required script content; pass content or code")


def _optional_string(params: Mapping[str, Any], name: str, *, default: str | None = None) -> str | None:
    value = params.get(name, default)
    if value is None:
        return None
    if not isinstance(value, str):
        raise TypeError(f"{name} must be a string")
    return value


def _optional_bool(params: Mapping[str, Any], name: str, *, default: bool) -> bool:
    value = params.get(name, default)
    if isinstance(value, bool):
        return value
    raise TypeError(f"{name} must be a boolean")


def _optional_positive_int(params: Mapping[str, Any], name: str) -> int | None:
    value = params.get(name)
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{name} must be an integer")
    if value <= 0:
        raise ValueError(f"{name} must be greater than zero")
    return value


__all__ = ["register_script_materialization_tools"]
