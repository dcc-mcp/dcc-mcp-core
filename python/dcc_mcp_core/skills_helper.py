"""Stable Rust-backed helper namespace for DCC-MCP skill scripts.

Skill authors should import dependency-light helpers from this module instead
of reaching across implementation modules or adding small third-party runtime
dependencies for JSON, YAML, validation, result envelopes, or cancellation.

The module intentionally keeps imports lazy: importing
``dcc_mcp_core.skills_helper`` does not load the PyO3 extension until a
Rust-backed helper is used.
"""

from __future__ import annotations

import importlib
from typing import Any


class SkillHelperError(Exception):
    """Base exception for skill-helper failures raised by future helpers."""


def _core_symbol(name: str) -> Any:
    from dcc_mcp_core import _core

    return getattr(_core, name)


def json_dumps(obj: Any, *, ensure_ascii: bool = True, indent: int | None = None) -> str:
    """Serialize *obj* to JSON using the Rust-backed codec."""
    return _core_symbol("json_dumps")(obj, ensure_ascii=ensure_ascii, indent=indent)


def json_loads(s: str) -> Any:
    """Deserialize JSON text using the Rust-backed codec."""
    return _core_symbol("json_loads")(s)


def yaml_dumps(obj: Any) -> str:
    """Serialize *obj* to YAML using the Rust-backed codec."""
    return _core_symbol("yaml_dumps")(obj)


def yaml_loads(s: str) -> Any:
    """Deserialize YAML text using the Rust-backed codec."""
    return _core_symbol("yaml_loads")(s)


def skill_error_from_exception(
    exc: BaseException,
    *,
    message: str | None = None,
    error: str | None = None,
    prompt: str | None = None,
    **context: Any,
) -> dict[str, Any]:
    """Convert an exception into the standard skill error dictionary shape."""
    from dcc_mcp_core.skill import skill_error

    return skill_error(
        message or str(exc) or type(exc).__name__,
        error or type(exc).__name__,
        prompt=prompt,
        **context,
    )


_LAZY_EXPORTS: dict[str, str] = {
    # Result envelopes and validation from the Rust extension.
    "deserialize_result": "dcc_mcp_core._core",
    "error_result": "dcc_mcp_core._core",
    "from_exception": "dcc_mcp_core._core",
    "serialize_result": "dcc_mcp_core._core",
    "success_result": "dcc_mcp_core._core",
    "ToolValidator": "dcc_mcp_core._core",
    "validate_action_result": "dcc_mcp_core._core",
    # Shared MCP/REST call envelope normalization.
    "normalize_tool_arguments": "dcc_mcp_core.host",
    "normalize_tool_meta": "dcc_mcp_core.host",
    # Schema derivation helpers.
    "derive_parameters_schema": "dcc_mcp_core.schema",
    "derive_schema": "dcc_mcp_core.schema",
    "schema_from_doc": "dcc_mcp_core.schema",
    "tool_spec_from_callable": "dcc_mcp_core.schema",
    # Cooperative cancellation.
    "CancelledError": "dcc_mcp_core.cancellation",
    "check_cancelled": "dcc_mcp_core.cancellation",
    "check_dcc_cancelled": "dcc_mcp_core.cancellation",
    # Pure-Python skill script result helpers.
    "run_main": "dcc_mcp_core.skill",
    "skill_entry": "dcc_mcp_core.skill",
    "skill_error": "dcc_mcp_core.skill",
    "skill_error_with_trace": "dcc_mcp_core.skill",
    "skill_exception": "dcc_mcp_core.skill",
    "skill_success": "dcc_mcp_core.skill",
    "skill_warning": "dcc_mcp_core.skill",
}


def __getattr__(name: str) -> Any:
    module_path = _LAZY_EXPORTS.get(name)
    if module_path is None:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    module = importlib.import_module(module_path)
    value = getattr(module, name)
    globals()[name] = value
    return value


def __dir__() -> list[str]:
    return sorted(__all__)


_DIRECT_EXPORTS = [
    "SkillHelperError",
    "json_dumps",
    "json_loads",
    "skill_error_from_exception",
    "yaml_dumps",
    "yaml_loads",
]


__all__ = sorted([*_DIRECT_EXPORTS, *_LAZY_EXPORTS])
