"""Capability graph for cross-skill agent reasoning (issue #1336).

The graph models edges between skills, tools, and named capabilities so the
search ranker, the agent planner, and the admin UI can answer questions like

* "what else do I need before this skill is useful?"     (``REQUIRES``)
* "what does this skill produce that other skills need?" (``PRODUCES``)
* "if this skill is missing, what is the fallback?"      (``FALLBACK_FOR``)

This module is the public contract: a small, in-memory, threadsafe graph
that downstream PRs (semantic skill index #1333, agent memory layers
#1334, gateway search reranker integration) plug into without renegotiating
the wire shape.

Design rules:

* Pure data + bounded expansion. No I/O, no async, no embedding inference.
* Idempotent inserts — registering the same skill twice never duplicates
  edges.
* Edge kinds are a closed enum so admin counters stay low-cardinality.
* Expansion is depth-bounded so worst-case query time is O(nodes * depth).
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from dataclasses import field
from enum import Enum
from threading import RLock
from typing import Any
from typing import Iterable
from typing import Mapping

__all__ = ["CapabilityEdge", "CapabilityGraph", "EdgeKind"]


class EdgeKind(str, Enum):
    """Closed vocabulary of relationships between capability nodes."""

    DEPENDS_ON = "depends_on"
    REQUIRES = "requires"
    PRODUCES = "produces"
    USED_IN = "used_in"
    COMPATIBLE_WITH = "compatible_with"
    REPLACES = "replaces"
    FALLBACK_FOR = "fallback_for"

    @classmethod
    def parse(cls, value: str | EdgeKind) -> EdgeKind:
        """Accept the enum, the snake_case label, or the kebab-case alias."""
        if isinstance(value, cls):
            return value
        normalised = str(value).strip().lower().replace("-", "_")
        for kind in cls:
            if kind.value == normalised:
                return kind
        raise ValueError(f"unknown EdgeKind {value!r}")


@dataclass(frozen=True)
class CapabilityEdge:
    """Directed edge between two capability nodes."""

    source: str
    target: str
    kind: EdgeKind
    weight: float = 1.0
    notes: str = ""

    def __post_init__(self) -> None:
        if not self.source or not self.target:
            raise ValueError("CapabilityEdge source and target must be non-empty")
        if self.source == self.target:
            raise ValueError(f"self-loops not allowed: {self.source!r}")
        if not (0.0 <= self.weight <= 1.0):
            raise ValueError(f"weight must be in [0.0, 1.0]; got {self.weight!r}")


@dataclass
class _Adjacency:
    out: dict[EdgeKind, list[CapabilityEdge]] = field(default_factory=dict)
    inc: dict[EdgeKind, list[CapabilityEdge]] = field(default_factory=dict)


class CapabilityGraph:
    """Threadsafe in-memory capability graph (issue #1336)."""

    def __init__(self) -> None:
        self._lock = RLock()
        self._nodes: dict[str, _Adjacency] = {}
        self._edges: set[tuple[str, str, EdgeKind]] = set()

    # ── mutation ───────────────────────────────────────────────────────

    def add_node(self, node_id: str) -> None:
        if not node_id:
            raise ValueError("node id must be non-empty")
        with self._lock:
            self._nodes.setdefault(node_id, _Adjacency())

    def add_edge(self, edge: CapabilityEdge) -> bool:
        """Insert an edge. Returns ``True`` when it was new."""
        key = (edge.source, edge.target, edge.kind)
        with self._lock:
            if key in self._edges:
                return False
            self._edges.add(key)
            self._nodes.setdefault(edge.source, _Adjacency()).out.setdefault(edge.kind, []).append(edge)
            self._nodes.setdefault(edge.target, _Adjacency()).inc.setdefault(edge.kind, []).append(edge)
            return True

    def register_skill(
        self,
        skill_id: str,
        *,
        requires: Iterable[str] = (),
        produces: Iterable[str] = (),
        depends_on: Iterable[str] = (),
        replaces: Iterable[str] = (),
        fallback_for: Iterable[str] = (),
        compatible_with: Iterable[str] = (),
    ) -> int:
        """Register a skill's outbound edges from its declared metadata.

        Returns the count of newly-inserted edges (idempotent on repeat
        calls). Mirrors the optional fields added to ``SkillMetadata`` in
        #1335 so a skill loader can wire its frontmatter directly.
        """
        added = 0
        for target in requires:
            added += int(self.add_edge(CapabilityEdge(skill_id, target, EdgeKind.REQUIRES)))
        for target in produces:
            added += int(self.add_edge(CapabilityEdge(skill_id, target, EdgeKind.PRODUCES)))
        for target in depends_on:
            added += int(self.add_edge(CapabilityEdge(skill_id, target, EdgeKind.DEPENDS_ON)))
        for target in replaces:
            added += int(self.add_edge(CapabilityEdge(skill_id, target, EdgeKind.REPLACES)))
        for target in fallback_for:
            added += int(self.add_edge(CapabilityEdge(skill_id, target, EdgeKind.FALLBACK_FOR)))
        for target in compatible_with:
            added += int(self.add_edge(CapabilityEdge(skill_id, target, EdgeKind.COMPATIBLE_WITH)))
        return added

    # ── inspection ─────────────────────────────────────────────────────

    def nodes(self) -> tuple[str, ...]:
        with self._lock:
            return tuple(sorted(self._nodes))

    def edges(self, kinds: Iterable[EdgeKind] | None = None) -> tuple[CapabilityEdge, ...]:
        wanted = frozenset(kinds) if kinds else None
        with self._lock:
            out: list[CapabilityEdge] = []
            for adj in self._nodes.values():
                for kind, edge_list in adj.out.items():
                    if wanted is not None and kind not in wanted:
                        continue
                    out.extend(edge_list)
            return tuple(out)

    def neighbors(
        self,
        node_id: str,
        *,
        kinds: Iterable[EdgeKind] | None = None,
        direction: str = "out",
    ) -> tuple[CapabilityEdge, ...]:
        """Return edges adjacent to ``node_id``.

        ``direction`` is ``"out"`` (default), ``"in"``, or ``"both"``.
        """
        if direction not in {"out", "in", "both"}:
            raise ValueError(f"direction must be 'out', 'in', or 'both'; got {direction!r}")
        wanted = frozenset(kinds) if kinds else None
        with self._lock:
            adj = self._nodes.get(node_id)
            if adj is None:
                return ()
            buckets: list[Mapping[EdgeKind, list[CapabilityEdge]]] = []
            if direction in {"out", "both"}:
                buckets.append(adj.out)
            if direction in {"in", "both"}:
                buckets.append(adj.inc)
            out: list[CapabilityEdge] = []
            for bucket in buckets:
                for kind, edge_list in bucket.items():
                    if wanted is not None and kind not in wanted:
                        continue
                    out.extend(edge_list)
            return tuple(out)

    # ── expansion ──────────────────────────────────────────────────────

    def expand(
        self,
        seeds: Iterable[str],
        *,
        kinds: Iterable[EdgeKind] | None = None,
        max_depth: int = 2,
        direction: str = "out",
    ) -> tuple[str, ...]:
        """Bounded BFS expansion. Returns reachable node ids excluding seeds.

        ``max_depth`` is clamped to ``[1, 16]`` so a single call cannot
        wander the whole graph.
        """
        if max_depth < 1:
            return ()
        depth = min(max_depth, 16)
        wanted = frozenset(kinds) if kinds else None
        seen: set[str] = set(seeds)
        queue: deque[tuple[str, int]] = deque((node, 0) for node in seen)
        reached: list[str] = []
        with self._lock:
            while queue:
                node_id, d = queue.popleft()
                if d >= depth:
                    continue
                for edge in self.neighbors(node_id, kinds=wanted, direction=direction):
                    target = edge.target if direction != "in" else edge.source
                    if target in seen:
                        continue
                    seen.add(target)
                    reached.append(target)
                    queue.append((target, d + 1))
        return tuple(reached)

    # ── serialisation ──────────────────────────────────────────────────

    def to_json(self) -> dict[str, Any]:
        with self._lock:
            return {
                "nodes": list(sorted(self._nodes)),
                "edges": [
                    {
                        "source": s,
                        "target": t,
                        "kind": k.value,
                    }
                    for (s, t, k) in sorted(self._edges)
                ],
            }

    @classmethod
    def from_json(cls, payload: Mapping[str, Any]) -> CapabilityGraph:
        graph = cls()
        for node_id in payload.get("nodes", ()):
            graph.add_node(str(node_id))
        for entry in payload.get("edges", ()):
            graph.add_edge(
                CapabilityEdge(
                    source=str(entry["source"]),
                    target=str(entry["target"]),
                    kind=EdgeKind.parse(entry["kind"]),
                    weight=float(entry.get("weight", 1.0)),
                    notes=str(entry.get("notes", "")),
                )
            )
        return graph

    # ── stats ──────────────────────────────────────────────────────────

    def __len__(self) -> int:
        with self._lock:
            return len(self._nodes)

    def edge_count(self) -> int:
        with self._lock:
            return len(self._edges)
