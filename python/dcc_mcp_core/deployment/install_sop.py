"""Shared agent-first adapter Install SOP contracts."""

from __future__ import annotations

import hashlib
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

_INSTALL_SOP_SCHEMA_ID = "https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json"
_INSTALL_SOP_SCHEMA_DIALECT = "https://json-schema.org/draft/2020-12/schema"
_INSTALL_SOP_SCHEMA_SHA256 = "3ca25788439917b4d4c0617230a762f9797756b5b54f45c8c4149f975b90f904"
_MAX_NATIVE_DIAGNOSTIC_BYTES = 512
_MAX_NATIVE_DIAGNOSTICS_BYTES = 8192
_MAX_SEMANTIC_ERRORS = 32
_SCHEMA_PATH = Path(__file__).resolve().parent.parent / "schemas" / "adapter-install-sop-v1.schema.json"


class _DuplicateJsonKeyError(ValueError):
    """Internal marker for duplicate JSON object keys."""


def _unique_json_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise _DuplicateJsonKeyError("duplicate_json_object_key")
        result[key] = value
    return result


def _decode_unique_json(document: str) -> Any:
    return json.loads(document, object_pairs_hook=_unique_json_object)


def _contains_external_ref(value: Any) -> bool:
    if isinstance(value, list):
        return any(_contains_external_ref(item) for item in value)
    if not isinstance(value, dict):
        return False
    for key, item in value.items():
        if key in {"$ref", "$dynamicRef", "$recursiveRef"}:
            if not isinstance(item, str) or (item != "#" and not item.startswith("#/")):
                return True
        elif _contains_external_ref(item):
            return True
    return False


def _schema_integrity_error(code: str) -> RuntimeError:
    return RuntimeError(f"Install SOP schema integrity error: {code}")


def _load_install_sop_schema_document() -> tuple[str, dict[str, Any]]:
    try:
        schema_bytes = _SCHEMA_PATH.read_bytes()
    except OSError as exc:
        raise _schema_integrity_error("schema_unavailable") from exc

    try:
        schema_json = schema_bytes.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise _schema_integrity_error("schema_invalid_utf8") from exc

    try:
        schema = _decode_unique_json(schema_json)
    except _DuplicateJsonKeyError as exc:
        raise _schema_integrity_error("schema_duplicate_key") from exc
    except (TypeError, ValueError) as exc:
        raise _schema_integrity_error("schema_invalid_json") from exc

    if not isinstance(schema, dict):
        raise _schema_integrity_error("schema_invalid_json")
    if schema.get("$id") != _INSTALL_SOP_SCHEMA_ID:
        raise _schema_integrity_error("schema_identity_mismatch")
    if schema.get("$schema") != _INSTALL_SOP_SCHEMA_DIALECT:
        raise _schema_integrity_error("schema_dialect_mismatch")
    if _contains_external_ref(schema):
        raise _schema_integrity_error("schema_external_ref")
    if hashlib.sha256(schema_bytes).hexdigest() != _INSTALL_SOP_SCHEMA_SHA256:
        raise _schema_integrity_error("schema_digest_mismatch")
    return schema_json, schema


def load_install_sop_schema() -> dict[str, Any]:
    """Return a verified fresh copy of the packaged Install SOP JSON Schema."""
    return _load_install_sop_schema_document()[1]


def _contains_control_character(value: str) -> bool:
    return any(ord(character) < 0x20 or ord(character) == 0x7F for character in value)


def _semantic_validation_errors(report: Mapping[str, Any]) -> list[str]:
    errors = []
    truncated = False

    def add(code: str, path: str) -> None:
        nonlocal truncated
        if len(errors) >= _MAX_SEMANTIC_ERRORS:
            truncated = True
            return
        errors.append(f"code={code} path={path}")

    for collection_name in ("steps", "next_steps"):
        collection = report.get(collection_name)
        if not isinstance(collection, list):
            continue
        item_name = "step" if collection_name == "steps" else "next_step"
        seen_ids = set()
        for index, item in enumerate(collection):
            if not isinstance(item, Mapping):
                continue
            item_id = item.get("id")
            if isinstance(item_id, str):
                if item_id in seen_ids:
                    add(f"duplicate_{item_name}_id", f"/{collection_name}/{index}/id")
                else:
                    seen_ids.add(item_id)
                if _contains_control_character(item_id):
                    add("control_character", f"/{collection_name}/{index}/id")

            if collection_name != "next_steps":
                continue
            command = item.get("command")
            if isinstance(command, list):
                for argument_index, argument in enumerate(command):
                    if isinstance(argument, str) and _contains_control_character(argument):
                        add(
                            "control_character",
                            f"/next_steps/{index}/command/{argument_index}",
                        )
            file_edit = item.get("file_edit")
            if isinstance(file_edit, Mapping):
                path = file_edit.get("path")
                if isinstance(path, str) and _contains_control_character(path):
                    add("control_character", f"/next_steps/{index}/file_edit/path")

    receipt_path = report.get("receipt_path")
    if isinstance(receipt_path, str) and _contains_control_character(receipt_path):
        add("control_character", "/receipt_path")

    if truncated:
        errors[-1] = "code=semantic_validation_truncated path=/"
    return errors


def _raise_native_runtime_error(code: str, cause: Exception | None = None) -> None:
    error = RuntimeError(f"Install SOP validator runtime error: {code}")
    if cause is None:
        raise error
    raise error from cause


def validate_install_sop_report(report: Mapping[str, Any]) -> None:
    """Validate the canonical Install SOP v1 schema and bounded semantics.

    Validation checks report structure, duplicate IDs, and control characters.
    It does not authorize command execution or filesystem targets; callers own
    those policy decisions immediately before applying a returned next step.

    Raises:
        TypeError: If *report* is not a mapping.
        RuntimeError: If schema integrity or the native validator ABI fails.
        ValueError: If the report is not JSON-compatible or violates the contract.

    """
    if not isinstance(report, Mapping):
        raise TypeError("Install SOP report must be a mapping")

    schema_json, _ = _load_install_sop_schema_document()
    try:
        report_json = json.dumps(
            report,
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
        )
        _decode_unique_json(report_json)
    except _DuplicateJsonKeyError as exc:
        raise ValueError("Install SOP report is not JSON-compatible: duplicate_object_key") from exc
    except (TypeError, ValueError) as exc:
        raise ValueError("Install SOP report is not JSON-compatible") from exc

    try:
        from dcc_mcp_core import _core
    except ImportError as exc:
        raise RuntimeError(
            "Install SOP validator runtime error: native_module_unavailable; native dcc-mcp-core wheel required"
        ) from exc

    validator = getattr(_core, "_validate_install_sop_report_json", None)
    if not callable(validator):
        _raise_native_runtime_error("native_symbol_unavailable")

    try:
        errors = validator(schema_json, report_json)
    except Exception as exc:
        _raise_native_runtime_error("native_call_failed", exc)

    if type(errors) is not list:
        _raise_native_runtime_error("native_result_type")
    if any(type(error) is not str for error in errors):
        _raise_native_runtime_error("native_result_entry_type")
    diagnostic_sizes = [len(error.encode("utf-8")) for error in errors]
    if (
        any(size > _MAX_NATIVE_DIAGNOSTIC_BYTES for size in diagnostic_sizes)
        or sum(diagnostic_sizes) > _MAX_NATIVE_DIAGNOSTICS_BYTES
    ):
        _raise_native_runtime_error("native_result_bounds")
    if errors:
        details = "\n".join(f"- {error}" for error in errors)
        raise ValueError(f"Install SOP report failed schema validation:\n{details}")

    semantic_errors = _semantic_validation_errors(report)
    if semantic_errors:
        details = "\n".join(f"- {error}" for error in semantic_errors)
        raise ValueError(f"Install SOP report failed semantic validation:\n{details}")


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
