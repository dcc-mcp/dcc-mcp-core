"""Framework-level post-condition readback for mutating tools (issue #2260).

The evaluation that produced issue #2260 found typed tools returning
``success=true`` while doing nothing observable.  :mod:`dcc_mcp_core.result_envelope`
and :func:`dcc_mcp_core.skill.skill_success` can *report* a verification claim
through ``postcondition.verified``, but nothing in the framework performed the
readback, so most tools simply omitted it and stayed silently unverified.

This module closes that gap with one declarative contract:

* :class:`PostconditionCheck` — read one piece of state back and compare it
  against what the mutation claimed to produce.
* :func:`verify_postcondition` — run the checks and return structured evidence
  suitable for ``postcondition``.
* :func:`with_postcondition` — decorate a mutating tool handler so an
  unconfirmed effect fails loudly (``on_unverified="fail"``) or is explicitly
  marked ``verified: false`` (``on_unverified="warn"``) instead of being
  reported as a clean success.

Readback is host-owned and stays adapter-neutral: core never guesses what a
Houdini parm or a 3ds Max material slot looks like, it only standardizes how
the observation is captured, compared, bounded, and reported.

Typical use in an adapter tool::

    from dcc_mcp_core.postcondition import PostconditionCheck, with_postcondition

    @with_postcondition(
        PostconditionCheck(
            "material_slot_readback",
            read=lambda: cmds.getAttr(f"{shader}.normalCamera"),
            expected=changed_from(None),
        ),
        on_unverified="fail",
    )
    def assign_bitmap_texture(...):
        ...
        return skill_success("Texture assigned", slot=slot)

Bounds and redaction here are evidence hygiene, not authentication: a tool can
still lie about what it read.  The contract makes the absence of confirmation
visible in-band so callers stop trusting unconfirmed successes.
"""

from __future__ import annotations

from dataclasses import dataclass
import functools
import json
import math
import re
from typing import Any
from typing import Callable
from typing import Mapping
from typing import Sequence
from typing import Union

from dcc_mcp_core.errors import DccMcpError
from dcc_mcp_core.result_envelope import ToolResultEnvelope

POSTCONDITION_SCHEMA_VERSION = "dcc-mcp.postcondition.v1"

#: Report an unconfirmed effect as a success carrying ``verified: false``.
UNVERIFIED_WARN = "warn"
#: Convert an unconfirmed effect into a failure envelope.
UNVERIFIED_FAIL = "fail"
UNVERIFIED_POLICIES = (UNVERIFIED_WARN, UNVERIFIED_FAIL)

#: Stable error code used when a mutating tool cannot confirm its own effect.
POSTCONDITION_UNVERIFIED = "postcondition_unverified"

_MAX_DEPTH = 4
_MAX_ITEMS = 16
_MAX_STRING_CHARS = 256
_MAX_INTEGER_BITS = 512
_TRUNCATED = "<truncated>"
_REDACTED = "<redacted>"
_SENSITIVE_KEY = re.compile(
    r"(?:api[_-]?key|authorization|credential|password|secret|token)",
    re.IGNORECASE,
)


class PostconditionError(DccMcpError):
    """Fail-closed post-condition contract error with a stable public code."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class _Sentinel:
    """Named marker that cannot collide with a real read-back value."""

    __slots__ = ("_name",)

    def __init__(self, name: str) -> None:
        self._name = name

    def __repr__(self) -> str:
        return self._name

    def __bool__(self) -> bool:
        raise TypeError("post-condition sentinels are not boolean evidence")


#: Default ``expected``: the read-back value must simply be present.
PRESENT = _Sentinel("PRESENT")


@dataclass(frozen=True)
class ChangedFrom:
    """Require the read-back value to differ from *previous*.

    This is the shape that catches "returned success while doing nothing": a
    slot that was unbound before the call must not still be unbound after it.
    """

    previous: Any


def changed_from(previous: Any) -> ChangedFrom:
    """Return an expectation meaning "this value must have moved"."""
    return ChangedFrom(previous)


@dataclass(frozen=True)
class PostconditionOutcome:
    """One read-back observation and the verdict derived from it."""

    method: str
    verified: bool
    expected: Any = None
    actual: Any = None
    error: str | None = None
    description: str = ""

    def to_dict(self) -> dict[str, Any]:
        """Return the JSON-safe evidence for this single check."""
        outcome: dict[str, Any] = {
            "method": self.method,
            "verified": self.verified,
            "expected": self.expected,
            "actual": self.actual,
        }
        if self.error is not None:
            outcome["error"] = self.error
        if self.description:
            outcome["description"] = self.description
        return outcome


@dataclass(frozen=True)
class PostconditionCheck:
    """One declarative read-back assertion for a mutating tool.

    Args:
        method: Stable read-back method name, e.g. ``"material_slot_readback"``.
        read: Callable that reads the changed state back from the host. It is
            invoked once, after the mutation and before the tool returns.
        expected: Required outcome. The :data:`PRESENT` default only requires a
            non-empty value; :func:`changed_from` requires movement; any other
            value requires equality.
        equals: Optional custom comparison ``(actual, expected) -> bool``.
        description: Optional human-readable note carried into the evidence.

    """

    method: str
    read: Callable[[], Any]
    expected: Any = PRESENT
    equals: Callable[[Any, Any], bool] | None = None
    description: str = ""

    def __post_init__(self) -> None:
        if not isinstance(self.method, str) or not self.method:
            raise TypeError("post-condition check method must be a non-empty string")
        if not callable(self.read):
            raise TypeError("post-condition check read must be callable")
        if self.equals is not None and not callable(self.equals):
            raise TypeError("post-condition check equals must be callable or None")
        if not isinstance(self.description, str):
            raise TypeError("post-condition check description must be a string")

    def evaluate(self) -> PostconditionOutcome:
        """Read the state back once and compare it against the expectation."""
        try:
            actual = self.read()
        except BaseException as exc:
            return PostconditionOutcome(
                method=self.method,
                verified=False,
                expected=_expected_evidence(self.expected),
                actual=None,
                error=_error_text(exc),
                description=self.description,
            )
        return self._compare(actual)

    def _compare(self, actual: Any) -> PostconditionOutcome:
        expected_evidence = _expected_evidence(self.expected)
        try:
            if self.equals is not None:
                verified = bool(self.equals(actual, self.expected))
            elif isinstance(self.expected, ChangedFrom):
                verified = _values_differ(actual, self.expected.previous)
            elif isinstance(self.expected, _Sentinel):
                verified = _is_present(actual)
            else:
                verified = _values_equal(actual, self.expected)
        except BaseException as exc:
            return PostconditionOutcome(
                method=self.method,
                verified=False,
                expected=expected_evidence,
                actual=_json_safe(actual),
                error=_error_text(exc),
                description=self.description,
            )
        return PostconditionOutcome(
            method=self.method,
            verified=verified,
            expected=expected_evidence,
            actual=_json_safe(actual),
            description=self.description,
        )


CheckInput = Union[PostconditionCheck, Sequence[PostconditionCheck]]


def verify_postcondition(
    checks: CheckInput,
    *,
    on_unverified: str = UNVERIFIED_WARN,
    method: str | None = None,
) -> dict[str, Any]:
    """Run *checks* and return structured ``postcondition`` evidence.

    Args:
        checks: One :class:`PostconditionCheck` or a sequence of them. Every
            check runs, so a partially-applied mutation reports which parts
            landed instead of stopping at the first mismatch.
        on_unverified: How the caller will treat an unconfirmed effect. It is
            recorded in the evidence so a consumer can tell "not verified and
            that was tolerated" from "not verified and that was an error".
        method: Override the reported evidence method name.

    Returns:
        A JSON-safe mapping with ``schema_version``, ``verified``, ``method``,
        ``policy``, and ``checks``. Single-check evidence also mirrors
        ``expected`` and ``actual`` at the top level for readability.

    """
    normalized = _normalize_checks(checks)
    if not normalized:
        raise PostconditionError(
            "postcondition_check_missing",
            "At least one post-condition check is required",
        )
    if on_unverified not in UNVERIFIED_POLICIES:
        raise PostconditionError(
            "postcondition_policy_invalid",
            f"Unknown post-condition policy: {on_unverified!r}",
        )
    outcomes = [check.evaluate() for check in normalized]
    verified = all(outcome.verified for outcome in outcomes)
    evidence: dict[str, Any] = {
        "schema_version": POSTCONDITION_SCHEMA_VERSION,
        "verified": verified,
        "method": method or _evidence_method(normalized),
        "policy": on_unverified,
        "checks": [outcome.to_dict() for outcome in outcomes],
    }
    if len(outcomes) == 1:
        evidence["expected"] = outcomes[0].expected
        evidence["actual"] = outcomes[0].actual
    _assert_json_serializable(evidence)
    return evidence


def apply_postcondition(
    result: Any,
    checks: CheckInput,
    *,
    on_unverified: str = UNVERIFIED_FAIL,
    message: str | None = None,
) -> Any:
    """Attach read-back evidence to one tool *result* and enforce the policy.

    Non-mapping results and results that are not a success envelope are already
    loud or not envelope-shaped; they are returned untouched. A success whose
    readback does not confirm the change either becomes a
    ``postcondition_unverified`` failure (``on_unverified="fail"``) or keeps
    ``success=true`` with ``postcondition.verified = false`` plus a recovery
    prompt (``on_unverified="warn"``).

    Evidence produced by the checks always wins over a ``verified`` value the
    handler declared itself: an unconfirmed effect must never ship as
    ``verified: true``.
    """
    if not isinstance(result, Mapping) or result.get("success") is not True:
        return result
    evidence = verify_postcondition(checks, on_unverified=on_unverified)
    envelope = _coerce_envelope(result)
    merged = _merge_postcondition(envelope.postcondition, evidence)
    if evidence["verified"]:
        return ToolResultEnvelope(
            success=True,
            message=envelope.message,
            error=None,
            prompt=envelope.prompt,
            context=dict(envelope.context),
            postcondition=merged,
            _meta=dict(envelope._meta),
        ).to_dict(prune_empty=False)
    if on_unverified == UNVERIFIED_FAIL:
        return ToolResultEnvelope(
            success=False,
            message=message or _unverified_message(evidence),
            error=POSTCONDITION_UNVERIFIED,
            prompt=_failure_prompt(evidence),
            context=dict(envelope.context),
            postcondition=merged,
            _meta=dict(envelope._meta),
        ).to_dict(prune_empty=False)
    return ToolResultEnvelope(
        success=True,
        message=envelope.message,
        error=None,
        prompt=envelope.prompt or _warning_prompt(evidence),
        context=dict(envelope.context),
        postcondition=merged,
        _meta=dict(envelope._meta),
    ).to_dict(prune_empty=False)


def with_postcondition(
    *checks: PostconditionCheck,
    on_unverified: str = UNVERIFIED_FAIL,
    message: str | None = None,
) -> Callable[[Callable[..., Any]], Callable[..., Any]]:
    """Decorate a mutating tool handler with post-condition readback.

    The wrapped handler runs first; its envelope is then passed through
    :func:`apply_postcondition`. Failures propagate untouched because an
    exception or an error envelope is already explicit.

    Example::

        @with_postcondition(
            PostconditionCheck(
                "texture_slot_readback",
                read=lambda: read_slot(shader, "normalCamera"),
                expected=changed_from(None),
            ),
            on_unverified="fail",
        )
        def assign_bitmap_texture(shader, path):
            ...
    """
    normalized = _normalize_checks(list(checks))
    if not normalized:
        raise PostconditionError(
            "postcondition_check_missing",
            "At least one post-condition check is required",
        )

    def decorate(func: Callable[..., Any]) -> Callable[..., Any]:
        @functools.wraps(func)
        def wrapper(*args: Any, **kwargs: Any) -> Any:
            return apply_postcondition(
                func(*args, **kwargs),
                normalized,
                on_unverified=on_unverified,
                message=message,
            )

        return wrapper

    return decorate


def _normalize_checks(checks: CheckInput) -> list[PostconditionCheck]:
    if isinstance(checks, PostconditionCheck):
        return [checks]
    if isinstance(checks, (list, tuple)):
        normalized: list[PostconditionCheck] = []
        for check in checks:
            if not isinstance(check, PostconditionCheck):
                raise TypeError("post-condition checks must be PostconditionCheck instances")
            normalized.append(check)
        return normalized
    raise TypeError("post-condition checks must be a PostconditionCheck or a sequence of them")


def _coerce_envelope(result: Mapping[str, Any]) -> ToolResultEnvelope:
    try:
        return ToolResultEnvelope.from_dict(result, strict=False)
    except (TypeError, ValueError) as exc:
        raise PostconditionError(
            "postcondition_envelope_invalid",
            f"Tool result is not a valid envelope: {exc}",
        ) from None


def _merge_postcondition(
    existing: Mapping[str, Any] | None,
    evidence: Mapping[str, Any],
) -> dict[str, Any]:
    merged: dict[str, Any] = dict(existing) if isinstance(existing, Mapping) else {}
    merged.update(evidence)
    declared = merged.get("verified")
    if isinstance(declared, bool) and declared and not evidence.get("verified"):
        # Readback is authoritative: a tool cannot self-certify as verified.
        merged["verified"] = False
    return merged


def _evidence_method(checks: Sequence[PostconditionCheck]) -> str:
    if len(checks) == 1:
        return checks[0].method
    return "postcondition_readback"


def _unverified_message(evidence: Mapping[str, Any]) -> str:
    failed = [str(check.get("method")) for check in evidence.get("checks", []) if not check.get("verified")]
    names = ", ".join(name for name in failed if name)
    suffix = f" ({names})" if names else ""
    return f"Post-condition readback did not confirm the change{suffix}"


def _failure_prompt(evidence: Mapping[str, Any]) -> str:
    return (
        "The tool reported success but readback did not confirm the change; "
        f"treat the mutation as unverified ({evidence.get('method')})."
    )


def _warning_prompt(evidence: Mapping[str, Any]) -> str:
    return (
        f"Post-condition readback could not confirm '{evidence.get('method')}'; "
        "verify the scene state before relying on this result."
    )


def _expected_evidence(expected: Any) -> Any:
    if isinstance(expected, ChangedFrom):
        return {"changed_from": _json_safe(expected.previous)}
    if isinstance(expected, _Sentinel):
        return repr(expected)
    return _json_safe(expected)


def _is_present(actual: Any) -> bool:
    if actual is None:
        return False
    if isinstance(actual, (str, bytes, bytearray, list, tuple, set, frozenset, dict)):
        return len(actual) > 0
    return True


def _values_equal(actual: Any, expected: Any) -> bool:
    result = actual == expected
    if isinstance(result, bool):
        return result
    # Host types (numpy arrays, MaxScript wrappers) may return non-bool here.
    return bool(result)


def _values_differ(actual: Any, previous: Any) -> bool:
    return not _values_equal(actual, previous)


def _error_text(exc: BaseException) -> str:
    return f"{type(exc).__name__}: {exc}"[:_MAX_STRING_CHARS]


def _bounded_repr(value: Any) -> str:
    try:
        text = repr(value)
    except Exception:
        text = f"<unrepresentable {type(value).__name__}>"
    return text if len(text) <= _MAX_STRING_CHARS else text[:_MAX_STRING_CHARS] + "..."


def _json_safe(value: Any, *, depth: int = 0) -> Any:
    """Bound and redact one read-back value before it crosses the wire."""
    if value is None or isinstance(value, bool):
        return value
    if isinstance(value, int):
        return value if abs(value).bit_length() <= _MAX_INTEGER_BITS else _bounded_repr(value)
    if isinstance(value, float):
        return value if math.isfinite(value) else _bounded_repr(value)
    if isinstance(value, str):
        return value if len(value) <= _MAX_STRING_CHARS else value[:_MAX_STRING_CHARS] + "..."
    if depth >= _MAX_DEPTH:
        return _bounded_repr(value)
    if isinstance(value, Mapping):
        out: dict[str, Any] = {}
        for index, (key, item) in enumerate(value.items()):
            if index >= _MAX_ITEMS:
                out["_truncated"] = True
                break
            key_text = str(key)
            out[key_text] = _REDACTED if _SENSITIVE_KEY.search(key_text) else _json_safe(item, depth=depth + 1)
        return out
    if isinstance(value, (list, tuple, set, frozenset)):
        items = list(value)[:_MAX_ITEMS]
        truncated = len(value) > _MAX_ITEMS
        out_list = [_json_safe(item, depth=depth + 1) for item in items]
        if truncated:
            out_list.append(_TRUNCATED)
        return out_list
    return _bounded_repr(value)


def _assert_json_serializable(evidence: Mapping[str, Any]) -> None:
    try:
        json.dumps(evidence, allow_nan=False)
    except (TypeError, ValueError) as exc:
        raise PostconditionError(
            "postcondition_invalid",
            f"Post-condition evidence is not JSON serializable: {exc}",
        ) from None


__all__ = [
    "POSTCONDITION_SCHEMA_VERSION",
    "POSTCONDITION_UNVERIFIED",
    "PRESENT",
    "UNVERIFIED_FAIL",
    "UNVERIFIED_POLICIES",
    "UNVERIFIED_WARN",
    "ChangedFrom",
    "PostconditionCheck",
    "PostconditionError",
    "PostconditionOutcome",
    "apply_postcondition",
    "changed_from",
    "verify_postcondition",
    "with_postcondition",
]
