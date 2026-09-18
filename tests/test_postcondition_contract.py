"""Framework-level post-condition readback contract (issue #2260).

The evaluation behind issue #2260 was dominated by tools returning
``success=true`` while doing nothing observable. These tests lock the contract
that turns that silent class of failure into either a loud failure or an
explicit ``verified: false``.
"""

from __future__ import annotations

from collections import deque
import json

import pytest

import dcc_mcp_core
from dcc_mcp_core.runtime.postcondition import POSTCONDITION_SCHEMA_VERSION
from dcc_mcp_core.runtime.postcondition import POSTCONDITION_UNVERIFIED
from dcc_mcp_core.runtime.postcondition import PRESENT
from dcc_mcp_core.runtime.postcondition import UNVERIFIED_FAIL
from dcc_mcp_core.runtime.postcondition import UNVERIFIED_WARN
from dcc_mcp_core.runtime.postcondition import ChangedFrom
from dcc_mcp_core.runtime.postcondition import PostconditionCheck
from dcc_mcp_core.runtime.postcondition import PostconditionError
from dcc_mcp_core.runtime.postcondition import apply_postcondition
from dcc_mcp_core.runtime.postcondition import changed_from
from dcc_mcp_core.runtime.postcondition import verify_postcondition
from dcc_mcp_core.runtime.postcondition import with_postcondition
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success

#: Mirrors ``postcondition._MAX_ITEMS`` / ``_MAX_STRING_CHARS``; kept local so
#: the contract is asserted against the documented bound, not the private name.
MAX_EVIDENCE_ITEMS = 16
MAX_EVIDENCE_CHARS = 256


def _success(**context):
    return skill_success("Texture assigned", slot="normalCamera", **context)


def test_postcondition_contract_is_top_level_exported() -> None:
    assert dcc_mcp_core.PostconditionCheck is PostconditionCheck
    assert dcc_mcp_core.PostconditionError is PostconditionError
    assert dcc_mcp_core.verify_postcondition is verify_postcondition
    assert dcc_mcp_core.with_postcondition is with_postcondition
    assert dcc_mcp_core.changed_from is changed_from


def test_readback_confirms_a_real_change() -> None:
    evidence = verify_postcondition(
        PostconditionCheck(
            "material_slot_readback",
            read=lambda: "/maps/helmet_normal.png",
            expected=changed_from(None),
        ),
    )

    assert evidence["verified"] is True
    assert evidence["method"] == "material_slot_readback"
    assert evidence["schema_version"] == POSTCONDITION_SCHEMA_VERSION
    assert evidence["checks"][0]["actual"] == "/maps/helmet_normal.png"
    json.dumps(evidence)


def test_silent_no_op_is_detected_as_unverified() -> None:
    """The exact #2260 failure mode: slot still unbound after a "success"."""
    check = PostconditionCheck("material_slot_readback", read=lambda: None, expected=changed_from(None))

    outcome = check.evaluate()

    assert outcome.verified is False
    assert outcome.actual is None
    assert outcome.expected == {"changed_from": None}
    assert verify_postcondition(check)["verified"] is False


def test_present_default_requires_a_non_empty_value() -> None:
    assert PostconditionCheck("slot", read=lambda: "bound").evaluate().verified is True
    assert PostconditionCheck("slot", read=lambda: None).evaluate().verified is False
    assert PostconditionCheck("slot", read=lambda: "").evaluate().verified is False
    assert PostconditionCheck("slot", read=lambda: []).evaluate().verified is False
    # Zero is a meaningful read-back value, not an absent one.
    assert PostconditionCheck("frame", read=lambda: 0).evaluate().verified is True


def test_equality_and_custom_comparison() -> None:
    assert PostconditionCheck("parm", read=lambda: 24, expected=24).evaluate().verified is True
    assert PostconditionCheck("parm", read=lambda: 12, expected=24).evaluate().verified is False
    near = PostconditionCheck(
        "vertex_count",
        read=lambda: 1003,
        expected=1000,
        equals=lambda actual, expected: abs(actual - expected) <= 10,
    )
    assert near.evaluate().verified is True


def test_readback_failure_is_reported_not_raised() -> None:
    def boom():
        raise RuntimeError("host query failed")

    outcome = PostconditionCheck("slot", read=boom).evaluate()

    assert outcome.verified is False
    assert outcome.actual is None
    assert outcome.error is not None
    assert "RuntimeError" in outcome.error


def test_comparison_failure_is_reported_not_raised() -> None:
    def boom(actual, expected):
        raise ValueError("ambiguous comparison")

    outcome = PostconditionCheck("slot", read=lambda: 1, equals=boom).evaluate()

    assert outcome.verified is False
    assert outcome.error is not None


def test_every_check_runs_and_evidence_names_the_failures() -> None:
    evidence = verify_postcondition(
        [
            PostconditionCheck("slot_bound", read=lambda: "tex.png", expected=changed_from(None)),
            PostconditionCheck("file_on_disk", read=lambda: None),
        ],
        on_unverified=UNVERIFIED_FAIL,
    )

    assert evidence["verified"] is False
    assert evidence["method"] == "postcondition_readback"
    assert evidence["policy"] == UNVERIFIED_FAIL
    assert [check["verified"] for check in evidence["checks"]] == [True, False]
    assert "expected" not in evidence


def test_warn_policy_marks_success_as_unverified() -> None:
    result = apply_postcondition(
        _success(),
        PostconditionCheck("slot_readback", read=lambda: None),
        on_unverified=UNVERIFIED_WARN,
    )

    assert result["success"] is True
    assert result["postcondition"]["verified"] is False
    assert result["postcondition"]["policy"] == UNVERIFIED_WARN
    assert result["context"]["slot"] == "normalCamera"
    assert "verify" in result["prompt"]


def test_fail_policy_converts_unconfirmed_success_into_a_failure() -> None:
    result = apply_postcondition(
        _success(),
        PostconditionCheck("slot_readback", read=lambda: None),
        on_unverified=UNVERIFIED_FAIL,
    )

    assert result["success"] is False
    assert result["error"] == POSTCONDITION_UNVERIFIED
    assert result["postcondition"]["verified"] is False
    # Diagnostic context survives so the caller can still see what was attempted.
    assert result["context"]["slot"] == "normalCamera"


def test_readback_outranks_a_self_declared_verified_flag() -> None:
    declared = skill_success("Texture assigned", verified=True, postcondition={"method": "claimed"})

    result = apply_postcondition(
        declared,
        PostconditionCheck("slot_readback", read=lambda: None),
        on_unverified=UNVERIFIED_WARN,
    )

    assert result["success"] is True
    assert result["postcondition"]["verified"] is False


def test_verified_success_is_preserved() -> None:
    result = apply_postcondition(
        _success(),
        PostconditionCheck("slot_readback", read=lambda: "tex.png", expected=changed_from(None)),
        on_unverified=UNVERIFIED_FAIL,
    )

    assert result["success"] is True
    assert result["postcondition"]["verified"] is True
    assert result["message"] == "Texture assigned"


def test_failures_and_non_envelopes_pass_through_untouched() -> None:
    failure = skill_error("Slot missing", "not_found", slot="normalCamera")
    check = PostconditionCheck("slot_readback", read=lambda: None)

    assert apply_postcondition(failure, check) is failure
    assert apply_postcondition("not-an-envelope", check) == "not-an-envelope"
    assert apply_postcondition({"success": False}, check) == {"success": False}


def test_decorator_enforces_the_contract_end_to_end() -> None:
    applied = {"slot": None}

    @with_postcondition(
        PostconditionCheck(
            "material_slot_readback",
            read=lambda: applied["slot"],
            expected=changed_from(None),
        ),
        on_unverified=UNVERIFIED_FAIL,
    )
    def assign_texture(path):
        applied["slot"] = path
        return skill_success("Texture assigned", path=path)

    noop = {"slot": None}

    @with_postcondition(
        PostconditionCheck(
            "material_slot_readback",
            read=lambda: noop["slot"],
            expected=changed_from(None),
        ),
        on_unverified=UNVERIFIED_FAIL,
    )
    def assign_texture_noop(path):
        # The #2260 failure mode: reports success, binds nothing.
        return skill_success("Texture assigned", path=path)

    confirmed = assign_texture("/maps/n.png")
    assert confirmed["success"] is True
    assert confirmed["postcondition"]["verified"] is True

    silent = assign_texture_noop("/maps/n.png")
    assert silent["success"] is False
    assert silent["error"] == POSTCONDITION_UNVERIFIED


def test_decorator_leaves_handler_metadata_and_failures_alone() -> None:
    @with_postcondition(PostconditionCheck("slot_readback", read=lambda: None))
    def failing():
        return skill_error("No selection", "invalid_input")

    assert failing()["error"] == "invalid_input"
    assert failing.__name__ == "failing"


def test_evidence_is_bounded_and_redacted() -> None:
    check = PostconditionCheck(
        "node_dump",
        read=lambda: {
            "api_key": "super-secret",
            "name": "x" * 500,
            "children": list(range(100)),
            "nested": {"a": {"b": {"c": {"d": {"e": 1}}}}},
        },
    )

    evidence = verify_postcondition(check)
    actual = evidence["actual"]

    assert actual["api_key"] == "<redacted>"
    assert len(actual["name"]) <= 260
    assert actual["children"][-1] == "<truncated>"
    assert isinstance(actual["nested"]["a"]["b"]["c"], str)
    json.dumps(evidence)


def test_unserializable_readback_values_degrade_to_repr() -> None:
    class HostObject:
        def __repr__(self):
            return "<host object>"

    outcome = PostconditionCheck("slot", read=lambda: HostObject()).evaluate()

    assert outcome.verified is True
    assert outcome.actual == "<host object>"
    json.dumps(outcome.to_dict())


def test_invalid_contract_inputs_are_rejected() -> None:
    with pytest.raises(TypeError):
        PostconditionCheck("", read=lambda: 1)
    with pytest.raises(TypeError):
        PostconditionCheck("slot", read="not-callable")
    with pytest.raises(TypeError):
        PostconditionCheck("slot", read=lambda: 1, equals="not-callable")
    with pytest.raises(TypeError):
        PostconditionCheck("slot", read=lambda: 1, description=42)


def test_missing_or_invalid_policy_fails_closed() -> None:
    with pytest.raises(PostconditionError) as exc_info:
        verify_postcondition([])
    assert exc_info.value.code == "postcondition_check_missing"

    with pytest.raises(PostconditionError) as exc_info:
        verify_postcondition(PostconditionCheck("slot", read=lambda: 1), on_unverified="ignore")
    assert exc_info.value.code == "postcondition_policy_invalid"

    with pytest.raises(PostconditionError):
        with_postcondition(on_unverified=UNVERIFIED_WARN)


def test_non_conforming_envelope_is_reported_not_swallowed() -> None:
    with pytest.raises(PostconditionError) as exc_info:
        apply_postcondition({"success": True, "message": 42}, PostconditionCheck("slot", read=lambda: 1))
    assert exc_info.value.code == "postcondition_envelope_invalid"


def test_changed_from_and_present_are_distinct_expectations() -> None:
    assert isinstance(changed_from(None), ChangedFrom)
    assert PRESENT is not changed_from(None)
    with pytest.raises(TypeError):
        bool(PRESENT)


def test_decorator_rejects_an_invalid_policy_before_the_handler_runs() -> None:
    """Policy errors must fail at import time, not after host state changed."""
    calls = []

    def assign_texture(path):
        calls.append(path)
        return skill_success("Texture assigned", path=path)

    check = PostconditionCheck("slot_readback", read=lambda: "tex.png")

    with pytest.raises(PostconditionError) as exc_info:
        with_postcondition(check, on_unverified="ignore")(assign_texture)
    assert exc_info.value.code == "postcondition_policy_invalid"
    assert calls == []

    # The same contract through ``verify_postcondition`` keeps the same code.
    with pytest.raises(PostconditionError) as exc_info:
        verify_postcondition(check, on_unverified="ignore")
    assert exc_info.value.code == "postcondition_policy_invalid"


def test_process_control_exceptions_propagate_instead_of_becoming_evidence() -> None:
    """``KeyboardInterrupt``/``SystemExit`` are cancellations, not verdicts."""

    def interrupted():
        raise KeyboardInterrupt

    with pytest.raises(KeyboardInterrupt):
        PostconditionCheck("slot", read=interrupted).evaluate()
    with pytest.raises(KeyboardInterrupt):
        verify_postcondition(PostconditionCheck("slot", read=interrupted))

    def exiting_comparison(actual, expected):
        raise SystemExit(3)

    with pytest.raises(SystemExit):
        PostconditionCheck("slot", read=lambda: 1, equals=exiting_comparison).evaluate()


def test_checks_may_be_any_non_string_sequence() -> None:
    check = PostconditionCheck("slot", read=lambda: "tex.png")

    assert verify_postcondition((check,))["verified"] is True
    assert verify_postcondition(deque([check]))["verified"] is True
    # A bare string is a sequence but never a check list.
    with pytest.raises(TypeError):
        verify_postcondition("slot_readback")
    with pytest.raises(TypeError):
        verify_postcondition(b"slot_readback")


def test_bounded_evidence_does_not_walk_a_large_collection() -> None:
    """Truncation must stay lazy: the host value is never fully materialized."""
    consumed = []

    class HostSet(set):
        def __iter__(self):
            for item in super().__iter__():
                consumed.append(item)
                yield item

    huge = HostSet(range(5000))

    evidence = verify_postcondition(PostconditionCheck("node_dump", read=lambda: huge))
    actual = evidence["actual"]

    assert actual[-1] == "<truncated>"
    assert len(actual) == MAX_EVIDENCE_ITEMS + 1
    # 16 kept entries plus the single look-ahead that detects truncation.
    assert len(consumed) == MAX_EVIDENCE_ITEMS + 1
    json.dumps(evidence)


def test_bounded_evidence_truncates_pathological_mapping_keys() -> None:
    evidence = verify_postcondition(
        PostconditionCheck("node_dump", read=lambda: {"k" * 5000: "value", "api_token": "secret"}),
    )
    actual = evidence["actual"]

    long_key = next(key for key in actual if key.startswith("kkk"))
    assert len(long_key) <= MAX_EVIDENCE_CHARS + 3
    assert actual[long_key] == "value"
    assert actual["api_token"] == "<redacted>"
    json.dumps(evidence)
