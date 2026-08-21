"""Content-addressed revision contract for cross-DCC asset synchronization.

The store deliberately exposes artefact URIs in manifests and keeps local
filesystem paths process-local.  Adapters may materialize a revision into a
host-owned input directory without leaking workstation paths to agents.
"""

from __future__ import annotations

from contextlib import contextmanager
from contextlib import suppress
from dataclasses import dataclass
from dataclasses import field
from datetime import datetime
from datetime import timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import tempfile
import time
from typing import Any
from typing import Dict
from typing import Iterator
from typing import Mapping
from typing import Optional
from typing import Union

from dcc_mcp_core.errors import DccMcpError

__all__ = [
    "AssetSyncConflictError",
    "AssetSyncRevision",
    "AssetSyncValidationError",
    "FileAssetSyncStore",
]


class AssetSyncValidationError(DccMcpError, ValueError):
    """Raised when a sync request or manifest violates the public contract."""


class AssetSyncConflictError(DccMcpError, RuntimeError):
    """Raised when optimistic revision preconditions do not match the head."""


def _utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _validate_identifier(value: str, field_name: str) -> str:
    value = str(value).strip()
    if not value or value in {".", ".."}:
        raise AssetSyncValidationError(f"{field_name} must not be empty")
    if len(value) > 128:
        raise AssetSyncValidationError(f"{field_name} exceeds 128 characters")
    if any(char in value for char in ("/", "\\", "\0")):
        raise AssetSyncValidationError(f"{field_name} must be one path-safe segment")
    return value


def _validate_relative_subfolder(value: str) -> Path:
    normalized = str(value).strip().replace("\\", "/")
    if not normalized:
        return Path()
    candidate = Path(normalized)
    if candidate.is_absolute() or any(part in {"", ".", ".."} for part in candidate.parts):
        raise AssetSyncValidationError("subfolder must be a safe relative path")
    return candidate


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _atomic_json_write(path: Path, payload: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=str(path.parent))
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            json.dump(payload, stream, ensure_ascii=False, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        Path(temporary).replace(path)
    except BaseException:
        with suppress(OSError):
            Path(temporary).unlink()
        raise


def _atomic_copy(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{destination.name}.", suffix=".tmp", dir=str(destination.parent))
    try:
        with source.open("rb") as reader, os.fdopen(fd, "wb") as writer:
            shutil.copyfileobj(reader, writer, length=1024 * 1024)
            writer.flush()
            os.fsync(writer.fileno())
        Path(temporary).replace(destination)
    except BaseException:
        with suppress(OSError):
            os.close(fd)
        with suppress(OSError):
            Path(temporary).unlink()
        raise


@contextmanager
def _exclusive_file_lock(path: Path, timeout_seconds: float = 10.0) -> Iterator[None]:
    """Hold a cross-process advisory lock on one stable lock file."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as stream:
        if stream.tell() == 0:
            stream.write(b"\0")
            stream.flush()
        deadline = time.monotonic() + timeout_seconds
        while True:
            try:
                stream.seek(0)
                if os.name == "nt":
                    import msvcrt

                    msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    import fcntl  # type: ignore[import-not-found]

                    fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except OSError as exc:
                if time.monotonic() >= deadline:
                    raise TimeoutError(f"timed out acquiring asset sync lock: {path}") from exc
                time.sleep(0.025)
        try:
            yield
        finally:
            stream.seek(0)
            if os.name == "nt":
                import msvcrt

                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl  # type: ignore[import-not-found]

                fcntl.flock(stream.fileno(), fcntl.LOCK_UN)


@dataclass(frozen=True)
class AssetSyncRevision:
    """Immutable head revision published by one DCC into a sync channel."""

    channel_id: str
    asset_id: str
    revision: int
    digest: str
    size_bytes: int
    format: str
    mime: str
    file_ref: Dict[str, Any]
    source_instance_id: Optional[str] = None
    base_revision: Optional[int] = None
    created_at: str = field(default_factory=_utc_now)
    spatial: Optional[Dict[str, Any]] = None
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return the path-free JSON wire representation."""
        result: Dict[str, Any] = {
            "channel_id": self.channel_id,
            "asset_id": self.asset_id,
            "revision": self.revision,
            "digest": self.digest,
            "size_bytes": self.size_bytes,
            "format": self.format,
            "mime": self.mime,
            "file_ref": dict(self.file_ref),
            "created_at": self.created_at,
            "metadata": dict(self.metadata),
        }
        if self.source_instance_id is not None:
            result["source_instance_id"] = self.source_instance_id
        if self.base_revision is not None:
            result["base_revision"] = self.base_revision
        if self.spatial is not None:
            result["spatial"] = dict(self.spatial)
        return result

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> AssetSyncRevision:
        """Reconstruct a revision from its wire representation."""
        spatial = data.get("spatial")
        return cls(
            channel_id=str(data["channel_id"]),
            asset_id=str(data["asset_id"]),
            revision=int(data["revision"]),
            base_revision=int(data["base_revision"]) if data.get("base_revision") is not None else None,
            digest=str(data["digest"]),
            size_bytes=int(data["size_bytes"]),
            format=str(data["format"]),
            mime=str(data["mime"]),
            file_ref=dict(data["file_ref"]),
            source_instance_id=data.get("source_instance_id"),
            created_at=str(data["created_at"]),
            spatial=dict(spatial) if spatial is not None else None,
            metadata=dict(data.get("metadata", {})),
        )


class FileAssetSyncStore:
    """Bounded local reference store for immutable asset revisions."""

    def __init__(self, root: Union[str, Path]) -> None:
        self._root = Path(root).resolve()

    def publish(
        self,
        source_path: Union[str, Path],
        *,
        channel_id: str,
        asset_id: str,
        format: str,
        mime: str,
        expected_head_revision: int,
        source_instance_id: Optional[str] = None,
        spatial: Optional[Mapping[str, Any]] = None,
        metadata: Optional[Mapping[str, Any]] = None,
    ) -> AssetSyncRevision:
        """Publish one immutable revision using optimistic head matching."""
        channel_id = _validate_identifier(channel_id, "channel_id")
        asset_id = _validate_identifier(asset_id, "asset_id")
        source = Path(source_path).resolve(strict=True)
        if not source.is_file():
            raise AssetSyncValidationError("source_path must reference a regular file")
        if expected_head_revision < 0:
            raise AssetSyncValidationError("expected_head_revision must be >= 0")
        normalized_format = str(format).strip().lower().lstrip(".")
        if not normalized_format:
            raise AssetSyncValidationError("format must not be empty")
        normalized_mime = str(mime).strip().lower()
        if not normalized_mime:
            raise AssetSyncValidationError("mime must not be empty")

        digest_hex = _sha256_file(source)
        digest = f"sha256:{digest_hex}"
        head_path = self._head_path(channel_id, asset_id)
        with _exclusive_file_lock(head_path.with_suffix(".lock")):
            head = self.read_head(channel_id, asset_id)
            observed_revision = head.revision if head is not None else 0
            if observed_revision != expected_head_revision:
                raise AssetSyncConflictError(
                    f"asset head is revision {observed_revision}, expected {expected_head_revision}"
                )
            if head is not None and head.digest == digest:
                return head

            object_path = self._object_path(digest_hex, normalized_format)
            if not object_path.exists() or _sha256_file(object_path) != digest_hex:
                _atomic_copy(source, object_path)

            revision = AssetSyncRevision(
                channel_id=channel_id,
                asset_id=asset_id,
                revision=observed_revision + 1,
                base_revision=observed_revision or None,
                digest=digest,
                size_bytes=object_path.stat().st_size,
                format=normalized_format,
                mime=normalized_mime,
                file_ref={
                    "uri": f"artefact://sha256/{digest_hex}",
                    "digest": digest,
                    "size_bytes": object_path.stat().st_size,
                    "mime": normalized_mime,
                },
                source_instance_id=source_instance_id,
                spatial=dict(spatial) if spatial is not None else None,
                metadata=dict(metadata or {}),
            )
            _atomic_json_write(head_path, revision.to_dict())
            return revision

    def read_head(self, channel_id: str, asset_id: str) -> Optional[AssetSyncRevision]:
        """Read the current head, returning ``None`` when it is unpublished."""
        head_path = self._head_path(
            _validate_identifier(channel_id, "channel_id"),
            _validate_identifier(asset_id, "asset_id"),
        )
        if not head_path.is_file():
            return None
        with head_path.open(encoding="utf-8") as stream:
            return AssetSyncRevision.from_dict(json.load(stream))

    def resolve(self, revision: AssetSyncRevision) -> Path:
        """Resolve a trusted local revision to its content-addressed object."""
        prefix = "sha256:"
        if not revision.digest.startswith(prefix):
            raise AssetSyncValidationError("revision digest must use sha256")
        path = self._object_path(revision.digest[len(prefix) :], revision.format)
        if not path.is_file():
            raise FileNotFoundError(str(path))
        if _sha256_file(path) != revision.digest[len(prefix) :]:
            raise AssetSyncValidationError("content-addressed object digest does not match revision")
        return path

    def materialize(
        self,
        revision: AssetSyncRevision,
        target_root: Union[str, Path],
        *,
        subfolder: str = "",
    ) -> Path:
        """Copy a revision beneath a consumer-owned root.

        The caller supplies the trusted root from operator configuration.  The
        public sync request may only choose a validated relative subfolder.
        """
        root = Path(target_root).resolve()
        relative_folder = _validate_relative_subfolder(subfolder)
        destination_dir = (root / relative_folder).resolve()
        try:
            destination_dir.relative_to(root)
        except ValueError as exc:
            raise AssetSyncValidationError("subfolder escapes target_root") from exc

        asset_id = _validate_identifier(revision.asset_id, "asset_id")
        normalized_format = _validate_identifier(revision.format, "format").lower().lstrip(".")
        digest_hex = revision.digest[len("sha256:") :] if revision.digest.startswith("sha256:") else ""
        if len(digest_hex) != 64 or any(char not in "0123456789abcdef" for char in digest_hex):
            raise AssetSyncValidationError("revision digest must be a lowercase sha256 digest")

        destination = destination_dir / (f"{asset_id}_r{revision.revision}_{digest_hex[:12]}.{normalized_format}")
        source = self.resolve(revision)
        if destination.is_file() and _sha256_file(destination) == digest_hex:
            return destination
        _atomic_copy(source, destination)
        return destination

    def _head_path(self, channel_id: str, asset_id: str) -> Path:
        return self._root / "channels" / channel_id / "heads" / f"{asset_id}.json"

    def _object_path(self, digest_hex: str, format: str) -> Path:
        return self._root / "objects" / "sha256" / f"{digest_hex}.{format}"
