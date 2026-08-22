"""Zero-dependency BM25-style skill index backend."""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from math import log
import re
from threading import RLock
from typing import Iterable

from dcc_mcp_core.skill_index._protocol import SkillDocument
from dcc_mcp_core.skill_index._protocol import SkillSearchHit

__all__ = ["LexicalSkillIndex"]


_TOKEN_RE = re.compile(r"[A-Za-z0-9]+")


def _tokenise(text: str) -> list[str]:
    return [tok.lower() for tok in _TOKEN_RE.findall(text)]


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
                    denom = tf + self._k1 * (1.0 - self._b + self._b * (entry.length / max(1.0, avg_len)))
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
