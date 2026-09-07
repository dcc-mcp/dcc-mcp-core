"""Shared agent-first adapter Install SOP contracts."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from typing import Mapping

INSTALL_SOP_SCHEMA_VERSION = 1

INSTALL_EXIT_OK = 0
INSTALL_EXIT_PREFLIGHT = 10
INSTALL_EXIT_ACQUIRE = 20
INSTALL_EXIT_INSTALL = 30
INSTALL_EXIT_VERIFY = 40
INSTALL_EXIT_REQUIRES_RESTART = 50

INSTALL_EXIT_CODES = {
    "ok": INSTALL_EXIT_OK,
    "preflight": INSTALL_EXIT_PREFLIGHT,
    "acquire": INSTALL_EXIT_ACQUIRE,
    "install": INSTALL_EXIT_INSTALL,
    "verify": INSTALL_EXIT_VERIFY,
    "requires_restart": INSTALL_EXIT_REQUIRES_RESTART,
}

_SCHEMA_PATH = Path(__file__).resolve().parent.parent / "schemas" / "adapter-install-sop-v1.schema.json"


def load_install_sop_schema() -> dict[str, Any]:
    """Return a fresh copy of the packaged Install SOP JSON Schema."""
    return json.loads(_SCHEMA_PATH.read_text(encoding="utf-8"))


def validate_install_sop_report(report: Mapping[str, Any]) -> None:
    """Validate an Install SOP report with full JSON Schema Draft 2020-12 semantics.

    The validator is compiled into the native Core wheel, so adapters do not
    need the third-party Python ``jsonschema`` package. The pure ``py37-lite``
    wheel intentionally cannot provide this native API.

    Raises:
        TypeError: If *report* is not a mapping.
        RuntimeError: If the native Core validation binding is unavailable.
        ValueError: If the report is not JSON-compatible or violates the schema.

    """
    if not isinstance(report, Mapping):
        raise TypeError("Install SOP report must be a mapping")

    try:
        schema_json = json.dumps(
            load_install_sop_schema(),
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
        )
        report_json = json.dumps(
            report,
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
        )
    except (TypeError, ValueError) as exc:
        raise ValueError(f"Install SOP report is not JSON-compatible: {exc}") from exc

    try:
        from dcc_mcp_core import _core
    except ImportError as exc:
        raise RuntimeError(
            "Install SOP validation requires a native dcc-mcp-core wheel; "
            "the py37-lite wheel does not include Draft 2020-12 validation"
        ) from exc

    validator = getattr(_core, "_validate_json_schema_draft_2020_12", None)
    if validator is None:
        raise RuntimeError(
            "Installed dcc-mcp-core native wheel does not provide Install SOP "
            "Draft 2020-12 validation; upgrade dcc-mcp-core"
        )

    errors = validator(schema_json, report_json)
    if errors:
        details = "\n".join(f"- {error}" for error in errors)
        raise ValueError(f"Install SOP report failed schema validation:\n{details}")


__all__ = [
    "INSTALL_EXIT_ACQUIRE",
    "INSTALL_EXIT_CODES",
    "INSTALL_EXIT_INSTALL",
    "INSTALL_EXIT_OK",
    "INSTALL_EXIT_PREFLIGHT",
    "INSTALL_EXIT_REQUIRES_RESTART",
    "INSTALL_EXIT_VERIFY",
    "INSTALL_SOP_SCHEMA_VERSION",
    "load_install_sop_schema",
    "validate_install_sop_report",
]
