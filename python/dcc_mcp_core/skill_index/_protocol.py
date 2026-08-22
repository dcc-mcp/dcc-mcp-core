"""Shared documents, results, and protocol for Python skill indexes."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable

from dcc_mcp_core._typing import Protocol


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
        """Return the searchable text shared by all index backends."""
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
    """One result row returned by a skill index."""

    skill_id: str
    score: float
    rank: int
    match_reasons: tuple[str, ...] = ()


class SemanticSkillIndex(Protocol):
    """Pluggable Python skill-search backend."""

    def index(self, documents: Iterable[SkillDocument]) -> int: ...
    def search(self, query: str, *, k: int = 8) -> tuple[SkillSearchHit, ...]: ...
    def clear(self) -> None: ...


__all__ = ["SemanticSkillIndex", "SkillDocument", "SkillSearchHit"]
