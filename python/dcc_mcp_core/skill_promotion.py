"""Advisory skill-promotion proposals for repeated escape-hatch scripts.

Escape-hatch tools (``execute_python``, host script eval, …) let an agent run
arbitrary code. When the *same* materialized script keeps showing up, that is
evidence a typed, reviewable skill should exist instead.

This module turns the source-free aggregation produced by
:meth:`dcc_mcp_core.observability_query.ObservabilityQuery.get_repeated_scripts`
into a structured, agent-visible :class:`SkillPromotionProposal` payload.

Scope (advisory only):

* It never persists a table, never writes a skill file, never opens a PR, and
  never publishes anything. A human or a downstream reviewer decides.
* Script *source* is deliberately out of scope. Identity comes from the
  redaction-safe ``script_execution`` telemetry
  (``crates/dcc-mcp-gateway-admin/src/domain/trace.rs``), which stores only a
  strict 64-hex ``sha256`` plus an optional ``reuse_key``.

Compatible with Python 3.7 (Maya 2022 and other embedded DCC hosts).
"""

from __future__ import annotations

from dataclasses import asdict
from dataclasses import dataclass
import hashlib
import json
import re
from typing import Any

__all__ = [
    "DECISION_MANUAL_REVIEW",
    "DECISION_PROPOSE_SKILL",
    "DEFAULT_PROMOTION_THRESHOLD",
    "RECOMMENDED_ACTION_REVIEW_ONLY",
    "SkillPromotionProposal",
    "build_skill_promotion_proposal",
    "build_skill_promotion_proposals",
    "candidate_id_for_evidence",
    "suggest_skill_name",
]

#: Minimum distinct executions before a repeated script is proposed as a skill.
DEFAULT_PROMOTION_THRESHOLD = 3

#: Decision emitted when a grouping meets the promotion threshold.
DECISION_PROPOSE_SKILL = "propose_skill"

#: Decision emitted when a grouping is below the promotion threshold.
DECISION_MANUAL_REVIEW = "manual_review"

#: Recommended action for every proposal — always a human decision.
RECOMMENDED_ACTION_REVIEW_ONLY = "human_review_only"

_SKILL_NAME_PREFIX = "promoted"
_NON_SLUG_RE = re.compile(r"[^a-z0-9]+")
_MAX_SKILL_NAME_SLUG = 24

# HARD CONSTRAINT: raising this constant is not enough by itself. The 8-hex
# tail is only safe because a proposal is advisory — a human reads the name,
# and two digests colliding in 8 hex chars is a review nuisance, not a silent
# overwrite. If skill directories are ever CREATED AUTOMATICALLY from this
# name, the tail MUST become the full 64-hex digest first: a truncated tail
# would let two different scripts collide onto one directory and silently
# clobber each other's files.
_SHA256_DIGEST_CHARS = 8


@dataclass(frozen=True)
class SkillPromotionProposal:
    """Structured, advisory proposal to promote one repeated script to a skill.

    Instances are immutable and carry no script source, no prompt text, and no
    file path — only the redaction-safe identity and the aggregation counters
    needed to justify a review.

    Attributes:
        candidate_id: Stable identity for the grouping; identical inputs always
            produce the same value (see :func:`candidate_id_for_evidence`).
        sha256: Lower-cased content hash of the materialized script.
        dcc_type: DCC the script ran in, when the audit row recorded one.
        tool_name: Escape-hatch tool that ran the script, when recorded.
        execution_count: Number of executions observed for this grouping.
        session_count: Number of distinct sessions observed.
        first_seen_ms: Earliest audit timestamp in the window.
        last_seen_ms: Latest audit timestamp in the window.
        threshold: Promotion threshold this proposal was evaluated against.
        suggested_skill_name: Deterministic, slug-safe name suggestion.

    """

    candidate_id: str
    sha256: str
    dcc_type: str | None
    tool_name: str | None
    execution_count: int
    session_count: int
    first_seen_ms: int
    last_seen_ms: int
    threshold: int
    suggested_skill_name: str

    @property
    def meets_threshold(self) -> bool:
        """Return ``True`` when the grouping reached the promotion threshold."""
        return self.execution_count >= self.threshold

    @property
    def decision(self) -> str:
        """Return the advisory decision for this proposal.

        ``"propose_skill"`` once :attr:`execution_count` reaches
        :attr:`threshold`; ``"manual_review"`` below it. The decision never
        triggers a side effect — it is a recommendation for a reviewer.
        """
        return DECISION_PROPOSE_SKILL if self.meets_threshold else DECISION_MANUAL_REVIEW

    @property
    def recommended_action(self) -> str:
        """Return the recommended action — always a human decision."""
        return RECOMMENDED_ACTION_REVIEW_ONLY

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-serializable payload including the derived decision."""
        payload = asdict(self)
        payload["decision"] = self.decision
        payload["recommended_action"] = self.recommended_action
        payload["meets_threshold"] = self.meets_threshold
        return payload


def _slug(value: Any) -> str:
    """Return a lowercase, hyphen-separated slug for a free-form label."""
    text = str(value or "").strip().lower()
    slug = _NON_SLUG_RE.sub("-", text).strip("-")
    if len(slug) > _MAX_SKILL_NAME_SLUG:
        slug = slug[:_MAX_SKILL_NAME_SLUG].rstrip("-")
    return slug


def _as_int(value: Any) -> int:
    """Best-effort integer coercion that never raises for audit-row values."""
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def suggest_skill_name(sha256: Any, dcc_type: Any = None, tool_name: Any = None) -> str:
    """Return a deterministic, slug-safe skill-name suggestion.

    The name is derived only from redaction-safe identity, so the same grouping
    always yields the same suggestion and no script content leaks into it.

    Example:
        >>> suggest_skill_name("a" * 64, "maya", "execute_python")
        'promoted-maya-execute-python-aaaaaaaa'

    """
    digest = str(sha256 or "").strip().lower()
    parts = [_SKILL_NAME_PREFIX, _slug(dcc_type), _slug(tool_name)]
    tail = digest[:_SHA256_DIGEST_CHARS] if digest else "unknown"
    return "-".join([part for part in parts if part] + [tail])


def candidate_id_for_evidence(row: dict[str, Any]) -> str:
    """Return a stable candidate identity for one repeated-script grouping.

    A content hash alone is not unique because the same materialized script can
    be reused by multiple tools, DCCs, or reuse keys. Canonical JSON keeps the
    identity deterministic while the digest bounds its size.
    """
    identity = {
        "sha256": str(row.get("sha256", "")),
        "reuse_key": row.get("reuse_key"),
        "dcc_type": row.get("dcc_type"),
        "tool_name": row.get("tool_name"),
    }
    canonical = json.dumps(identity, sort_keys=True, separators=(",", ":"))
    return f"script:{hashlib.sha256(canonical.encode('utf-8')).hexdigest()}"


def build_skill_promotion_proposal(
    row: dict[str, Any],
    *,
    threshold: int = DEFAULT_PROMOTION_THRESHOLD,
) -> SkillPromotionProposal:
    """Build one advisory proposal from a repeated-script evidence row.

    Args:
        row: Normalized evidence row — the shape returned by
            ``ObservabilityQuery.get_repeated_scripts()["data"]["scripts"]``
            or a raw aggregation row from the audit table.
        threshold: Executions required before the proposal is promoted from
            ``"manual_review"`` to ``"propose_skill"``.

    Returns:
        An immutable :class:`SkillPromotionProposal`.

    Raises:
        ValueError: If ``threshold`` is not a positive integer.

    """
    # ``bool`` is an ``int`` subclass, so it is excluded explicitly. Every
    # other non-``int`` (1.9, "3", …) is rejected here instead of being
    # silently truncated by ``int()`` below: truncation would promote a
    # grouping earlier than the caller configured (1.9 would become 1).
    if isinstance(threshold, bool) or not isinstance(threshold, int):
        raise ValueError("threshold must be a positive integer")
    if threshold < 1:
        raise ValueError("threshold must be a positive integer")
    sha256 = str(row.get("sha256", "")).strip().lower()
    dcc_type = row.get("dcc_type")
    tool_name = row.get("tool_name")
    return SkillPromotionProposal(
        candidate_id=candidate_id_for_evidence(row),
        sha256=sha256,
        dcc_type=dcc_type if isinstance(dcc_type, str) else None,
        tool_name=tool_name if isinstance(tool_name, str) else None,
        execution_count=_as_int(row.get("execution_count")),
        session_count=_as_int(row.get("session_count")),
        first_seen_ms=_as_int(row.get("first_seen_ms")),
        last_seen_ms=_as_int(row.get("last_seen_ms")),
        threshold=int(threshold),
        suggested_skill_name=suggest_skill_name(sha256, dcc_type, tool_name),
    )


def build_skill_promotion_proposals(
    rows: list[dict[str, Any]],
    *,
    threshold: int = DEFAULT_PROMOTION_THRESHOLD,
) -> list[SkillPromotionProposal]:
    """Build one proposal per evidence row, preserving input order."""
    return [build_skill_promotion_proposal(row, threshold=threshold) for row in rows]
