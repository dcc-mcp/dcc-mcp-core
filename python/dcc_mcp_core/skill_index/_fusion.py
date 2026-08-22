"""Reciprocal Rank Fusion across Python skill-index backends."""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from typing import Iterable

from dcc_mcp_core.skill_index._protocol import SemanticSkillIndex
from dcc_mcp_core.skill_index._protocol import SkillDocument
from dcc_mcp_core.skill_index._protocol import SkillSearchHit


@dataclass
class _BackendSpec:
    name: str
    index: SemanticSkillIndex
    weight: float = 1.0


class RrfFusionIndex:
    """Combine multiple skill indexes with Reciprocal Rank Fusion."""

    def __init__(self, *, rrf_k: int = 60) -> None:
        if rrf_k <= 0:
            raise ValueError("rrf_k must be > 0")
        self._rrf_k = rrf_k
        self._backends: list[_BackendSpec] = []

    def register(self, name: str, index: SemanticSkillIndex, *, weight: float = 1.0) -> RrfFusionIndex:
        """Register a named backend and return this fusion index."""
        if not name:
            raise ValueError("backend name must be non-empty")
        if weight <= 0:
            raise ValueError("backend weight must be > 0")
        self._backends.append(_BackendSpec(name=name, index=index, weight=weight))
        return self

    def index(self, documents: Iterable[SkillDocument]) -> int:
        """Index the same document snapshot in every registered backend."""
        docs = list(documents)
        for spec in self._backends:
            spec.index.index(docs)
        return len(docs)

    def clear(self) -> None:
        """Clear every registered backend."""
        for spec in self._backends:
            spec.index.clear()

    def search(self, query: str, *, k: int = 8) -> tuple[SkillSearchHit, ...]:
        """Fuse backend ranks into a deterministic result sequence."""
        if k <= 0 or not self._backends:
            return ()
        fused: dict[str, float] = defaultdict(float)
        reasons: dict[str, list[str]] = defaultdict(list)
        for spec in self._backends:
            hits = spec.index.search(query, k=max(k, 16))
            for hit in hits:
                fused[hit.skill_id] += spec.weight / (self._rrf_k + hit.rank + 1)
                reasons[hit.skill_id].append(f"{spec.name}:{hit.rank}")
        ordered = sorted(fused.items(), key=lambda item: item[1], reverse=True)
        return tuple(
            SkillSearchHit(
                skill_id=skill_id,
                score=score,
                rank=rank,
                match_reasons=tuple(reasons[skill_id]),
            )
            for rank, (skill_id, score) in enumerate(ordered[:k])
        )


__all__ = ["RrfFusionIndex"]
