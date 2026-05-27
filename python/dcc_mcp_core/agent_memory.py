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

_SENSITIVE_KEY_PARTS = (
    "api_key",
    "authorization",
    "password",
    "prompt",
    "secret",
    "token",
)
_MAX_SAFE_STRING_CHARS = 512


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

    Records bounded facts for skill/tool lifecycle events, injects safe memory
    summaries into discovery hooks, and compacts session-scoped observations
    into longterm patterns on ``SESSION_END``. The recorder is deliberately
    conservative: it stores structured payload fields only after redaction and
    can be disabled for privacy-sensitive deployments.
    """

    def __init__(
        self,
        store: MemoryStore,
        *,
        clock: Callable[[], float] = time.time,
        enabled: bool = True,
        summary_limit: int = 8,
        promote_on_session_end: bool = True,
    ) -> None:
        self._store = store
        self._clock = clock
        self._enabled = bool(enabled)
        self._summary_limit = max(1, summary_limit)
        self._promote_on_session_end = bool(promote_on_session_end)

    @property
    def enabled(self) -> bool:
        """Whether hooks should read/write memory."""
        return self._enabled

    def set_enabled(self, enabled: bool) -> None:
        """Enable or disable memory capture/injection without unregistering hooks."""
        self._enabled = bool(enabled)

    def install(self, hooks: Any) -> MemoryRecorder:
        from dcc_mcp_core.lifecycle_hooks import HookEvent

        hooks.on(HookEvent.SESSION_START, self._on_session_start)
        hooks.on(HookEvent.BEFORE_SEARCH, self._on_before_search)
        hooks.on(HookEvent.AFTER_SKILL_LOAD, self._on_after_skill_load)
        hooks.on(HookEvent.BEFORE_TOOL_CALL, self._on_before_tool_call)
        hooks.on(HookEvent.AFTER_TOOL_CALL, self._on_after_tool_call)
        hooks.on(HookEvent.SESSION_END, self._on_session_end)
        return self

    def summarize(
        self,
        *,
        session_id: str | None,
        dcc_name: str | None,
        limit: int | None = None,
    ) -> dict[str, Any]:
        """Return a compact, redacted summary safe for discovery/ranking.

        The summary contains keys and bounded payload fragments only; it never
        includes raw prompts or known credential-like fields.
        """
        if not self._enabled:
            return {}
        effective_limit = max(1, limit or self._summary_limit)
        entries = self._store.query(MemoryQuery(session_id=session_id, dcc_name=dcc_name, limit=effective_limit * 4))
        if not entries:
            return {}

        recent_successes: list[dict[str, Any]] = []
        recent_failures: list[dict[str, Any]] = []
        escape_hatches: list[dict[str, Any]] = []
        missing_capabilities: list[dict[str, Any]] = []
        skip_reasons: list[str] = []

        for entry in entries:
            item = self._summary_item(entry)
            payload = item["payload"]
            if entry.key.endswith(":ok") or payload.get("ok") is True:
                recent_successes.append(item)
            if entry.key.endswith(":fail") or payload.get("ok") is False:
                recent_failures.append(item)
            if payload.get("tool_role") == "escape_hatch" or "escape_hatch" in entry.key:
                escape_hatches.append(item)
            if payload.get("missing_capability"):
                missing_capabilities.append(item)
            reason = payload.get("skip_reason") or payload.get("reason")
            if isinstance(reason, str) and reason not in skip_reasons:
                skip_reasons.append(reason[:_MAX_SAFE_STRING_CHARS])

        summary: dict[str, Any] = {}
        if recent_successes:
            summary["recent_successes"] = recent_successes[:effective_limit]
            summary["prefer_tools"] = _unique_tool_names(recent_successes)[:effective_limit]
        if recent_failures:
            summary["recent_failures"] = recent_failures[:effective_limit]
            summary["avoid_tools"] = _unique_tool_names(recent_failures)[:effective_limit]
        if escape_hatches:
            summary["escape_hatches"] = escape_hatches[:effective_limit]
        if missing_capabilities:
            summary["missing_capabilities"] = missing_capabilities[:effective_limit]
        if skip_reasons:
            summary["skip_reasons"] = skip_reasons[:effective_limit]
        return summary

    def _make_entry(
        self,
        ctx: Any,
        layer: MemoryLayer,
        key: str,
        payload: Mapping[str, Any],
        *,
        score: float = 1.0,
    ) -> MemoryEntry:
        return MemoryEntry(
            layer=layer,
            key=key,
            session_id=ctx.session_id or "default",
            dcc_name=ctx.dcc_name,
            payload=_safe_payload(payload),
            created_unix_secs=self._clock(),
            score=score,
        )

    def _on_session_start(self, ctx: Any) -> None:
        if not self._enabled or ctx.payload is None:
            return
        summary = self.summarize(session_id=ctx.session_id, dcc_name=ctx.dcc_name)
        if summary:
            ctx.payload.setdefault("memory_summary", summary)

    def _on_before_search(self, ctx: Any) -> None:
        if not self._enabled or ctx.payload is None:
            return
        summary = self.summarize(session_id=ctx.session_id, dcc_name=ctx.dcc_name)
        if not summary:
            return
        ctx.payload.setdefault("memory_summary", summary)
        if "prefer_tools" in summary:
            ctx.payload.setdefault("memory_prefer_tools", summary["prefer_tools"])
        if "avoid_tools" in summary:
            ctx.payload.setdefault("memory_avoid_tools", summary["avoid_tools"])

    def _on_after_skill_load(self, ctx: Any) -> None:
        if not self._enabled:
            return
        skill_name = ctx.payload.get("skill_name") if ctx.payload else None
        if not skill_name:
            return
        self._store.put(self._make_entry(ctx, MemoryLayer.EPHEMERAL, f"skill_loaded:{skill_name}", ctx.payload or {}))

    def _on_before_tool_call(self, ctx: Any) -> None:
        if not self._enabled or ctx.payload is None:
            return
        summary = self.summarize(session_id=ctx.session_id, dcc_name=ctx.dcc_name)
        if summary:
            ctx.payload.setdefault("memory_summary", summary)

    def _on_after_tool_call(self, ctx: Any) -> None:
        if not self._enabled:
            return
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
                score=1.0 if ok else -1.0,
            )
        )

    def _on_session_end(self, ctx: Any) -> None:
        if not self._enabled:
            return
        if ctx.session_id is None:
            return
        if self._promote_on_session_end:
            self._compact_session(ctx)
        self._store.forget(session_id=ctx.session_id, layer=MemoryLayer.EPHEMERAL)
        self._store.forget(session_id=ctx.session_id, layer=MemoryLayer.WORKING)

    def _compact_session(self, ctx: Any) -> None:
        rows = self._store.query(
            MemoryQuery(
                layer=MemoryLayer.WORKING,
                session_id=ctx.session_id,
                dcc_name=ctx.dcc_name,
                limit=self._summary_limit * 8,
            )
        )
        grouped: dict[str, list[MemoryEntry]] = {}
        for row in rows:
            grouped.setdefault(row.key, []).append(row)
        for key, entries in grouped.items():
            ok_count = sum(1 for entry in entries if entry.payload.get("ok") is True or key.endswith(":ok"))
            fail_count = sum(1 for entry in entries if entry.payload.get("ok") is False or key.endswith(":fail"))
            sample = entries[0]
            payload = {
                "source_session": ctx.session_id,
                "count": len(entries),
                "ok_count": ok_count,
                "fail_count": fail_count,
                "tool_name": sample.payload.get("tool_name"),
                "tool_role": sample.payload.get("tool_role"),
                "missing_capability": sample.payload.get("missing_capability"),
                "skip_reason": sample.payload.get("skip_reason"),
            }
            self._store.put(
                MemoryEntry(
                    layer=MemoryLayer.LONGTERM,
                    key=f"pattern:{key}",
                    session_id="longterm",
                    dcc_name=ctx.dcc_name,
                    payload=_safe_payload(payload),
                    created_unix_secs=self._clock(),
                    score=float(ok_count - fail_count),
                )
            )

    def _summary_item(self, entry: MemoryEntry) -> dict[str, Any]:
        return {
            "key": entry.key,
            "layer": entry.layer.value,
            "score": entry.score,
            "payload": _safe_payload(entry.payload),
        }


def _safe_payload(payload: Mapping[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, value in payload.items():
        key_text = str(key)
        lowered = key_text.lower()
        if any(part in lowered for part in _SENSITIVE_KEY_PARTS):
            continue
        safe_value = _safe_value(value)
        if safe_value is not None:
            out[key_text] = safe_value
    return out


def _safe_value(value: Any) -> Any | None:
    if value is None or isinstance(value, (bool, int, float)):
        return value
    if isinstance(value, str):
        return value[:_MAX_SAFE_STRING_CHARS]
    if isinstance(value, MemoryLayer):
        return value.value
    if isinstance(value, Mapping):
        return _safe_payload(value)
    if isinstance(value, (list, tuple)):
        out = []
        for item in value[:16]:
            safe_item = _safe_value(item)
            if safe_item is not None:
                out.append(safe_item)
        return out
    return str(value)[:_MAX_SAFE_STRING_CHARS]


def _unique_tool_names(items: Iterable[Mapping[str, Any]]) -> list[str]:
    names: list[str] = []
    for item in items:
        payload = item.get("payload")
        if not isinstance(payload, Mapping):
            continue
        name = payload.get("tool_name")
        if isinstance(name, str) and name and name not in names:
            names.append(name)
    return names
