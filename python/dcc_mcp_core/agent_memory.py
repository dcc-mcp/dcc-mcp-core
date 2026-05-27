"""Agent memory layers for DCC adapters (issue #1334).

Three-tier memory model:

* **Ephemeral** — session-scoped facts (active scene, currently-selected
  nodes, recently-loaded skills, last-failed tool). Bounded ring-buffer
  per session; never persisted.
* **Working** — task-scoped facts (decisions made earlier in the same
  multi-step workflow, declined options, structured intermediate
  artefacts). Bounded TTL; never persisted by default.
* **Longterm** — durable facts (frequently-used skills, project-level
  conventions, persistent agent preferences). Stored only when an
  explicit storage backend is attached (file/sqlite); summarised, never
  raw prompts.

This module is the **contract surface** for #1334: the public layer
types, a `MemoryStore` trait, and an in-memory implementation. The
SQLite/file persistence path for the longterm layer is intentionally
left as an opt-in backend with a clear extension seam — adapters can
plug their own storage without renegotiating the public types when the
default backend lands later.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from dataclasses import field
from enum import Enum
import sys
from threading import RLock
import time
from typing import Any
from typing import Callable
from typing import Iterable
from typing import Mapping

if sys.version_info >= (3, 8):
    from typing import Protocol
else:  # pragma: no cover - py3.7 only

    class Protocol:  # type: ignore[no-redef]
        pass


__all__ = [
    "InMemoryMemoryStore",
    "MemoryEntry",
    "MemoryLayer",
    "MemoryQuery",
    "MemoryRecorder",
    "MemoryStore",
]


class MemoryLayer(str, Enum):
    """Closed vocabulary of memory tiers."""

    EPHEMERAL = "ephemeral"
    WORKING = "working"
    LONGTERM = "longterm"

    @classmethod
    def parse(cls, value: str | MemoryLayer) -> MemoryLayer:
        if isinstance(value, cls):
            return value
        normalised = str(value).strip().lower().replace("-", "_")
        for layer in cls:
            if layer.value == normalised:
                return layer
        raise ValueError(f"unknown MemoryLayer {value!r}")


@dataclass(frozen=True)
class MemoryEntry:
    """A single low-cardinality, JSON-safe memory record.

    The payload is a plain dict; callers are responsible for keeping it
    JSON-serialisable. ``raw_prompt`` is intentionally *not* a separate
    field: the contract is "summarised facts only, never raw prompts"
    so memory exports stay safe to ship to telemetry surfaces.
    """

    layer: MemoryLayer
    key: str
    session_id: str
    dcc_name: str
    payload: Mapping[str, Any] = field(default_factory=dict)
    created_unix_secs: float = field(default_factory=time.time)
    score: float = 1.0

    def with_score(self, score: float) -> MemoryEntry:
        return MemoryEntry(
            layer=self.layer,
            key=self.key,
            session_id=self.session_id,
            dcc_name=self.dcc_name,
            payload=self.payload,
            created_unix_secs=self.created_unix_secs,
            score=score,
        )


@dataclass(frozen=True)
class MemoryQuery:
    """Filter criteria for retrieving entries from an :class:`AgentMemoryStore`."""

    layer: MemoryLayer | None = None
    session_id: str | None = None
    dcc_name: str | None = None
    key_prefix: str | None = None
    limit: int = 16


class MemoryStore(Protocol):
    """Pluggable backend trait."""

    def put(self, entry: MemoryEntry) -> None: ...
    def query(self, q: MemoryQuery) -> tuple[MemoryEntry, ...]: ...
    def forget(self, *, session_id: str | None = None, layer: MemoryLayer | None = None) -> int: ...


class InMemoryMemoryStore:
    """Threadsafe in-memory implementation with bounded retention per layer.

    Default caps (override via constructor):

    * ephemeral: 256 entries / session
    * working:   1 024 entries / session, 6 h TTL
    * longterm:  4 096 entries total, no TTL (until a persistent
      backend is attached)
    """

    def __init__(
        self,
        *,
        ephemeral_cap_per_session: int = 256,
        working_cap_per_session: int = 1024,
        working_ttl_secs: int = 6 * 60 * 60,
        longterm_cap_total: int = 4096,
    ) -> None:
        self._lock = RLock()
        self._ephemeral_cap = max(1, ephemeral_cap_per_session)
        self._working_cap = max(1, working_cap_per_session)
        self._working_ttl = max(0, working_ttl_secs)
        self._longterm_cap = max(1, longterm_cap_total)
        # (layer, session_id) -> deque of entries
        self._by_session: dict[tuple[MemoryLayer, str], deque[MemoryEntry]] = {}
        # longterm is global (not per-session)
        self._longterm: deque[MemoryEntry] = deque(maxlen=self._longterm_cap)

    def put(self, entry: MemoryEntry) -> None:
        with self._lock:
            if entry.layer is MemoryLayer.LONGTERM:
                self._longterm.append(entry)
                return
            cap = self._ephemeral_cap if entry.layer is MemoryLayer.EPHEMERAL else self._working_cap
            bucket = self._by_session.setdefault((entry.layer, entry.session_id), deque(maxlen=cap))
            bucket.append(entry)

    def query(self, q: MemoryQuery) -> tuple[MemoryEntry, ...]:
        now = time.time()
        with self._lock:
            sources: list[Iterable[MemoryEntry]] = []
            if q.layer is None or q.layer is MemoryLayer.LONGTERM:
                sources.append(self._longterm)
            if q.layer is None or q.layer in (MemoryLayer.EPHEMERAL, MemoryLayer.WORKING):
                for (layer, sid), bucket in self._by_session.items():
                    if q.layer is not None and layer is not q.layer:
                        continue
                    if q.session_id is not None and sid != q.session_id:
                        continue
                    sources.append(bucket)

            out: list[MemoryEntry] = []
            for source in sources:
                for entry in source:
                    if q.dcc_name is not None and entry.dcc_name != q.dcc_name:
                        continue
                    if q.key_prefix is not None and not entry.key.startswith(q.key_prefix):
                        continue
                    if (
                        entry.layer is MemoryLayer.WORKING
                        and self._working_ttl > 0
                        and now - entry.created_unix_secs > self._working_ttl
                    ):
                        continue
                    out.append(entry)
            # most-recent first, then highest score
            out.sort(key=lambda e: (e.created_unix_secs, e.score), reverse=True)
            return tuple(out[: max(0, q.limit)])

    def forget(
        self,
        *,
        session_id: str | None = None,
        layer: MemoryLayer | None = None,
    ) -> int:
        removed = 0
        with self._lock:
            if layer is MemoryLayer.LONGTERM:
                removed = len(self._longterm)
                self._longterm.clear()
                return removed
            to_drop: list[tuple[MemoryLayer, str]] = []
            for key, bucket in self._by_session.items():
                layer_match = layer is None or key[0] is layer
                session_match = session_id is None or key[1] == session_id
                if layer_match and session_match:
                    removed += len(bucket)
                    to_drop.append(key)
            for key in to_drop:
                del self._by_session[key]
        return removed

    def __len__(self) -> int:
        with self._lock:
            return sum(len(b) for b in self._by_session.values()) + len(self._longterm)


# ── LifecycleHooks recorder ────────────────────────────────────────────


class MemoryRecorder:
    """Bridge ``LifecycleHooks`` events into a :class:`MemoryStore`.

    Records ephemeral facts for ``BEFORE_SKILL_LOAD`` / ``AFTER_SKILL_LOAD``
    / ``AFTER_TOOL_CALL``, and clears the session's ephemeral + working
    tiers on ``SESSION_END``. Hooks for ``before_search`` and
    ``before_tool_call`` are wired through the same surface so adapters
    can extend behaviour without changing the public contract.
    """

    def __init__(
        self,
        store: MemoryStore,
        *,
        clock: Callable[[], float] = time.time,
    ) -> None:
        self._store = store
        self._clock = clock

    def install(self, hooks: Any) -> MemoryRecorder:
        from dcc_mcp_core.lifecycle_hooks import HookEvent

        hooks.on(HookEvent.AFTER_SKILL_LOAD, self._on_after_skill_load)
        hooks.on(HookEvent.AFTER_TOOL_CALL, self._on_after_tool_call)
        hooks.on(HookEvent.SESSION_END, self._on_session_end)
        return self

    def _make_entry(
        self,
        ctx: Any,
        layer: MemoryLayer,
        key: str,
        payload: Mapping[str, Any],
    ) -> MemoryEntry:
        return MemoryEntry(
            layer=layer,
            key=key,
            session_id=ctx.session_id or "default",
            dcc_name=ctx.dcc_name,
            payload=dict(payload),
            created_unix_secs=self._clock(),
        )

    def _on_after_skill_load(self, ctx: Any) -> None:
        skill_name = ctx.payload.get("skill_name") if ctx.payload else None
        if not skill_name:
            return
        self._store.put(self._make_entry(ctx, MemoryLayer.EPHEMERAL, f"skill_loaded:{skill_name}", ctx.payload or {}))

    def _on_after_tool_call(self, ctx: Any) -> None:
        tool = ctx.payload.get("tool_name") if ctx.payload else None
        if not tool:
            return
        ok = bool(ctx.payload.get("ok", True))
        self._store.put(
            self._make_entry(
                ctx,
                MemoryLayer.WORKING,
                f"tool_call:{tool}:{'ok' if ok else 'fail'}",
                ctx.payload or {},
            )
        )

    def _on_session_end(self, ctx: Any) -> None:
        if ctx.session_id is None:
            return
        self._store.forget(session_id=ctx.session_id, layer=MemoryLayer.EPHEMERAL)
        self._store.forget(session_id=ctx.session_id, layer=MemoryLayer.WORKING)
