"""Tests for advisory skill-promotion proposals built from repeated scripts."""

from __future__ import annotations

import json
import sqlite3
from typing import Any

from dcc_mcp_core.observability_query import ObservabilityQuery
from dcc_mcp_core.skill_promotion import DECISION_MANUAL_REVIEW
from dcc_mcp_core.skill_promotion import DECISION_PROPOSE_SKILL
from dcc_mcp_core.skill_promotion import DEFAULT_PROMOTION_THRESHOLD
from dcc_mcp_core.skill_promotion import RECOMMENDED_ACTION_REVIEW_ONLY
from dcc_mcp_core.skill_promotion import SkillPromotionProposal
from dcc_mcp_core.skill_promotion import build_skill_promotion_proposal
from dcc_mcp_core.skill_promotion import build_skill_promotion_proposals
from dcc_mcp_core.skill_promotion import candidate_id_for_evidence
from dcc_mcp_core.skill_promotion import suggest_skill_name

SHA = "a" * 64


def _evidence_row(execution_count: int, **overrides: Any) -> dict[str, Any]:
    row = {
        "sha256": SHA,
        "reuse_key": "asset-builder",
        "dcc_type": "maya",
        "tool_name": "execute_python",
        "execution_count": execution_count,
        "session_count": min(execution_count, 3),
        "first_seen_ms": 1000,
        "last_seen_ms": 1000 * execution_count,
    }
    row.update(overrides)
    return row


def _audit_rows(count: int, *, sha256: str = SHA) -> list[dict[str, Any]]:
    """Build ``count`` aggregated repeated-script rows, as the SQL would."""
    return [
        _evidence_row(
            count,
            session_count=count,
            reused_count=0,
            rematerialized_count=count,
            first_seen_ms=1000,
            last_seen_ms=1000 * count,
            sha256=sha256,
        )
    ]


class TestSuggestSkillName:
    """Skill-name suggestions are deterministic and slug-safe."""

    def test_includes_dcc_tool_and_short_digest(self) -> None:
        name = suggest_skill_name(SHA, "maya", "execute_python")

        assert name == "promoted-maya-execute-python-aaaaaaaa"

    def test_is_deterministic(self) -> None:
        assert suggest_skill_name(SHA, "maya", "execute_python") == suggest_skill_name(SHA, "maya", "execute_python")

    def test_omits_missing_dimensions(self) -> None:
        name = suggest_skill_name(SHA, None, None)

        assert name == "promoted-aaaaaaaa"

    def test_normalizes_free_form_labels(self) -> None:
        name = suggest_skill_name(SHA, "Houdini 20.5", "Execute Python!!")

        assert " " not in name
        assert name.startswith("promoted-houdini-20-5-execute-python")

    def test_survives_empty_sha256(self) -> None:
        assert suggest_skill_name("", "maya") == "promoted-maya-unknown"


class TestCandidateId:
    """Candidate identity is stable for identical inputs."""

    def test_stable_across_calls(self) -> None:
        row = _evidence_row(4)

        assert candidate_id_for_evidence(row) == candidate_id_for_evidence(dict(row))

    def test_differs_by_grouping_dimension(self) -> None:
        base = candidate_id_for_evidence(_evidence_row(4))
        other_tool = candidate_id_for_evidence(_evidence_row(4, tool_name="execute_mel"))
        other_dcc = candidate_id_for_evidence(_evidence_row(4, dcc_type="houdini"))
        other_key = candidate_id_for_evidence(_evidence_row(4, reuse_key="layout-export"))

        assert len({base, other_tool, other_dcc, other_key}) == 4

    def test_ignores_counter_changes(self) -> None:
        """Identity covers the grouping, so counters must not move it."""
        assert candidate_id_for_evidence(_evidence_row(3)) == candidate_id_for_evidence(_evidence_row(9))


class TestBuildProposal:
    """Threshold logic for a single evidence row."""

    def test_meets_threshold_proposes_skill(self) -> None:
        proposal = build_skill_promotion_proposal(_evidence_row(3))

        assert proposal.decision == DECISION_PROPOSE_SKILL
        assert proposal.meets_threshold is True
        assert proposal.threshold == DEFAULT_PROMOTION_THRESHOLD
        assert proposal.recommended_action == RECOMMENDED_ACTION_REVIEW_ONLY

    def test_below_threshold_is_manual_review(self) -> None:
        proposal = build_skill_promotion_proposal(_evidence_row(2))

        assert proposal.decision == DECISION_MANUAL_REVIEW
        assert proposal.meets_threshold is False

    def test_custom_threshold(self) -> None:
        proposal = build_skill_promotion_proposal(_evidence_row(4), threshold=5)

        assert proposal.decision == DECISION_MANUAL_REVIEW
        assert proposal.threshold == 5

    def test_is_frozen(self) -> None:
        proposal = build_skill_promotion_proposal(_evidence_row(3))

        try:
            proposal.execution_count = 99  # type: ignore[misc]
        except Exception:
            return
        raise AssertionError("SkillPromotionProposal must be immutable")

    def test_payload_shape(self) -> None:
        payload = build_skill_promotion_proposal(_evidence_row(3)).to_dict()

        assert set(payload) == {
            "candidate_id",
            "sha256",
            "dcc_type",
            "tool_name",
            "execution_count",
            "session_count",
            "first_seen_ms",
            "last_seen_ms",
            "threshold",
            "suggested_skill_name",
            "decision",
            "recommended_action",
            "meets_threshold",
        }
        json.dumps(payload)

    def test_rejects_invalid_threshold(self) -> None:
        for bad in (0, -1, True):
            try:
                build_skill_promotion_proposal(_evidence_row(3), threshold=bad)
            except ValueError:
                continue
            raise AssertionError(f"threshold={bad!r} should be rejected")

    def test_builds_one_proposal_per_row(self) -> None:
        proposals = build_skill_promotion_proposals([_evidence_row(3), _evidence_row(1)])

        assert [proposal.decision for proposal in proposals] == [
            DECISION_PROPOSE_SKILL,
            DECISION_MANUAL_REVIEW,
        ]

    def test_is_instance_of_dataclass(self) -> None:
        assert isinstance(build_skill_promotion_proposal(_evidence_row(3)), SkillPromotionProposal)


class TestQueryIntegration:
    """Proposals flow through ``get_repeated_scripts`` with a fake read_fn."""

    def test_repeated_rows_propose_skill(self) -> None:
        def read(_sql: str, _params: dict[str, Any]) -> list[dict[str, Any]]:
            return _audit_rows(4)

        response = ObservabilityQuery(read_json_fn=read).get_repeated_scripts(min_repeats=3)

        proposal = response["data"]["skill_promotion_proposals"][0]
        assert proposal["decision"] == DECISION_PROPOSE_SKILL
        assert proposal["execution_count"] == 4
        assert proposal["suggested_skill_name"] == "promoted-maya-execute-python-aaaaaaaa"
        assert response["data"]["promotion_candidates"][0]["proposal"]["decision"] == DECISION_PROPOSE_SKILL

    def test_candidate_id_is_stable_for_same_input(self) -> None:
        def read(_sql: str, _params: dict[str, Any]) -> list[dict[str, Any]]:
            return _audit_rows(3)

        first = ObservabilityQuery(read_json_fn=read).get_repeated_scripts(min_repeats=3)
        second = ObservabilityQuery(read_json_fn=read).get_repeated_scripts(min_repeats=3)

        assert (
            first["data"]["skill_promotion_proposals"][0]["candidate_id"]
            == second["data"]["skill_promotion_proposals"][0]["candidate_id"]
        )

    def test_below_threshold_is_manual_review(self) -> None:
        """A caller can surface sub-threshold groupings via promotion_threshold."""

        def read(_sql: str, _params: dict[str, Any]) -> list[dict[str, Any]]:
            return _audit_rows(3)

        response = ObservabilityQuery(read_json_fn=read).get_repeated_scripts(
            min_repeats=2,
            promotion_threshold=5,
        )

        proposal = response["data"]["skill_promotion_proposals"][0]
        assert proposal["decision"] == DECISION_MANUAL_REVIEW
        assert proposal["threshold"] == 5
        assert proposal["meets_threshold"] is False

    def test_legacy_decision_keys_are_unchanged(self) -> None:
        def read(_sql: str, _params: dict[str, Any]) -> list[dict[str, Any]]:
            return _audit_rows(4)

        response = ObservabilityQuery(read_json_fn=read).get_repeated_scripts(min_repeats=3)

        candidate = response["data"]["promotion_candidates"][0]
        assert candidate["decision"] == DECISION_MANUAL_REVIEW
        assert candidate["recommended_action"] == RECOMMENDED_ACTION_REVIEW_ONLY

    def test_rejects_invalid_promotion_threshold(self) -> None:
        query = ObservabilityQuery(read_json_fn=lambda _sql, _params: [])

        for bad in (0, -1, True):
            try:
                query.get_repeated_scripts(promotion_threshold=bad)
            except ValueError:
                continue
            raise AssertionError(f"promotion_threshold={bad!r} should be rejected")

    def test_aggregates_sqlite_audit_rows_end_to_end(self) -> None:
        connection = sqlite3.connect(":memory:")
        connection.row_factory = sqlite3.Row
        connection.execute("CREATE TABLE audits (request_id TEXT PRIMARY KEY, ts_ms INTEGER, audit_json TEXT)")
        for index, reused in enumerate((False, False, True), start=1):
            audit = {
                "session_id": f"session-{index}",
                "action": "execute_python",
                "dcc_type": "maya",
                "script_execution": {"sha256": "b" * 64, "reused": reused, "reuse_key": "layout-export"},
            }
            connection.execute(
                "INSERT INTO audits VALUES (?, ?, ?)",
                (f"request-{index}", index * 1000, json.dumps(audit)),
            )

        def read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            return [dict(row) for row in connection.execute(sql, params).fetchall()]

        response = ObservabilityQuery(read_json_fn=read).get_repeated_scripts(min_repeats=3)

        proposal = response["data"]["skill_promotion_proposals"][0]
        assert proposal["execution_count"] == 3
        assert proposal["session_count"] == 3
        assert proposal["decision"] == DECISION_PROPOSE_SKILL
        assert proposal["sha256"] == "b" * 64
        assert proposal["suggested_skill_name"] == "promoted-maya-execute-python-bbbbbbbb"

    def test_no_script_source_leaks_into_payload(self) -> None:
        def read(_sql: str, _params: dict[str, Any]) -> list[dict[str, Any]]:
            return _audit_rows(3)

        response = ObservabilityQuery(read_json_fn=read).get_repeated_scripts(min_repeats=3)

        blob = json.dumps(response).lower()
        for forbidden in ("source", "script_body", "code"):
            assert forbidden not in blob
