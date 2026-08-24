"""Canonical, versioned feedback findings shared across DCC-MCP runtimes."""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
from pathlib import Path
import re
from typing import Any

from dcc_mcp_core.skills_helper import json_dumps
from dcc_mcp_core.skills_helper import json_loads

FINDING_V1_SCHEMA_VERSION = 1

_MAX_ID_CHARS = 256
_MAX_TEXT_CHARS = 4096
_MAX_REPRO_ITEMS = 64
_MAX_EVIDENCE_BYTES = 32 * 1024
_PHASES = {"install", "startup", "dispatch", "skill", "other"}
_SEVERITIES = {"blocker", "degraded", "workaround_found", "suggestion"}
_AGENT_FIELDS = {
    "phase",
    "severity",
    "tool_slug",
    "intent",
    "observed",
    "expected",
    "repro",
    "evidence",
}
_KNOWN_EVIDENCE_FIELDS = {"request_id", "job_id", "instance_id", "error_kind", "run_id"}
_REDACTION_MARKER = re.compile(r"(?:\[|<|\{)\s*redacted\s*(?:\]|>|\})", re.IGNORECASE)


class FindingValidationError(ValueError):
    """Raised when a finding cannot satisfy the bounded v1 contract."""


@dataclass(frozen=True)
class FindingRuntimeContext:
    """Runtime-owned identity fields that agents must not self-report."""

    dcc_type: str
    adapter: str
    adapter_version: str
    core_version: str
    host_version: str
    os: str
    owning_repo: str


def finding_v1_json_schema() -> dict[str, Any]:
    """Load and return the packaged Finding v1 JSON Schema."""
    schema_path = Path(__file__).with_name("feedback-finding-v1.schema.json")
    payload = json_loads(schema_path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise FindingValidationError("Finding v1 schema must be a JSON object")
    return payload


def finding_tool_input_schema() -> dict[str, Any]:
    """Return the agent-authored subset of the canonical schema."""
    canonical = finding_v1_json_schema()
    properties = canonical.get("properties", {})
    return {
        "type": "object",
        "properties": {name: properties[name] for name in sorted(_AGENT_FIELDS)},
        "required": ["phase", "severity", "intent", "observed", "expected", "repro"],
        "anyOf": [
            {"required": ["tool_slug"]},
            {
                "required": ["evidence"],
                "properties": {"evidence": {"required": ["error_kind"]}},
            },
        ],
        "additionalProperties": False,
    }


def finding_fingerprint(
    *,
    owning_repo: str,
    phase: str,
    tool_slug: str | None,
    error_kind: str | None,
    host_version: str,
) -> str:
    """Return the stable v1 hash of owner, phase, subject, and host major."""
    owner = _normalize_owner(owning_repo)
    normalized_phase = _required_choice("phase", phase, _PHASES)
    subject_value = tool_slug if isinstance(tool_slug, str) and tool_slug.strip() else error_kind
    subject = _required_text("tool_slug or evidence.error_kind", subject_value, _MAX_ID_CHARS).lower()
    host = _required_text("host_version", host_version, _MAX_ID_CHARS)
    match = re.search(r"[0-9]+", host)
    host_major = match.group(0) if match else "unknown"
    basis = f"finding-v1\n{owner}\n{normalized_phase}\n{subject}\n{host_major}"
    return "sha256:" + hashlib.sha256(basis.encode("utf-8")).hexdigest()


def build_finding_v1(args: dict[str, Any], context: FindingRuntimeContext) -> dict[str, Any]:
    """Validate agent input, auto-fill runtime identity, and build Finding v1."""
    if not isinstance(args, dict):
        raise FindingValidationError("Finding input must be an object")
    unknown = sorted(set(args) - _AGENT_FIELDS)
    if unknown:
        raise FindingValidationError("Unknown finding fields: {}".format(", ".join(unknown)))

    runtime = {
        "dcc_type": _required_text("dcc_type", context.dcc_type, _MAX_ID_CHARS),
        "adapter": _required_text("adapter", context.adapter, _MAX_ID_CHARS),
        "adapter_version": _required_text("adapter_version", context.adapter_version, _MAX_ID_CHARS),
        "core_version": _required_text("core_version", context.core_version, _MAX_ID_CHARS),
        "host_version": _required_text("host_version", context.host_version, _MAX_ID_CHARS),
        "os": _required_text("os", context.os, _MAX_ID_CHARS),
    }
    owner = _normalize_owner(context.owning_repo)
    phase = _required_choice("phase", args.get("phase"), _PHASES)
    severity = _required_choice("severity", args.get("severity"), _SEVERITIES)
    tool_slug = _optional_text("tool_slug", args.get("tool_slug"), _MAX_ID_CHARS)
    intent = _required_text("intent", args.get("intent"), _MAX_TEXT_CHARS)
    observed = _required_text("observed", args.get("observed"), _MAX_TEXT_CHARS)
    expected = _required_text("expected", args.get("expected"), _MAX_TEXT_CHARS)
    repro = _validate_repro(args.get("repro"))
    evidence = _validate_evidence(args.get("evidence", {}))
    error_kind = evidence.get("error_kind")
    if tool_slug is None and error_kind is None:
        raise FindingValidationError("tool_slug or evidence.error_kind is required")

    marker_payload = json_dumps(
        {"intent": intent, "observed": observed, "expected": expected, "repro": repro, "evidence": evidence}
    )
    finding: dict[str, Any] = {
        "schema_version": FINDING_V1_SCHEMA_VERSION,
        "fingerprint": finding_fingerprint(
            owning_repo=owner,
            phase=phase,
            tool_slug=tool_slug,
            error_kind=error_kind,
            host_version=runtime["host_version"],
        ),
        **runtime,
        "phase": phase,
        "severity": severity,
        "intent": intent,
        "observed": observed,
        "expected": expected,
        "repro": repro,
        "evidence": evidence,
        "redaction_status": {
            "mode": "needs-review",
            "redaction_markers_detected": _REDACTION_MARKER.search(marker_payload) is not None,
        },
    }
    if tool_slug is not None:
        finding["tool_slug"] = tool_slug
    return finding


def normalize_legacy_feedback(args: dict[str, Any]) -> dict[str, Any]:
    """Convert the original compatibility-tool input into agent Finding fields."""
    allowed = {"tool_name", "intent", "attempt", "blocker", "severity", "request_id", "job_id"}
    unknown = sorted(set(args) - allowed)
    if unknown:
        raise FindingValidationError("Unknown legacy feedback fields: {}".format(", ".join(unknown)))
    tool_slug = _required_text("tool_name", args.get("tool_name"), _MAX_ID_CHARS)
    intent = _required_text("intent", args.get("intent"), _MAX_TEXT_CHARS)
    observed = _required_text("blocker", args.get("blocker"), _MAX_TEXT_CHARS)
    severity = args.get("severity")
    severity_map = {"blocked": "blocker", "workaround_found": "workaround_found", "suggestion": "suggestion"}
    if severity not in severity_map:
        raise FindingValidationError("severity must be blocked, workaround_found, or suggestion")
    attempt = _optional_text("attempt", args.get("attempt"), _MAX_TEXT_CHARS)
    step = attempt or f"Invoke {tool_slug} for the stated intent"
    evidence: dict[str, Any] = {"error_kind": "legacy_feedback"}
    for name in ("request_id", "job_id"):
        value = _optional_text(name, args.get(name), _MAX_ID_CHARS)
        if value is not None:
            evidence[name] = value
    return {
        "phase": "dispatch",
        "severity": severity_map[severity],
        "tool_slug": tool_slug,
        "intent": intent,
        "observed": observed,
        "expected": f"Complete the stated intent: {intent}",
        "repro": {"steps": [step]},
        "evidence": evidence,
    }


def _normalize_owner(value: Any) -> str:
    owner = _required_text("owning_repo", value, _MAX_ID_CHARS).replace("\\", "/").lower()
    for prefix in ("https://github.com/", "http://github.com/", "github.com/"):
        if owner.startswith(prefix):
            owner = owner[len(prefix) :]
            break
    owner = owner.rstrip("/")
    if owner.endswith(".git"):
        owner = owner[:-4]
    owner = owner.rstrip("/")
    if not owner:
        raise FindingValidationError("owning_repo must not be empty")
    return owner


def _required_text(name: str, value: Any, max_chars: int) -> str:
    if not isinstance(value, str) or not value.strip():
        raise FindingValidationError(f"{name} must be a non-empty string")
    normalized = value.strip()
    if len(normalized) > max_chars:
        raise FindingValidationError(f"{name} exceeds {max_chars} characters")
    return normalized


def _optional_text(name: str, value: Any, max_chars: int) -> str | None:
    if value is None:
        return None
    return _required_text(name, value, max_chars)


def _required_choice(name: str, value: Any, allowed: set[str]) -> str:
    normalized = _required_text(name, value, _MAX_ID_CHARS)
    if normalized not in allowed:
        raise FindingValidationError("{} must be one of: {}".format(name, ", ".join(sorted(allowed))))
    return normalized


def _validate_repro(value: Any) -> dict[str, list[str]]:
    if not isinstance(value, dict) or set(value) - {"argv", "steps"}:
        raise FindingValidationError("repro must contain only argv or steps")
    present = [name for name in ("argv", "steps") if name in value]
    if len(present) != 1:
        raise FindingValidationError("repro must contain exactly one of argv or steps")
    name = present[0]
    items = value[name]
    if not isinstance(items, list) or not items or len(items) > _MAX_REPRO_ITEMS:
        raise FindingValidationError(f"repro.{name} must contain 1 to {_MAX_REPRO_ITEMS} items")
    return {name: [_required_text("repro item", item, _MAX_TEXT_CHARS) for item in items]}


def _validate_evidence(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise FindingValidationError("evidence must be an object")
    if len(value) > 64:
        raise FindingValidationError("evidence exceeds the 64-property limit")
    evidence = dict(value)
    for name in _KNOWN_EVIDENCE_FIELDS:
        if name in evidence:
            evidence[name] = _required_text(name, evidence[name], _MAX_ID_CHARS)
    try:
        encoded = json_dumps(evidence).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise FindingValidationError("evidence must contain JSON values") from exc
    if len(encoded) > _MAX_EVIDENCE_BYTES:
        raise FindingValidationError("evidence exceeds the 32768-byte limit")
    return evidence


__all__ = [
    "FINDING_V1_SCHEMA_VERSION",
    "FindingRuntimeContext",
    "FindingValidationError",
    "build_finding_v1",
    "finding_fingerprint",
    "finding_tool_input_schema",
    "finding_v1_json_schema",
    "normalize_legacy_feedback",
]
