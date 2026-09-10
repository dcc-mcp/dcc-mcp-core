"""Vendor-neutral, non-blocking provider boundary for durable agent memory.

Providers store and retrieve inert context records only.  They are deliberately
not given a server, dispatcher, tool registry, or callback that could execute
actions.  DCC adapters should call providers through :class:`MemoryCoordinator`
so filesystem, database, or network work never runs on the host thread.
"""

from __future__ import annotations

from concurrent.futures import Future
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
from dataclasses import dataclass
from enum import Enum
import json
from pathlib import Path
import sqlite3
from threading import RLock
from threading import Timer
import time
from typing import Any
from typing import Callable
from typing import Mapping

from dcc_mcp_core._typing import Protocol

MEMORY_PROVIDER_ENTRY_POINT_GROUP = "dcc_mcp.memory_providers"


def _require_json_safe(value: Any, field_name: str) -> None:
    try:
        json.dumps(value, allow_nan=False)
    except (TypeError, ValueError) as exc:
        raise TypeError(f"{field_name} must be JSON-safe") from exc


@dataclass(frozen=True)
class MemoryRecord:
    """Portable, inert memory record shared with third-party providers."""

    record_id: str
    scope: str
    kind: str
    content: str
    source: str
    metadata: Mapping[str, Any]
    created_unix_secs: float | None = None
    expires_unix_secs: float | None = None

    def __post_init__(self) -> None:
        for name in ("record_id", "scope", "kind", "content", "source"):
            if not isinstance(getattr(self, name), str) or not getattr(self, name).strip():
                raise ValueError(f"{name} must be a non-empty string")
        _require_json_safe(self.metadata, "metadata")
        object.__setattr__(self, "metadata", dict(self.metadata))

    def to_dict(self) -> dict[str, Any]:
        value = asdict(self)
        _require_json_safe(value, "record")
        return value


@dataclass(frozen=True)
class MemoryRecallRequest:
    """Bounded provider recall query."""

    query: str
    scope: str | None = None
    kinds: tuple[str, ...] = ()
    limit: int = 16

    def __post_init__(self) -> None:
        if not isinstance(self.query, str):
            raise TypeError("query must be a string")
        if self.limit < 0:
            raise ValueError("limit must be non-negative")


@dataclass(frozen=True)
class MemoryRememberRequest:
    """Records approved by the caller's persistence policy."""

    records: tuple[MemoryRecord, ...]


@dataclass(frozen=True)
class MemoryForgetRequest:
    """Narrow deletion request; an empty request deletes nothing."""

    record_ids: tuple[str, ...] = ()
    scope: str | None = None


@dataclass(frozen=True)
class MemoryWriteReceipt:
    """Counts returned by a provider write."""

    accepted: int
    rejected: int = 0


@dataclass(frozen=True)
class MemoryDeleteReceipt:
    """Count returned by a provider deletion."""

    deleted: int


class MemoryProviderStatus(str, Enum):
    """Low-cardinality provider readiness state."""

    READY = "ready"
    DEGRADED = "degraded"
    DISABLED = "disabled"


@dataclass(frozen=True)
class MemoryHealth:
    """Provider health without credentials or high-cardinality data."""

    status: MemoryProviderStatus
    provider: str
    detail: str | None = None


class MemoryProvider(Protocol):
    """Provider SPI. Implementations must return data, never executable work."""

    def recall(self, request: MemoryRecallRequest) -> tuple[MemoryRecord, ...]: ...
    def remember(self, request: MemoryRememberRequest) -> MemoryWriteReceipt: ...
    def forget(self, request: MemoryForgetRequest) -> MemoryDeleteReceipt: ...
    def health(self) -> MemoryHealth: ...
    def close(self) -> None: ...


class NoopMemoryProvider:
    """Privacy-preserving default used until persistence is explicitly enabled."""

    def recall(self, request: MemoryRecallRequest) -> tuple[MemoryRecord, ...]:
        return ()

    def remember(self, request: MemoryRememberRequest) -> MemoryWriteReceipt:
        return MemoryWriteReceipt(accepted=0, rejected=len(request.records))

    def forget(self, request: MemoryForgetRequest) -> MemoryDeleteReceipt:
        return MemoryDeleteReceipt(deleted=0)

    def health(self) -> MemoryHealth:
        return MemoryHealth(status=MemoryProviderStatus.DISABLED, provider="noop")

    def close(self) -> None:
        return None


_DDL = """
CREATE TABLE IF NOT EXISTS memory_provider_records (
  record_id TEXT PRIMARY KEY, scope TEXT NOT NULL, kind TEXT NOT NULL,
  content TEXT NOT NULL, source TEXT NOT NULL, metadata_json TEXT NOT NULL,
  created_unix_secs REAL, expires_unix_secs REAL
)
"""


class LocalMemoryProvider:
    """Small opt-in SQLite provider suitable for local/private deployments."""

    def __init__(self, path: str | Path) -> None:
        self._path = Path(path)
        self._path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = RLock()
        with self._connect() as conn:
            conn.execute(_DDL)

    def _connect(self) -> sqlite3.Connection:
        return sqlite3.connect(str(self._path), timeout=0.5)

    def remember(self, request: MemoryRememberRequest) -> MemoryWriteReceipt:
        with self._lock, self._connect() as conn:
            for record in request.records:
                conn.execute(
                    "INSERT OR REPLACE INTO memory_provider_records VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    (
                        record.record_id,
                        record.scope,
                        record.kind,
                        record.content,
                        record.source,
                        json.dumps(record.metadata, sort_keys=True, separators=(",", ":"), allow_nan=False),
                        record.created_unix_secs,
                        record.expires_unix_secs,
                    ),
                )
        return MemoryWriteReceipt(accepted=len(request.records))

    def recall(self, request: MemoryRecallRequest) -> tuple[MemoryRecord, ...]:
        clauses = ["content LIKE ?", "(expires_unix_secs IS NULL OR expires_unix_secs > ?)"]
        params: list[Any] = [f"%{request.query}%"]
        params.append(time.time())
        if request.scope is not None:
            clauses.append("scope = ?")
            params.append(request.scope)
        if request.kinds:
            clauses.append(f"kind IN ({','.join('?' for _ in request.kinds)})")
            params.extend(request.kinds)
        params.append(request.limit)
        sql = (
            "SELECT record_id, scope, kind, content, source, metadata_json, "
            "created_unix_secs, expires_unix_secs FROM memory_provider_records WHERE "
            + " AND ".join(clauses)
            + " ORDER BY created_unix_secs DESC LIMIT ?"
        )
        with self._lock, self._connect() as conn:
            rows = conn.execute(sql, params).fetchall()
        return tuple(
            MemoryRecord(
                record_id=row[0],
                scope=row[1],
                kind=row[2],
                content=row[3],
                source=row[4],
                metadata=json.loads(row[5]),
                created_unix_secs=row[6],
                expires_unix_secs=row[7],
            )
            for row in rows
        )

    def forget(self, request: MemoryForgetRequest) -> MemoryDeleteReceipt:
        clauses: list[str] = []
        params: list[Any] = []
        if request.record_ids:
            clauses.append(f"record_id IN ({','.join('?' for _ in request.record_ids)})")
            params.extend(request.record_ids)
        if request.scope is not None:
            clauses.append("scope = ?")
            params.append(request.scope)
        if not clauses:
            return MemoryDeleteReceipt(deleted=0)
        with self._lock, self._connect() as conn:
            cursor = conn.execute("DELETE FROM memory_provider_records WHERE " + " AND ".join(clauses), params)
        return MemoryDeleteReceipt(deleted=max(0, cursor.rowcount))

    def health(self) -> MemoryHealth:
        try:
            with self._connect() as conn:
                conn.execute("SELECT 1").fetchone()
            return MemoryHealth(status=MemoryProviderStatus.READY, provider="local-sqlite")
        except sqlite3.Error as exc:
            return MemoryHealth(status=MemoryProviderStatus.DEGRADED, provider="local-sqlite", detail=str(exc))

    def close(self) -> None:
        return None


class MemoryCoordinator:
    """Run every provider operation on bounded worker threads with a deadline."""

    def __init__(self, provider: MemoryProvider, *, timeout_secs: float = 2.0, max_workers: int = 2) -> None:
        self._provider = provider
        self._timeout_secs = max(0.01, float(timeout_secs))
        self._executor = ThreadPoolExecutor(max_workers=max(1, max_workers), thread_name_prefix="dcc-memory")

    def _submit(self, operation: Callable[..., Any], argument: Any = None) -> Future[Any]:
        result: Future[Any] = Future()

        def run() -> None:
            try:
                value = operation() if argument is None else operation(argument)
                if not result.done():
                    result.set_result(value)
            except BaseException as exc:
                if not result.done():
                    result.set_exception(exc)

        worker = self._executor.submit(run)

        def expire() -> None:
            if result.done():
                return
            try:
                result.set_exception(TimeoutError("memory provider timed out"))
            except Exception:
                return

        timer = Timer(self._timeout_secs, expire)
        timer.daemon = True
        timer.start()
        result.add_done_callback(lambda _future: timer.cancel())
        result.add_done_callback(lambda future: worker.cancel() if future.cancelled() else None)
        return result

    def recall(self, request: MemoryRecallRequest) -> Future[tuple[MemoryRecord, ...]]:
        return self._submit(self._provider.recall, request)

    def remember(self, request: MemoryRememberRequest) -> Future[MemoryWriteReceipt]:
        return self._submit(self._provider.remember, request)

    def forget(self, request: MemoryForgetRequest) -> Future[MemoryDeleteReceipt]:
        return self._submit(self._provider.forget, request)

    def health(self) -> Future[MemoryHealth]:
        return self._submit(self._provider.health)

    def close(self) -> None:
        # ``cancel_futures`` was added in Python 3.9. The executor has only a
        # bounded number of workers, and outstanding futures are already
        # guarded by coordinator timeouts, so the Python 3.7-compatible form
        # preserves the shutdown contract.
        self._executor.shutdown(wait=False)
        self._provider.close()


MemoryProviderFactory = Callable[..., MemoryProvider]


def discover_memory_provider_factories(
    *, entry_points: Callable[..., Any] | None = None
) -> dict[str, MemoryProviderFactory]:
    """Load opt-in provider factories registered by external distributions."""
    if entry_points is None:
        try:
            from importlib import metadata as importlib_metadata
        except ImportError:  # pragma: no cover - Python 3.7 compatibility
            import importlib_metadata  # type: ignore[import-not-found,no-redef]
        entry_points = importlib_metadata.entry_points
    try:
        candidates = entry_points(group=MEMORY_PROVIDER_ENTRY_POINT_GROUP)
    except TypeError:  # legacy importlib_metadata API
        all_points = entry_points()
        candidates = (
            all_points.select(group=MEMORY_PROVIDER_ENTRY_POINT_GROUP)
            if hasattr(all_points, "select")
            else all_points.get(MEMORY_PROVIDER_ENTRY_POINT_GROUP, ())
        )
    discovered: dict[str, MemoryProviderFactory] = {}
    for entry_point in candidates:
        discovered[str(entry_point.name)] = entry_point.load()
    return discovered


__all__ = [
    "MEMORY_PROVIDER_ENTRY_POINT_GROUP",
    "LocalMemoryProvider",
    "MemoryCoordinator",
    "MemoryDeleteReceipt",
    "MemoryForgetRequest",
    "MemoryHealth",
    "MemoryProvider",
    "MemoryProviderFactory",
    "MemoryProviderStatus",
    "MemoryRecallRequest",
    "MemoryRecord",
    "MemoryRememberRequest",
    "MemoryWriteReceipt",
    "NoopMemoryProvider",
    "discover_memory_provider_factories",
]
