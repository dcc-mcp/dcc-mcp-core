"""Semantic skill index for DCC adapters (issue #1333).

Design goal: typed-intent search across thousands of skills with predictable
recall even when the agent's wording differs from the SKILL.md author's.

This module is the **public contract** for #1333:

* `SemanticSkillIndex` Protocol — pluggable backend trait.
* `SkillDocument` / `SkillSearchHit` — wire types shared by every backend.
* `LexicalSkillIndex` — default in-memory BM25-style lexical backend.
* `RrfFusionIndex` — Reciprocal Rank Fusion combiner that takes two or
  more indexes (e.g. lexical + vector) and merges their rankings.

The vector backend is intentionally **not** included in this PR. It needs a
heavy dependency (ONNX runtime / sqlite-vec / FAISS) that the core crate
must not require by default. Adapters that want vector recall today can
implement the Protocol and pass their backend into `RrfFusionIndex`
without renegotiating the wire shape when the default vector backend
lands later.

External design references:

* Robertson & Zaragoza, *The Probabilistic Relevance Framework: BM25
  and Beyond* (Found. Trends Inf. Retr., 2009).
* Cormack et al., *Reciprocal Rank Fusion outperforms Condorcet and
  individual rank learning methods* (SIGIR 2009).
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field
from math import log
import re
from threading import RLock
from typing import Iterable, Mapping, Protocol

__all__ = [
    "LexicalSkillIndex",
    "RrfFusionIndex",
    "SemanticSkillIndex",
    "SkillDocument",
    "SkillSearchHit",
]


_TOKEN_RE = re.compile(r"[A-Za-z0-9]+")


def _tokenise(text: str) -> list[str]:
    return [tok.lower() for tok in _TOKEN_RE.findall(text)]


@dataclass(frozen=True)
class SkillDocument:
    """Stable wire shape for an indexable skill or tool record."""

    skill_id: str
    name: str
    summary: str = ""
    intent: str = ""
    tags: tuple[str, ...] = ()
    search_aliases: tuple[str, ...] = ()
    dcc_name: str = ""

    def corpus(self) -> str:
        return " ".join(
            (
                self.name,
                self.intent,
                self.summary,
                " ".join(self.tags),
                " ".join(self.search_aliases),
            )
        )


@dataclass(frozen=True)
class SkillSearchHit:
    skill_id: str
    score: float
    rank: int
    match_reasons: tuple[str, ...] = ()


class SemanticSkillIndex(Protocol):
    """Pluggable semantic search backend."""

    def index(self, documents: Iterable[SkillDocument]) -> int: ...
    def search(self, query: str, *, k: int = 8) -> tuple[SkillSearchHit, ...]: ...
    def clear(self) -> None: ...


# ── Lexical (BM25-style) ───────────────────────────────────────────────


@dataclass
class _LexEntry:
    doc: SkillDocument
    term_counts: dict[str, int]
    length: int


class LexicalSkillIndex:
    """In-memory BM25-style lexical index. Threadsafe."""

    def __init__(self, *, k1: float = 1.5, b: float = 0.75) -> None:
        if k1 <= 0 or not (0.0 <= b <= 1.0):
            raise ValueError("k1 must be > 0 and b must be in [0.0, 1.0]")
        self._k1 = k1
        self._b = b
        self._lock = RLock()
        self._docs: dict[str, _LexEntry] = {}
        self._df: dict[str, int] = defaultdict(int)
        self._total_length = 0

    def __len__(self) -> int:
        with self._lock:
            return len(self._docs)

    def index(self, documents: Iterable[SkillDocument]) -> int:
        """Add or replace documents. Returns the count actually written."""
        added = 0
        with self._lock:
            for doc in documents:
                self._remove_unlocked(doc.skill_id)
                tokens = _tokenise(doc.corpus())
                counts: dict[str, int] = defaultdict(int)
                for tok in tokens:
                    counts[tok] += 1
                self._docs[doc.skill_id] = _LexEntry(doc, dict(counts), len(tokens))
                for tok in counts:
                    self._df[tok] += 1
                self._total_length += len(tokens)
                added += 1
        return added

    def clear(self) -> None:
        with self._lock:
            self._docs.clear()
            self._df.clear()
            self._total_length = 0

    def _remove_unlocked(self, skill_id: str) -> None:
        prev = self._docs.pop(skill_id, None)
        if prev is None:
            return
        self._total_length -= prev.length
        for tok in prev.term_counts:
            self._df[tok] -= 1
            if self._df[tok] <= 0:
                del self._df[tok]

    def remove(self, skill_id: str) -> bool:
        with self._lock:
            before = skill_id in self._docs
            self._remove_unlocked(skill_id)
            return before

    def search(self, query: str, *, k: int = 8) -> tuple[SkillSearchHit, ...]:
        if k <= 0:
            return ()
        with self._lock:
            terms = _tokenise(query)
            if not terms or not self._docs:
                return ()
            avg_len = self._total_length / max(1, len(self._docs))
            scored: list[tuple[str, float, list[str]]] = []
            for skill_id, entry in self._docs.items():
                score = 0.0
                matched: list[str] = []
                for term in terms:
                    tf = entry.term_counts.get(term, 0)
                    if tf == 0:
                        continue
                    df = self._df.get(term, 1)
                    n = len(self._docs)
                    idf = log(1.0 + (n - df + 0.5) / (df + 0.5))
                    denom = tf + self._k1 * (
                        1.0 - self._b + self._b * (entry.length / max(1.0, avg_len))
                    )
                    score += idf * (tf * (self._k1 + 1.0) / max(1e-9, denom))
                    matched.append(term)
                if score > 0:
                    scored.append((skill_id, score, matched))
            scored.sort(key=lambda x: x[1], reverse=True)
            return tuple(
                SkillSearchHit(
                    skill_id=sid,
                    score=score,
                    rank=rank,
                    match_reasons=tuple(f"lex:{tok}" for tok in matched),
                )
                for rank, (sid, score, matched) in enumerate(scored[:k])
            )


# ── Reciprocal Rank Fusion ─────────────────────────────────────────────


@dataclass
class _BackendSpec:
    name: str
    index: SemanticSkillIndex
    weight: float = 1.0


class RrfFusionIndex:
    """Combine multiple :class:`SemanticSkillIndex` backends via RRF.

    Cormack et al. 2009 — score per doc is
    ``Σ weight_i / (rrf_k + rank_i)``. Default ``rrf_k = 60`` matches the
    original paper.
    """

    def __init__(self, *, rrf_k: int = 60) -> None:
        if rrf_k <= 0:
            raise ValueError("rrf_k must be > 0")
        self._rrf_k = rrf_k
        self._backends: list[_BackendSpec] = []

    def register(self, name: str, index: SemanticSkillIndex, *, weight: float = 1.0) -> RrfFusionIndex:
        if not name:
            raise ValueError("backend name must be non-empty")
        if weight <= 0:
            raise ValueError("backend weight must be > 0")
        self._backends.append(_BackendSpec(name=name, index=index, weight=weight))
        return self

    def index(self, documents: Iterable[SkillDocument]) -> int:
        docs = list(documents)
        for spec in self._backends:
            spec.index.index(docs)
        return len(docs)

    def clear(self) -> None:
        for spec in self._backends:
            spec.index.clear()

    def search(self, query: str, *, k: int = 8) -> tuple[SkillSearchHit, ...]:
        if k <= 0 or not self._backends:
            return ()
        # per-doc fused score and contributing match reasons
        fused: dict[str, float] = defaultdict(float)
        reasons: dict[str, list[str]] = defaultdict(list)
        for spec in self._backends:
            hits = spec.index.search(query, k=max(k, 16))
            for hit in hits:
                fused[hit.skill_id] += spec.weight / (self._rrf_k + hit.rank + 1)
                reasons[hit.skill_id].append(f"{spec.name}:{hit.rank}")
        ordered = sorted(fused.items(), key=lambda x: x[1], reverse=True)
        return tuple(
            SkillSearchHit(
                skill_id=sid,
                score=score,
                rank=rank,
                match_reasons=tuple(reasons[sid]),
            )
            for rank, (sid, score) in enumerate(ordered[:k])
        )
