"""Per-instance script materialization store (#1220).

The store writes ad-hoc script content to a host-local file before execution.
It keeps the path contract tied to the DCC type, live instance, and MCP
session so downstream execution, audit, sandbox allowlists, and cleanup can
reason about a concrete file instead of an opaque inline ``code`` string.
"""

from __future__ import annotations

import contextlib
from dataclasses import dataclass
from datetime import datetime
from datetime import timedelta
from datetime import timezone
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
from typing import Any
from typing import Mapping
import uuid

SCRIPT_MATERIALIZATION_ROOT_ENV = "DCC_MCP_SCRIPT_MATERIALIZATION_ROOT"
DEFAULT_SCRIPT_MATERIALIZATION_ROOT = Path.home() / ".dcc-mcp"

_SEGMENT_RE = re.compile(r"[^A-Za-z0-9_.-]+")
_SAFE_SUFFIX_RE = re.compile(r"[^A-Za-z0-9_.-]+")


@dataclass(frozen=True)
class MaterializedScript:
    """Structured descriptor returned by :func:`materialize_script`."""

    file_ref: dict[str, Any]
    file_path: str
    path: str
    language: str
    suffix: str
    sha256: str
    bytes: int
    created_at: str
    expires_at: str | None
    ttl_secs: int | None
    dcc_type: str
    instance_id: str
    session_id: str
    script_id: str
    tool_call_id: str | None = None
    correlation_id: str | None = None
    reused: bool = False

    @property
    def file_ref_uri(self) -> str:
        """Return the FileRef-compatible URI for this script."""
        return str(self.file_ref["uri"])

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-serializable descriptor."""
        data = {
            "file_ref": self.file_ref,
            "file_ref_uri": self.file_ref_uri,
            "file_path": self.file_path,
            "path": self.path,
            "language": self.language,
            "suffix": self.suffix,
            "sha256": self.sha256,
            "bytes": self.bytes,
            "created_at": self.created_at,
            "expires_at": self.expires_at,
            "ttl_secs": self.ttl_secs,
            "dcc_type": self.dcc_type,
            "instance_id": self.instance_id,
            "session_id": self.session_id,
            "script_id": self.script_id,
            "tool_call_id": self.tool_call_id,
            "correlation_id": self.correlation_id,
            "reused": self.reused,
        }
        return {key: value for key, value in data.items() if value is not None}


def default_script_materialization_root(root: str | os.PathLike[str] | None = None) -> Path:
    """Return the configured script materialization root.

    ``root`` wins first, then ``DCC_MCP_SCRIPT_MATERIALIZATION_ROOT``, then the
    documented default ``~/.dcc-mcp``.
    """
    if root is not None:
        return Path(root).expanduser()
    configured = os.environ.get(SCRIPT_MATERIALIZATION_ROOT_ENV)
    if configured:
        return Path(configured).expanduser()
    return DEFAULT_SCRIPT_MATERIALIZATION_ROOT


def sanitize_materialization_segment(value: str | None, *, default: str = "unknown") -> str:
    """Return a single filesystem-safe path segment.

    Separators, drive delimiters, traversal tokens, whitespace, and other
    special characters collapse to ``_``. Custom DCC names remain readable
    when they already use safe ASCII characters.
    """
    raw = "" if value is None else str(value).strip()
    raw = raw.replace("\\", "_").replace("/", "_").replace(":", "_")
    safe = _SEGMENT_RE.sub("_", raw).strip("._-")
    if not safe or safe in {".", ".."}:
        return default
    return safe[:96]


def _normalize_suffix(suffix: str | None) -> str:
    raw = ".py" if not suffix else str(suffix).strip()
    raw = raw.replace("\\", "_").replace("/", "_").replace(":", "_")
    raw = raw.lstrip(".")
    safe = "." + _SAFE_SUFFIX_RE.sub("_", raw)
    if safe in {".", ".."}:
        return ".txt"
    return safe[:32]


def _utc_now() -> datetime:
    return datetime.now(timezone.utc).replace(tzinfo=None)


def _format_utc(value: datetime) -> str:
    return value.replace(microsecond=0).isoformat() + "Z"


def _parse_utc(value: str | None) -> datetime | None:
    if not value:
        return None
    text = value[:-1] if value.endswith("Z") else value
    try:
        return datetime.strptime(text, "%Y-%m-%dT%H:%M:%S")
    except ValueError:
        return None


def _resolve_path(path: Path) -> Path:
    return path.resolve()


def _ensure_within_root(path: Path, root: Path) -> Path:
    resolved_root = _resolve_path(root)
    resolved_path = _resolve_path(path)
    try:
        resolved_path.relative_to(resolved_root)
    except ValueError as exc:
        raise ValueError(f"materialized script path escapes root: {path}") from exc
    return resolved_path


def _lexical_path_under_root(path: Path, root: Path) -> bool:
    parent = path.parent.resolve() if path.parent.exists() else path.parent
    candidate = parent / path.name
    try:
        candidate.relative_to(root)
    except ValueError:
        return False
    return True


def _atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(
        prefix=".tmp-",
        suffix=".part",
        dir=str(path.parent),
    )
    tmp = Path(tmp_name)
    try:
        with os.fdopen(fd, "wb") as fp:
            fp.write(data)
            fp.flush()
            os.fsync(fp.fileno())
        tmp.replace(path)
    except Exception:
        with contextlib.suppress(Exception):
            tmp.unlink()
        raise


def _metadata_path(script_path: Path) -> Path:
    return script_path.with_name(script_path.name + ".meta.json")


def _mime_for_language(language: str) -> str:
    normalized = language.lower()
    if normalized in {"python", "py"}:
        return "text/x-python"
    if normalized in {"mel"}:
        return "text/x-mel"
    if normalized in {"javascript", "js"}:
        return "text/javascript"
    if normalized in {"powershell", "ps1"}:
        return "text/x-powershell"
    return "text/plain"


def _file_ref_from_descriptor(descriptor: Mapping[str, Any]) -> dict[str, Any]:
    metadata = {
        "dcc_type": descriptor["dcc_type"],
        "instance_id": descriptor["instance_id"],
        "session_id": descriptor["session_id"],
        "script_id": descriptor["script_id"],
        "language": descriptor["language"],
        "suffix": descriptor["suffix"],
        "materialization_kind": "script",
    }
    return {
        "uri": Path(str(descriptor["file_path"])).as_uri(),
        "mime": _mime_for_language(str(descriptor["language"])),
        "size_bytes": descriptor["bytes"],
        "display_name": Path(str(descriptor["file_path"])).name,
        "digest": f"sha256:{descriptor['sha256']}",
        "tool_call_id": descriptor.get("tool_call_id"),
        "session_id": descriptor["session_id"],
        "correlation_id": descriptor.get("correlation_id"),
        "created_at": descriptor["created_at"],
        "expires_at": descriptor.get("expires_at"),
        "metadata": metadata,
    }


def _descriptor_from_dict(data: Mapping[str, Any], *, reused: bool) -> MaterializedScript:
    file_ref = data.get("file_ref")
    if not isinstance(file_ref, dict):
        file_ref = _file_ref_from_descriptor(data)
    return MaterializedScript(
        file_ref=dict(file_ref),
        file_path=str(data["file_path"]),
        path=str(data.get("path", data["file_path"])),
        language=str(data["language"]),
        suffix=str(data["suffix"]),
        sha256=str(data["sha256"]),
        bytes=int(data["bytes"]),
        created_at=str(data["created_at"]),
        expires_at=data.get("expires_at"),
        ttl_secs=data.get("ttl_secs"),
        dcc_type=str(data["dcc_type"]),
        instance_id=str(data["instance_id"]),
        session_id=str(data["session_id"]),
        script_id=str(data["script_id"]),
        tool_call_id=data.get("tool_call_id"),
        correlation_id=data.get("correlation_id"),
        reused=reused,
    )


def _script_id_for(
    *,
    sha256: str,
    reuse: bool,
    reuse_key: str | None,
    prefix: str,
) -> str:
    if reuse:
        if reuse_key:
            key = sanitize_materialization_segment(reuse_key, default="reuse")
            return f"{key}_{sha256[:12]}"
        return sha256
    safe_prefix = sanitize_materialization_segment(prefix, default="script")
    return f"{safe_prefix}_{uuid.uuid4().hex}_{sha256[:12]}"


def materialize_script(
    content: str,
    *,
    dcc_type: str,
    instance_id: str,
    session_id: str,
    language: str = "python",
    suffix: str = ".py",
    display_name: str | None = None,
    reuse: bool = False,
    reuse_key: str | None = None,
    ttl_secs: int | None = None,
    root: str | os.PathLike[str] | None = None,
    tool_call_id: str | None = None,
    correlation_id: str | None = None,
    prefix: str = "script",
) -> MaterializedScript:
    """Materialize script content under a DCC/session-scoped host path."""
    if not isinstance(content, str):
        raise TypeError("content must be a string")
    if ttl_secs is not None and (isinstance(ttl_secs, bool) or ttl_secs <= 0):
        raise ValueError("ttl_secs must be a positive integer")

    body = content.encode("utf-8")
    sha256 = hashlib.sha256(body).hexdigest()
    suffix = _normalize_suffix(suffix)
    language = sanitize_materialization_segment(language, default="text").lower()
    safe_dcc = sanitize_materialization_segment(dcc_type)
    safe_instance = sanitize_materialization_segment(instance_id)
    safe_session = sanitize_materialization_segment(session_id)
    script_id = _script_id_for(
        sha256=sha256,
        reuse=reuse,
        reuse_key=reuse_key,
        prefix=display_name or prefix,
    )

    store_root = default_script_materialization_root(root)
    target_dir = store_root / safe_dcc / "temp" / safe_instance / safe_session
    target_path = target_dir / f"{script_id}{suffix}"
    target_dir.mkdir(parents=True, exist_ok=True)
    resolved_target = _ensure_within_root(target_path, store_root)
    metadata_path = _metadata_path(resolved_target)

    if reuse and resolved_target.is_file() and metadata_path.is_file():
        try:
            existing = json.loads(metadata_path.read_text(encoding="utf-8"))
        except (OSError, ValueError, TypeError):
            existing = None
        if isinstance(existing, dict) and existing.get("sha256") == sha256:
            expires_at = _parse_utc(existing.get("expires_at"))
            if expires_at is None or expires_at > _utc_now():
                return _descriptor_from_dict(existing, reused=True)

    now = _utc_now()
    expires_at = _format_utc(now + timedelta(seconds=ttl_secs)) if ttl_secs else None
    descriptor = {
        "file_path": str(resolved_target),
        "path": str(resolved_target),
        "language": language,
        "suffix": suffix,
        "sha256": sha256,
        "bytes": len(body),
        "created_at": _format_utc(now),
        "expires_at": expires_at,
        "ttl_secs": ttl_secs,
        "dcc_type": safe_dcc,
        "instance_id": safe_instance,
        "session_id": safe_session,
        "script_id": script_id,
        "tool_call_id": tool_call_id,
        "correlation_id": correlation_id,
        "reused": False,
    }
    descriptor["file_ref"] = _file_ref_from_descriptor(descriptor)

    _atomic_write(resolved_target, body)
    _atomic_write(
        metadata_path,
        json.dumps(descriptor, sort_keys=True, separators=(",", ":")).encode("utf-8"),
    )
    return _descriptor_from_dict(descriptor, reused=False)


def cleanup_materialized_scripts(
    *,
    root: str | os.PathLike[str] | None = None,
    now: datetime | None = None,
    include_unexpired: bool = False,
) -> int:
    """Remove expired script files and sidecars below ``root``.

    The cleanup pass only unlinks files whose metadata sidecar lives below the
    configured materialization root. It does not recursively remove arbitrary
    directories and it does not follow symlink targets.
    """
    store_root = default_script_materialization_root(root)
    if not store_root.exists():
        return 0

    resolved_root = _resolve_path(store_root)
    cutoff = now or _utc_now()
    removed = 0
    for sidecar in resolved_root.rglob("*.meta.json"):
        with contextlib.suppress(ValueError):
            sidecar.relative_to(resolved_root)
        try:
            data = json.loads(sidecar.read_text(encoding="utf-8"))
        except (OSError, ValueError, TypeError):
            continue
        expires_at = _parse_utc(data.get("expires_at") if isinstance(data, dict) else None)
        if not include_unexpired and (expires_at is None or expires_at > cutoff):
            continue
        script_path = Path(str(data.get("file_path", ""))) if isinstance(data, dict) else Path()
        targets = [script_path, sidecar]
        for target in targets:
            with contextlib.suppress(Exception):
                if _lexical_path_under_root(target, resolved_root) and (target.is_file() or target.is_symlink()):
                    target.unlink()
                    removed += 1
    return removed


__all__ = [
    "DEFAULT_SCRIPT_MATERIALIZATION_ROOT",
    "SCRIPT_MATERIALIZATION_ROOT_ENV",
    "MaterializedScript",
    "cleanup_materialized_scripts",
    "default_script_materialization_root",
    "materialize_script",
    "sanitize_materialization_segment",
]
