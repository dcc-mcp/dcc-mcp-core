"""Vector-based skill index — local-first, deployment-friendly (issue #1393).

Implements :class:`SemanticSkillIndex` from
:mod:`dcc_mcp_core.semantic_skill_index` over an in-process vector store.
Defaults to :class:`~dcc_mcp_core.vector_embedder.HashedEmbedder` (zero-dep)
and :class:`InMemoryVectorStore` (brute-force cosine), so adapters get
working semantic-lite recall without adding any runtime dependency.

Architecture::

    ┌──────────────────────────┐
    │ VectorSkillIndex         │   ← implements SemanticSkillIndex Protocol
    │  index(documents)        │
    │  search(query, k) → hits │
    └────────────┬─────────────┘
                 │
        ┌────────┴────────┐
        ▼                 ▼
    Embedder         VectorStore
    (Protocol)       (Protocol)
        │                 │
        │ embed(text)     │ add / remove / search(qv, k)
        │ → unit vector   │
        ▼                 ▼
    HashedEmbedder   InMemoryVectorStore
    (zero-dep)       (zero-dep, brute-force cosine)
    OnnxEmbedder     SqliteVecStore (future)
    (optional)       RemoteVectorStore (future)

Both seams are Protocols, so adapters can swap implementations independently
(e.g. keep the in-memory store but plug in a remote embedder, or keep the
hashed embedder but persist the store via SQLite-vec) without touching call sites.

To get the best of both worlds — exact-match precision plus intent recall —
register both a :class:`~dcc_mcp_core.semantic_skill_index.LexicalSkillIndex`
*and* a :class:`VectorSkillIndex` into
:class:`~dcc_mcp_core.semantic_skill_index.RrfFusionIndex`::

    from dcc_mcp_core import (
        LexicalSkillIndex, RrfFusionIndex, VectorSkillIndex,
    )

    fused = (
        RrfFusionIndex()
        .register("lex", LexicalSkillIndex())
        .register("vec", VectorSkillIndex())
    )
    fused.index(documents)
    hits = fused.search("how do i create a polygon sphere", k=8)
"""

from __future__ import annotations

from array import array
from dataclasses import dataclass
import sys
from threading import RLock
from typing import Iterable

from dcc_mcp_core.semantic_skill_index import SkillDocument
from dcc_mcp_core.semantic_skill_index import SkillSearchHit
from dcc_mcp_core.vector_embedder import Embedder
from dcc_mcp_core.vector_embedder import HashedEmbedder

if sys.version_info >= (3, 8):
    from typing import Protocol
    from typing import runtime_checkable
else:  # pragma: no cover - py3.7 only

    class Protocol:  # type: ignore[no-redef]
        pass

    def runtime_checkable(cls):  # type: ignore[no-redef]
        return cls


__all__ = [
    "InMemoryVectorStore",
    "VectorSkillIndex",
    "VectorStore",
]


def _cosine_dot(a: array[float], b: array[float]) -> float:
    """Dot product of two equal-length vectors.

    Both inputs are expected to be L2-normalised by the embedder, so dot
    product equals cosine similarity. Returns 0.0 on length mismatch so a
    runtime embedder swap (different ``dim``) is degraded gracefully rather
    than crashing the search.
    """
    if len(a) != len(b):
        return 0.0
    total = 0.0
    for x, y in zip(a, b):
        total += x * y
    return total


@runtime_checkable
class VectorStore(Protocol):
    """Pluggable backing store for ``(skill_id, vector)`` rows."""

    def add(self, skill_id: str, vector: array[float]) -> None: ...

    def remove(self, skill_id: str) -> bool: ...

    def clear(self) -> None: ...

    def search(self, query_vector: array[float], k: int) -> list[tuple[str, float]]: ...

    def __len__(self) -> int: ...


@dataclass
class _VecRow:
    skill_id: str
    vector: array[float]


class InMemoryVectorStore:
    """Brute-force cosine search over Python ``array.array`` rows.

    Threadsafe via a single ``RLock``. Performance budget: ~10 µs per row at
    ``dim=256`` on a modern CPython, so 10 k rows ≈ 100 ms / query, 1 k rows
    ≈ 10 ms. Realistic DCC adapters today have ≤100 skills total, putting
    every query well under 2 ms — no HNSW / FAISS / sqlite-vec needed.

    When skill counts grow beyond ~10 k *and* search latency becomes a
    bottleneck, swap this class for a persistent vector store implementing
    the same :class:`VectorStore` Protocol; the embedder and index code do
    not change.
    """

    def __init__(self) -> None:
        self._rows: dict[str, _VecRow] = {}
        self._lock = RLock()

    def __len__(self) -> int:
        with self._lock:
            return len(self._rows)

    def add(self, skill_id: str, vector: array[float]) -> None:
        with self._lock:
            self._rows[skill_id] = _VecRow(skill_id, vector)

    def remove(self, skill_id: str) -> bool:
        with self._lock:
            return self._rows.pop(skill_id, None) is not None

    def clear(self) -> None:
        with self._lock:
            self._rows.clear()

    def search(self, query_vector: array[float], k: int) -> list[tuple[str, float]]:
        if k <= 0:
            return []
        with self._lock:
            scored: list[tuple[str, float]] = []
            for row in self._rows.values():
                score = _cosine_dot(row.vector, query_vector)
                if score > 0:
                    scored.append((row.skill_id, score))
            scored.sort(key=lambda item: item[1], reverse=True)
            return scored[:k]


class VectorSkillIndex:
    """:class:`SemanticSkillIndex` implementation backed by an embedder + vector store.

    By default uses :class:`HashedEmbedder` and :class:`InMemoryVectorStore`,
    both zero-dep. Callers can inject either to swap in a real ONNX embedder
    (via the ``[semantic]`` extra) or a persistent vector store without
    touching downstream call sites.

    Empty queries return an empty tuple (matches
    :class:`LexicalSkillIndex` behaviour). Re-indexing a known
    ``skill_id`` replaces the previous vector — same contract as
    :class:`LexicalSkillIndex`.
    """

    def __init__(
        self,
        *,
        embedder: Embedder | None = None,
        store: VectorStore | None = None,
    ) -> None:
        self._embedder: Embedder = embedder if embedder is not None else HashedEmbedder()
        self._store: VectorStore = store if store is not None else InMemoryVectorStore()

    def __len__(self) -> int:
        return len(self._store)

    @property
    def embedder(self) -> Embedder:
        """The embedder this index uses; useful for diagnostics and tests."""
        return self._embedder

    @property
    def store(self) -> VectorStore:
        """The vector store this index uses; useful for diagnostics and tests."""
        return self._store

    def index(self, documents: Iterable[SkillDocument]) -> int:
        added = 0
        for doc in documents:
            vec = self._embedder.embed(doc.corpus())
            self._store.add(doc.skill_id, vec)
            added += 1
        return added

    def remove(self, skill_id: str) -> bool:
        return self._store.remove(skill_id)

    def clear(self) -> None:
        self._store.clear()

    def search(self, query: str, *, k: int = 8) -> tuple[SkillSearchHit, ...]:
        if k <= 0 or not query.strip():
            return ()
        query_vec = self._embedder.embed(query)
        hits = self._store.search(query_vec, k)
        return tuple(
            SkillSearchHit(
                skill_id=skill_id,
                score=score,
                rank=rank,
                match_reasons=("vec:cosine",),
            )
            for rank, (skill_id, score) in enumerate(hits)
        )
