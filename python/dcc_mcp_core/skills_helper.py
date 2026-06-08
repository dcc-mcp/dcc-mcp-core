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
from dataclasses import dataclass
import json as _json
from pathlib import Path
from typing import Any

from dcc_mcp_core._lazy import lazy_dir
from dcc_mcp_core._lazy import resolve_lazy_symbol


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


class SkillHttpError(SkillHelperError):
    """Raised when a Rust-backed HTTP helper cannot complete a request."""

    def __init__(
        self,
        message: str,
        *,
        kind: str,
        url: str | None = None,
        status: int | None = None,
        headers: Mapping[str, str] | None = None,
    ) -> None:
        self.kind = kind
        self.url = url
        self.status = status
        self.headers = dict(headers or {})
        detail = f"{kind}: {message}"
        if status is not None:
            detail = f"HTTP {status}: {detail}"
        if url:
            detail = f"{url}: {detail}"
        super().__init__(detail)

    def to_skill_error(self, *, message: str | None = None, **context: Any) -> dict[str, Any]:
        """Convert the HTTP failure into the standard skill error envelope."""
        payload = {
            "kind": self.kind,
            "url": self.url,
            "status": self.status,
            **context,
        }
        return skill_error_from_exception(self, message=message, error=self.kind, **payload)


class HttpStatusError(SkillHttpError):
    """Raised when an HTTP response status is outside the 2xx range."""

    def __init__(self, response: HttpResponse) -> None:
        self.response = response
        super().__init__(
            f"HTTP request returned status {response.status}",
            kind="http-status",
            url=response.url,
            status=response.status,
            headers=redact_http_headers(response.headers),
        )


@dataclass(frozen=True)
class HttpResponse:
    """Structured response returned by :func:`http_request`."""

    status: int
    headers: Mapping[str, str]
    _body: bytes
    url: str
    elapsed_ms: int
    truncated: bool = False

    @property
    def bytes(self) -> bytes:
        """Response body bytes, bounded by the request's ``max_bytes``."""
        return self._body

    @property
    def text(self) -> str:
        """Response body decoded as UTF-8 with replacement for invalid bytes."""
        return self._body.decode("utf-8", errors="replace")

    def json(self) -> Any:
        """Parse the response body using the Rust-backed JSON codec."""
        if self.truncated:
            raise SkillHttpError(
                f"response exceeded max_bytes and was truncated at {len(self._body)} bytes",
                kind="response-truncated",
                url=self.url,
                status=self.status,
                headers=redact_http_headers(self.headers),
            )
        return load_json_text(self.text, source=f"{self.url} response")

    def raise_for_status(self) -> None:
        """Raise :class:`HttpStatusError` when the response is not 2xx."""
        if self.status < 200 or self.status >= 300:
            raise HttpStatusError(self)


DEFAULT_FILE_MAX_BYTES = 64 * 1024 * 1024
DEFAULT_PAYLOAD_MAX_BYTES = 64 * 1024 * 1024
DEFAULT_HTTP_TIMEOUT_MS = 5_000
DEFAULT_HTTP_MAX_BYTES = 1024 * 1024
REDACTED_HTTP_HEADER_VALUE = "[REDACTED]"
SENSITIVE_HTTP_HEADERS = {
    "authorization",
    "cookie",
    "proxy-authorization",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
}


def _core_symbol(name: str) -> Any:
    from dcc_mcp_core import _core

    return getattr(_core, name)


def _optional_core_symbol(name: str) -> Any:
    try:
        return _core_symbol(name)
    except ModuleNotFoundError as exc:
        if exc.name == "dcc_mcp_core._core":
            return None
        raise


def json_dumps(obj: Any, *, ensure_ascii: bool = True, indent: int | None = None) -> str:
    """Serialize *obj* to JSON, preferring the Rust-backed codec when available."""
    dumps = _optional_core_symbol("json_dumps")
    if dumps is not None:
        return dumps(obj, ensure_ascii=ensure_ascii, indent=indent)
    return _json.dumps(obj, ensure_ascii=ensure_ascii, indent=indent)


def json_loads(s: str) -> Any:
    """Deserialize JSON text, preferring the Rust-backed codec when available."""
    loads = _optional_core_symbol("json_loads")
    if loads is not None:
        return loads(s)
    return _json.loads(s)


def yaml_dumps(obj: Any) -> str:
    """Serialize *obj* to YAML using the Rust-backed codec."""
    return _core_symbol("yaml_dumps")(obj)


def yaml_loads(s: str) -> Any:
    """Deserialize YAML text using the Rust-backed codec."""
    return _core_symbol("yaml_loads")(s)


def redact_http_headers(headers: Mapping[str, Any] | None) -> dict[str, str]:
    """Return a copy of headers with common credential-bearing values redacted."""
    redacted: dict[str, str] = {}
    for name, value in (headers or {}).items():
        key = str(name)
        if key.lower() in SENSITIVE_HTTP_HEADERS:
            redacted[key] = REDACTED_HTTP_HEADER_VALUE
        else:
            redacted[key] = str(value)
    return redacted


def http_request(
    method: str,
    url: str,
    *,
    headers: Mapping[str, Any] | None = None,
    query: Mapping[str, Any] | list[tuple[Any, Any]] | tuple[tuple[Any, Any], ...] | None = None,
    json: Any = None,
    body: str | bytes | bytearray | memoryview | None = None,
    timeout_ms: int = DEFAULT_HTTP_TIMEOUT_MS,
    max_bytes: int = DEFAULT_HTTP_MAX_BYTES,
    raise_for_status: bool = False,
) -> HttpResponse:
    """Perform a bounded synchronous HTTP request through the Rust helper."""
    if json is not None and body is not None:
        raise SkillHttpError(
            "json and body are mutually exclusive",
            kind="invalid-body",
            url=url,
            headers=redact_http_headers(headers),
        )
    if timeout_ms <= 0:
        raise SkillHttpError("timeout_ms must be greater than zero", kind="invalid-timeout", url=url)
    if max_bytes < 0:
        raise SkillHttpError("max_bytes must be >= 0", kind="invalid-max-bytes", url=url)

    json_body = dump_json_text(json, ensure_ascii=False) if json is not None else None
    raw_body = _coerce_http_body(body)
    raw = _core_symbol("skill_http_request")(
        method,
        url,
        headers=_header_pairs(headers),
        query=_query_pairs(query),
        json_body=json_body,
        body=raw_body,
        timeout_ms=int(timeout_ms),
        max_bytes=int(max_bytes),
    )
    if not raw.get("ok"):
        raise SkillHttpError(
            str(raw.get("message") or raw.get("error_kind") or "HTTP request failed"),
            kind=str(raw.get("error_kind") or "request"),
            url=str(raw.get("url") or url),
            headers=redact_http_headers(headers),
        )
    response = HttpResponse(
        status=int(raw["status"]),
        headers=dict(raw.get("headers") or {}),
        _body=bytes(raw.get("body") or b""),
        url=str(raw.get("url") or url),
        elapsed_ms=int(raw.get("elapsed_ms") or 0),
        truncated=bool(raw.get("truncated")),
    )
    if raise_for_status:
        response.raise_for_status()
    return response


def http_get_json(
    url: str,
    *,
    headers: Mapping[str, Any] | None = None,
    query: Mapping[str, Any] | list[tuple[Any, Any]] | tuple[tuple[Any, Any], ...] | None = None,
    timeout_ms: int = DEFAULT_HTTP_TIMEOUT_MS,
    max_bytes: int = DEFAULT_HTTP_MAX_BYTES,
) -> Any:
    """GET *url*, require 2xx, and parse JSON with the Rust-backed codec."""
    response = http_request(
        "GET",
        url,
        headers=headers,
        query=query,
        timeout_ms=timeout_ms,
        max_bytes=max_bytes,
        raise_for_status=True,
    )
    return response.json()


_MISSING = object()


def http_post_json(
    url: str,
    payload: Any = _MISSING,
    *,
    json: Any = _MISSING,
    headers: Mapping[str, Any] | None = None,
    query: Mapping[str, Any] | list[tuple[Any, Any]] | tuple[tuple[Any, Any], ...] | None = None,
    timeout_ms: int = DEFAULT_HTTP_TIMEOUT_MS,
    max_bytes: int = DEFAULT_HTTP_MAX_BYTES,
) -> Any:
    """POST JSON to *url*, require 2xx, and parse the JSON response."""
    if payload is not _MISSING and json is not _MISSING:
        raise SkillHttpError("payload and json are aliases; pass only one", kind="invalid-body", url=url)
    request_json = None if payload is _MISSING and json is _MISSING else (json if json is not _MISSING else payload)
    response = http_request(
        "POST",
        url,
        headers=headers,
        query=query,
        json=request_json,
        timeout_ms=timeout_ms,
        max_bytes=max_bytes,
        raise_for_status=True,
    )
    return response.json()


def _header_pairs(headers: Mapping[str, Any] | None) -> list[tuple[str, str]] | None:
    if headers is None:
        return None
    return [(str(name), str(value)) for name, value in headers.items()]


def _query_pairs(
    query: Mapping[str, Any] | list[tuple[Any, Any]] | tuple[tuple[Any, Any], ...] | None,
) -> list[tuple[str, str]] | None:
    if query is None:
        return None
    items = query.items() if isinstance(query, Mapping) else query
    pairs: list[tuple[str, str]] = []
    for name, value in items:
        if value is None:
            continue
        if isinstance(value, (list, tuple, set)):
            for item in value:
                if item is not None:
                    pairs.append((str(name), str(item)))
        else:
            pairs.append((str(name), str(value)))
    return pairs


def _coerce_http_body(body: str | bytes | bytearray | memoryview | None) -> bytes | None:
    if body is None:
        return None
    if isinstance(body, str):
        return body.encode("utf-8")
    try:
        return bytes(body)
    except TypeError as exc:
        raise SkillHttpError(str(exc), kind="invalid-body") from exc


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
    return resolve_lazy_symbol(name, _LAZY_EXPORTS, module_name=__name__)


def __dir__() -> list[str]:
    return sorted([*_DIRECT_EXPORTS, *lazy_dir(_LAZY_EXPORTS)])


_DIRECT_EXPORTS = [
    "DEFAULT_HTTP_MAX_BYTES",
    "DEFAULT_HTTP_TIMEOUT_MS",
    "DEFAULT_FILE_MAX_BYTES",
    "DEFAULT_PAYLOAD_MAX_BYTES",
    "HttpResponse",
    "HttpStatusError",
    "SkillFileError",
    "SkillCodecError",
    "SkillHelperError",
    "SkillHttpError",
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
    "http_get_json",
    "http_post_json",
    "http_request",
    "json_dumps",
    "json_loads",
    "load_json_file",
    "load_json_text",
    "load_text",
    "load_yaml_file",
    "load_yaml_text",
    "redact_http_headers",
    "REDACTED_HTTP_HEADER_VALUE",
    "skill_error_from_exception",
    "SENSITIVE_HTTP_HEADERS",
    "yaml_dumps",
    "yaml_loads",
]


__all__ = sorted([*_DIRECT_EXPORTS, *_LAZY_EXPORTS])
