"""Stable Rust-backed helper namespace for DCC-MCP skill scripts.

Skill authors should import dependency-light helpers from this module instead
of reaching across implementation modules or adding small third-party runtime
dependencies for JSON, YAML, validation, result envelopes, or cancellation.

The module intentionally keeps imports lazy: importing
``dcc_mcp_core.skills_helper`` does not load the PyO3 extension until a
Rust-backed helper is used.
"""

from __future__ import annotations

from collections.abc import Mapping
import importlib
from pathlib import Path
from typing import Any


class SkillHelperError(Exception):
    """Base exception for skill-helper failures raised by future helpers."""


class SkillCodecError(SkillHelperError):
    """Raised when a skill helper cannot load or dump structured data."""

    def __init__(self, message: str, *, codec: str, source: str | None = None) -> None:
        self.codec = codec
        self.source = source
        detail = f"{codec}: {message}"
        if source:
            detail = f"{source}: {detail}"
        super().__init__(detail)


class SkillFileError(SkillHelperError):
    """Raised when a skill helper cannot safely read, write, hash, or compress data."""

    def __init__(self, message: str, *, operation: str, source: str | None = None) -> None:
        self.operation = operation
        self.source = source
        detail = f"{operation}: {message}"
        if source:
            detail = f"{source}: {detail}"
        super().__init__(detail)


DEFAULT_FILE_MAX_BYTES = 64 * 1024 * 1024
DEFAULT_PAYLOAD_MAX_BYTES = 64 * 1024 * 1024


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


def _source_name(path: str | Path) -> str:
    return str(path)


def _coerce_bytes(data: bytes | bytearray | memoryview, *, operation: str) -> bytes:
    try:
        return bytes(data)
    except TypeError as exc:
        raise SkillFileError(str(exc), operation=operation) from exc


def _ensure_supported_algorithm(actual: str, expected: str, *, operation: str) -> None:
    if actual != expected:
        raise SkillFileError(
            f"unsupported algorithm {actual!r}; supported algorithm is {expected!r}",
            operation=operation,
        )


def _require_mapping_root(value: Any, *, codec: str, source: str | None, require_mapping: bool) -> Any:
    if require_mapping and not isinstance(value, Mapping):
        raise SkillCodecError(
            f"expected a mapping root, got {type(value).__name__}",
            codec=codec,
            source=source,
        )
    return value


def load_text(path: str | Path, *, max_bytes: int | None = None) -> str:
    """Read a UTF-8 text file with an optional byte-size guard."""
    p = Path(path)
    try:
        data = p.read_bytes()
    except OSError as exc:
        raise SkillCodecError(str(exc), codec="text", source=_source_name(p)) from exc
    if max_bytes is not None and len(data) > max_bytes:
        raise SkillCodecError(
            f"file is {len(data)} bytes, exceeding max_bytes={max_bytes}",
            codec="text",
            source=_source_name(p),
        )
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise SkillCodecError(str(exc), codec="text", source=_source_name(p)) from exc


def dump_text(path: str | Path, text: str, *, create_parents: bool = True) -> Path:
    """Write UTF-8 text and return the written path."""
    p = Path(path)
    if create_parents:
        p.parent.mkdir(parents=True, exist_ok=True)
    try:
        p.write_text(text, encoding="utf-8")
    except OSError as exc:
        raise SkillCodecError(str(exc), codec="text", source=_source_name(p)) from exc
    return p


def load_json_text(text: str, *, source: str | None = None, require_mapping: bool = False) -> Any:
    """Load JSON text and wrap parse/root-shape errors with source context."""
    try:
        value = json_loads(text)
    except Exception as exc:
        raise SkillCodecError(str(exc), codec="json", source=source) from exc
    return _require_mapping_root(value, codec="json", source=source, require_mapping=require_mapping)


def dump_json_text(obj: Any, *, ensure_ascii: bool = True, indent: int | None = None) -> str:
    """Dump JSON text with the Rust-backed codec."""
    try:
        return json_dumps(obj, ensure_ascii=ensure_ascii, indent=indent)
    except Exception as exc:
        raise SkillCodecError(str(exc), codec="json") from exc


def load_json_file(path: str | Path, *, require_mapping: bool = False, max_bytes: int | None = None) -> Any:
    """Load a UTF-8 JSON file with source-aware parse errors."""
    p = Path(path)
    return load_json_text(
        load_text(p, max_bytes=max_bytes),
        source=_source_name(p),
        require_mapping=require_mapping,
    )


def dump_json_file(
    path: str | Path,
    obj: Any,
    *,
    ensure_ascii: bool = True,
    indent: int | None = 2,
    create_parents: bool = True,
) -> Path:
    """Serialize JSON with the Rust-backed codec and write it as UTF-8."""
    return dump_text(
        path,
        dump_json_text(obj, ensure_ascii=ensure_ascii, indent=indent),
        create_parents=create_parents,
    )


def load_yaml_text(text: str, *, source: str | None = None, require_mapping: bool = False) -> Any:
    """Load YAML text and wrap parse/root-shape errors with source context."""
    try:
        value = yaml_loads(text)
    except Exception as exc:
        raise SkillCodecError(str(exc), codec="yaml", source=source) from exc
    return _require_mapping_root(value, codec="yaml", source=source, require_mapping=require_mapping)


def dump_yaml_text(obj: Any) -> str:
    """Dump YAML text with the Rust-backed codec."""
    try:
        return yaml_dumps(obj)
    except Exception as exc:
        raise SkillCodecError(str(exc), codec="yaml") from exc


def load_yaml_file(path: str | Path, *, require_mapping: bool = False, max_bytes: int | None = None) -> Any:
    """Load a UTF-8 YAML file with source-aware parse errors."""
    p = Path(path)
    return load_yaml_text(
        load_text(p, max_bytes=max_bytes),
        source=_source_name(p),
        require_mapping=require_mapping,
    )


def dump_yaml_file(path: str | Path, obj: Any, *, create_parents: bool = True) -> Path:
    """Serialize YAML with the Rust-backed codec and write it as UTF-8."""
    return dump_text(path, dump_yaml_text(obj), create_parents=create_parents)


def ensure_within_root(root: str | Path, path: str | Path, *, must_exist: bool = False) -> Path:
    """Resolve *path* under *root* and reject traversal outside the root."""
    try:
        resolved = _core_symbol("ensure_within_root")(str(root), str(path), must_exist=must_exist)
    except Exception as exc:
        raise SkillFileError(str(exc), operation="ensure_within_root", source=_source_name(path)) from exc
    return Path(resolved)


def atomic_write_bytes(
    path: str | Path,
    data: bytes | bytearray | memoryview,
    *,
    root: str | Path | None = None,
    create_parents: bool = True,
    max_bytes: int | None = DEFAULT_FILE_MAX_BYTES,
) -> Path:
    """Atomically write bytes and return the written path."""
    p = ensure_within_root(root, path) if root is not None else Path(path)
    payload = _coerce_bytes(data, operation="atomic_write_bytes")
    try:
        written = _core_symbol("atomic_write_bytes")(
            str(p),
            payload,
            create_parents=create_parents,
            max_bytes=max_bytes,
        )
    except Exception as exc:
        raise SkillFileError(str(exc), operation="atomic_write_bytes", source=_source_name(p)) from exc
    return Path(written)


def atomic_write_text(
    path: str | Path,
    text: str,
    *,
    root: str | Path | None = None,
    create_parents: bool = True,
    max_bytes: int | None = DEFAULT_FILE_MAX_BYTES,
) -> Path:
    """Atomically write UTF-8 text and return the written path."""
    p = ensure_within_root(root, path) if root is not None else Path(path)
    try:
        written = _core_symbol("atomic_write_text")(
            str(p),
            text,
            create_parents=create_parents,
            max_bytes=max_bytes,
        )
    except Exception as exc:
        raise SkillFileError(str(exc), operation="atomic_write_text", source=_source_name(p)) from exc
    return Path(written)


def bytes_digest(
    data: bytes | bytearray | memoryview,
    *,
    algorithm: str = "sha256",
    max_bytes: int | None = DEFAULT_FILE_MAX_BYTES,
) -> str:
    """Hash bytes with the Rust-backed SHA-256 helper."""
    _ensure_supported_algorithm(algorithm, "sha256", operation="bytes_digest")
    payload = _coerce_bytes(data, operation="bytes_digest")
    try:
        return _core_symbol("bytes_digest_sha256")(payload, max_bytes=max_bytes)
    except Exception as exc:
        raise SkillFileError(str(exc), operation="bytes_digest") from exc


def file_digest(
    path: str | Path,
    *,
    algorithm: str = "sha256",
    root: str | Path | None = None,
    max_bytes: int | None = DEFAULT_FILE_MAX_BYTES,
) -> str:
    """Stream-hash a file with the Rust-backed SHA-256 helper."""
    _ensure_supported_algorithm(algorithm, "sha256", operation="file_digest")
    p = ensure_within_root(root, path, must_exist=True) if root is not None else Path(path)
    try:
        return _core_symbol("file_digest_sha256")(str(p), max_bytes=max_bytes)
    except Exception as exc:
        raise SkillFileError(str(exc), operation="file_digest", source=_source_name(p)) from exc


def compress_bytes(
    data: bytes | bytearray | memoryview,
    *,
    algorithm: str = "lz4",
    max_bytes: int | None = DEFAULT_PAYLOAD_MAX_BYTES,
) -> bytes:
    """Compress bytes with Rust-backed LZ4 frame encoding."""
    _ensure_supported_algorithm(algorithm, "lz4", operation="compress_bytes")
    payload = _coerce_bytes(data, operation="compress_bytes")
    try:
        return _core_symbol("lz4_compress")(payload, max_bytes=max_bytes)
    except Exception as exc:
        raise SkillFileError(str(exc), operation="compress_bytes") from exc


def decompress_bytes(
    data: bytes | bytearray | memoryview,
    *,
    algorithm: str = "lz4",
    max_bytes: int | None = DEFAULT_PAYLOAD_MAX_BYTES,
) -> bytes:
    """Decompress LZ4 frame bytes with a Rust-backed output-size guard."""
    _ensure_supported_algorithm(algorithm, "lz4", operation="decompress_bytes")
    payload = _coerce_bytes(data, operation="decompress_bytes")
    try:
        return _core_symbol("lz4_decompress")(payload, max_bytes=max_bytes)
    except Exception as exc:
        raise SkillFileError(str(exc), operation="decompress_bytes") from exc


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
    "DEFAULT_FILE_MAX_BYTES",
    "DEFAULT_PAYLOAD_MAX_BYTES",
    "SkillFileError",
    "SkillCodecError",
    "SkillHelperError",
    "atomic_write_bytes",
    "atomic_write_text",
    "bytes_digest",
    "compress_bytes",
    "decompress_bytes",
    "dump_json_file",
    "dump_json_text",
    "dump_text",
    "dump_yaml_file",
    "dump_yaml_text",
    "ensure_within_root",
    "file_digest",
    "json_dumps",
    "json_loads",
    "load_json_file",
    "load_json_text",
    "load_text",
    "load_yaml_file",
    "load_yaml_text",
    "skill_error_from_exception",
    "yaml_dumps",
    "yaml_loads",
]


__all__ = sorted([*_DIRECT_EXPORTS, *_LAZY_EXPORTS])
