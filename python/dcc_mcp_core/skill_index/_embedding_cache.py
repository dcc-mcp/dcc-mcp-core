"""Durable content-addressed cache for skill document embeddings."""

from __future__ import annotations

from array import array
import hashlib
import logging
import os
from pathlib import Path
import threading
from typing import Any
import uuid

from dcc_mcp_core import json_dumps
from dcc_mcp_core import json_loads
from dcc_mcp_core.constants import ENV_EMBED_CACHE_PATH

logger = logging.getLogger(__name__)

_CACHE_VERSION = 1
_FINGERPRINT_VERSION = "skill-embedding-v1"


def default_embedding_cache_path() -> Path:
    """Return the operator-overridable cache path shared by process starts."""
    configured = os.environ.get(ENV_EMBED_CACHE_PATH)
    if configured:
        return Path(configured).expanduser()
    return Path.home() / ".dcc-mcp" / "cache" / "skill-embeddings.json"


def embedder_fingerprint(embedder: Any) -> str:
    """Return a stable identity for vector compatibility and invalidation."""
    cls = type(embedder)
    attributes = {}
    for name in ("dim", "model_name", "backend_name", "char_n", "token_weight", "char_weight"):
        value = getattr(embedder, name, None)
        if isinstance(value, (str, int, float, bool)) or value is None:
            attributes[name] = value
    identity = f"{_FINGERPRINT_VERSION}:{cls.__module__}.{cls.__qualname__}"
    stable_attributes = "\0".join(f"{name}={attributes[name]!r}" for name in sorted(attributes))
    return hashlib.sha256(f"{identity}\0{stable_attributes}".encode()).hexdigest()


class EmbeddingCache:
    """Small JSON cache keyed by embedder identity and document content."""

    def __init__(self, path: str | Path) -> None:
        self._path = Path(path).expanduser()
        self._lock = threading.RLock()
        self._entries = self._read_entries()
        self._dirty = False

    @staticmethod
    def key(fingerprint: str, content: str) -> str:
        payload = f"{fingerprint}\0{content}".encode()
        return hashlib.sha256(payload).hexdigest()

    def get(self, fingerprint: str, content: str, *, dim: int) -> array[float] | None:
        with self._lock:
            entry = self._entries.get(self.key(fingerprint, content))
            if not isinstance(entry, dict) or entry.get("fingerprint") != fingerprint:
                return None
            values = entry.get("vector")
            if not isinstance(values, list) or len(values) != dim:
                return None
            try:
                return array("d", (float(value) for value in values))
            except (TypeError, ValueError, OverflowError):
                return None

    def put(self, fingerprint: str, content: str, vector: array[float]) -> None:
        with self._lock:
            self._entries[self.key(fingerprint, content)] = {
                "fingerprint": fingerprint,
                "vector": list(vector),
            }
            self._dirty = True

    def flush(self) -> None:
        with self._lock:
            if not self._dirty:
                return
            temporary: Path | None = None
            try:
                self._path.parent.mkdir(parents=True, exist_ok=True)
                current = self._read_entries()
                current.update(self._entries)
                payload = {"version": _CACHE_VERSION, "entries": current}
                temporary = self._path.with_name(f".{self._path.name}.{uuid.uuid4().hex}.tmp")
                temporary.write_text(json_dumps(payload), encoding="utf-8")
                temporary.replace(self._path)
                self._entries = current
                self._dirty = False
            except OSError as exc:
                logger.warning("Could not persist embedding cache %s: %s", self._path, exc)
            finally:
                if temporary is not None:
                    try:
                        if temporary.exists():
                            temporary.unlink()
                    except OSError:
                        pass

    def _read_entries(self) -> dict[str, dict[str, Any]]:
        if not self._path.is_file():
            return {}
        try:
            payload = json_loads(self._path.read_text(encoding="utf-8"))
            if not isinstance(payload, dict) or payload.get("version") != _CACHE_VERSION:
                return {}
            entries = payload.get("entries")
            return dict(entries) if isinstance(entries, dict) else {}
        except (OSError, TypeError, ValueError) as exc:
            logger.warning("Could not load embedding cache %s: %s", self._path, exc)
            return {}


__all__ = ["EmbeddingCache", "default_embedding_cache_path", "embedder_fingerprint"]
