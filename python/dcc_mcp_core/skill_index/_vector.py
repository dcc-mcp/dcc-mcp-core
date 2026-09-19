"""Vector-based skill index — local-first, deployment-friendly (issue #1393).

Implements :class:`SemanticSkillIndex` from
:mod:`dcc_mcp_core.skill_index` over an in-process vector store.
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
register both a :class:`~dcc_mcp_core.skill_index.LexicalSkillIndex`
*and* a :class:`VectorSkillIndex` into
:class:`~dcc_mcp_core.skill_index.RrfFusionIndex`::

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
from pathlib import Path
from threading import RLock
from typing import Iterable

from dcc_mcp_core._typing import Protocol
from dcc_mcp_core._typing import runtime_checkable
from dcc_mcp_core.skill_index._protocol import SkillDocument
from dcc_mcp_core.skill_index._protocol import SkillSearchHit
from dcc_mcp_core.vector_embedder import CachedEmbedder
from dcc_mcp_core.vector_embedder import Embedder
from dcc_mcp_core.vector_embedder import EmbeddingCache
from dcc_mcp_core.vector_embedder import EmbeddingCacheStats
from dcc_mcp_core.vector_embedder import HashedEmbedder
from dcc_mcp_core.vector_embedder import default_embedding_cache_path

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

    Args:
        embedder: Embedder to use; defaults to :class:`HashedEmbedder`.
        store: Vector store to use; defaults to :class:`InMemoryVectorStore`.
        cache_path: File backing the embedding warm-start cache.  ``None``
            (default) keeps the index purely in memory — no filesystem
            side effects.
        embedding_cache: Pre-built :class:`EmbeddingCache` to share between
            indexes.  Wins over *cache_path*.
        dcc_name: Convenience warm-start switch.  When set (and no explicit
            *cache_path* / *embedding_cache* is given) vectors are cached at
            :func:`~dcc_mcp_core.vector_embedder.default_embedding_cache_path`
            so a second process start with unchanged skill docs performs zero
            embedding computations (issue #2300).

    """

    def __init__(
        self,
        *,
        embedder: Embedder | None = None,
        store: VectorStore | None = None,
        cache_path: str | Path | None = None,
        embedding_cache: EmbeddingCache | None = None,
        dcc_name: str | None = None,
    ) -> None:
        base_embedder: Embedder = embedder if embedder is not None else HashedEmbedder()
        resolved_path: str | Path | None = cache_path
        if resolved_path is None and embedding_cache is None and dcc_name:
            # Warm-start by DCC name: ``~/.dcc-mcp/<dcc>/skill-embeddings.json``.
            resolved_path = default_embedding_cache_path(dcc_name)
        if embedding_cache is not None or resolved_path is not None:
            # Warm-start: unchanged documents reuse the persisted vector instead
            # of being re-embedded on every process start (issue #2300).
            base_embedder = CachedEmbedder(base_embedder, embedding_cache, cache_path=resolved_path)
        self._embedder: Embedder = base_embedder
        self._store: VectorStore = store if store is not None else InMemoryVectorStore()

    def __len__(self) -> int:
        return len(self._store)

    @property
    def embedder(self) -> Embedder:
        """The embedder this index uses; useful for diagnostics and tests."""
        return self._embedder

    @property
    def embedding_cache(self) -> EmbeddingCache | None:
        """The warm-start cache, or ``None`` when this index does not persist one."""
        return getattr(self._embedder, "cache", None)

    @property
    def embedding_stats(self) -> EmbeddingCacheStats | None:
        """Hit/miss counters for the warm-start cache, or ``None``."""
        return getattr(self._embedder, "stats", None)

    def flush_embedding_cache(self) -> bool:
        """Persist any pending cached vectors. Returns success."""
        cache = self.embedding_cache
        if cache is None:
            return False
        return cache.flush()

    @property
    def store(self) -> VectorStore:
        """The vector store this index uses; useful for diagnostics and tests."""
        return self._store

    def index(self, documents: Iterable[SkillDocument]) -> int:
        """Embed and store *documents*, persisting the warm-start cache once.

        The cache is flushed after the whole batch, not per document: an
        in-memory cache already serves repeated documents inside one call, and
        a per-miss flush would rewrite the cache file N times for N documents.
        """
        added = 0
        for doc in documents:
            vec = self._embedder.embed(doc.corpus())
            self._store.add(doc.skill_id, vec)
            added += 1
        self.flush_embedding_cache()
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
