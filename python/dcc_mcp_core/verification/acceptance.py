"""Released-product production acceptance matrix.

This module owns the shared, versioned contract that records the production
acceptance state of every released DCC-MCP CLI adapter.  A green source test or
a mock can therefore never be mistaken for released, licensed, real-host proof:
every adapter must carry twelve independent per-level statuses (never a single
collapsed ``supported`` boolean), and the validator fails closed whenever an
optimistic wrapper, a source-tree-only import, an artifact-less release, a
mock-as-host-proof, or a missing readiness transition would otherwise pass.

This module ships only the shape and the fail-closed evaluator.  The adapter
that owns a licensed acceptance run is responsible for producing the evidence;
this module refuses to promote that evidence to a level it cannot prove.

Why it lives here
-----------------

Keeping the record and its evaluator in ``dcc-mcp-core`` gives every
downstream adapter and the public catalog a single, versioned target, exactly
like :mod:`dcc_mcp_core.schemas.finding` does for findings and
:mod:`dcc_mcp_core.verifier` does for asset verification.

Example:
-------
::

    from dcc_mcp_core.verification.acceptance import evaluate_acceptance

    evaluation = evaluate_acceptance(record)
    if not evaluation.ok:
        for finding in evaluation.findings:
            print(finding.code, finding.message)

The module is pure Python (no compiled ``_core`` extension, no third-party
runtime dependency) so it can run in an embedded DCC Python or under tests
without importing the PyO3 extension.

"""

from __future__ import annotations

import datetime
import hashlib
from pathlib import Path
import re
from typing import Any
from typing import Callable
import urllib.parse

from dcc_mcp_core.skills_helper import json_dumps
from dcc_mcp_core.skills_helper import json_loads

PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION = 1

#: Ordered list of the twelve production-acceptance levels.  Order encodes the
#: linear readiness chain (package -> bootstrap -> readyz -> search ->
#: instance-qualified call -> bounded readback); level order is therefore a
#: semantic property, not just a display convention.
LEVELS: tuple[str, ...] = (
    "repository_identity",
    "released_artifact",
    "package_import",
    "editor_bootstrap",
    "sidecar_identity",
    "readyz",
    "gateway_capability_index",
    "instance_qualified_call",
    "bounded_readback",
    "licensed_host_acceptance",
    "uninstall_rollback",
    "mixed_version_recovery",
)

#: The twelve independent per-level statuses.  There is intentionally no
#: ``supported`` value: acceptance is a matrix, not a boolean.
STATUSES: tuple[str, ...] = (
    "PASS",
    "PARTIAL",
    "FAIL",
    "NOT_RUN",
    "BLOCKED",
    "NOT_APPLICABLE",
    "UNKNOWN",
)

#: How a level's evidence was obtained.  Only ``real-host`` can ever promote a
#: licensed acceptance level; the rest label lower-trust signals truthfully.
EVIDENCE_SOURCES: tuple[str, ...] = (
    "real-host",
    "source-tree",
    "mock",
    "contract-test",
    "public-ci",
    "release",
    "catalog",
    "manual",
    "none",
)

#: The linear readiness chain.  A level may not be PASS when an earlier
#: applicable level in this chain is not PASS (issue #2384 "missing
#: transition").  ``sidecar_identity`` is excluded when a host has no sidecar
#: (it is marked ``NOT_APPLICABLE``), collapsing the chain to the exact
#: package -> bootstrap -> readyz -> search -> call -> readback sequence the
#: issue specifies.
TRANSITION_CHAIN: tuple[str, ...] = (
    "package_import",
    "editor_bootstrap",
    "sidecar_identity",
    "readyz",
    "gateway_capability_index",
    "instance_qualified_call",
    "bounded_readback",
)

#: Evidence sources that can never stand in for a licensed real-host run.
NON_HOST_SOURCES = frozenset({"source-tree", "mock", "contract-test", "public-ci"})

#: Overall-status precedence.  Lower index = worse.  ``NOT_APPLICABLE`` levels
#: are excluded from the derivation; every other status wins over PASS.
_STATUS_PRECEDENCE: tuple[str, ...] = ("FAIL", "BLOCKED", "PARTIAL", "NOT_RUN", "UNKNOWN", "PASS")

_DIGEST_RE = re.compile(r"^[a-fA-F0-9]{64}$")

#: Loopback hosts that can never be a public evidence source.
_LOOPBACK_HOSTS = frozenset({"localhost", "::1", "0.0.0.0"})
#: Private-use IPv4 prefixes that must not survive into a public report.
_PRIVATE_V4_PREFIXES = (
    "127.",
    "10.",
    "192.168.",
    "169.254.",
    "172.16.",
    "172.17.",
    "172.18.",
    "172.19.",
    "172.20.",
    "172.21.",
    "172.22.",
    "172.23.",
    "172.24.",
    "172.25.",
    "172.26.",
    "172.27.",
    "172.28.",
    "172.29.",
    "172.30.",
    "172.31.",
)


class AcceptanceValidationError(ValueError):
    """Raised when an acceptance record violates the bounded v1 contract."""


def _required_str(name: str, value: Any, max_chars: int = 256) -> str:
    if not isinstance(value, str) or not value.strip():
        raise AcceptanceValidationError(f"{name} must be a non-empty string")
    normalized = value.strip()
    if len(normalized) > max_chars:
        raise AcceptanceValidationError(f"{name} exceeds {max_chars} characters")
    return normalized


def _optional_str(name: str, value: Any, max_chars: int = 256) -> str | None:
    if value is None:
        return None
    return _required_str(name, value, max_chars)


def _required_choice(name: str, value: Any, allowed: tuple[str, ...]) -> str:
    normalized = _required_str(name, value)
    if normalized not in allowed:
        raise AcceptanceValidationError(f"{name} must be one of: {', '.join(allowed)}")
    return normalized


def _required_bool(name: str, value: Any) -> bool:
    if not isinstance(value, bool):
        raise AcceptanceValidationError(f"{name} must be a boolean")
    return value


def _required_nonneg_int(name: str, value: Any) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise AcceptanceValidationError(f"{name} must be a non-negative integer")
    return value


def normalize_digest(name: str, value: Any) -> str:
    """Return a lowercase 64-hex digest, accepting an optional ``sha256:`` prefix."""
    if not isinstance(value, str):
        raise AcceptanceValidationError(f"{name} must be a hex digest string")
    digest = value.strip().lower()
    if digest.startswith("sha256:"):
        digest = digest[7:]
    if not _DIGEST_RE.match(digest):
        raise AcceptanceValidationError(f"{name} must be a 64-character hex digest")
    return digest


def recompute_sha256(data: bytes) -> str:
    """Return the lowercase hex SHA-256 of *data*."""
    return hashlib.sha256(data).hexdigest()


def verify_release_sha256(
    url: str,
    expected_sha256: str,
    *,
    fetcher: Callable[[str], bytes] | None = None,
) -> bool:
    """Download *url* and verify its bytes match *expected_sha256*.

    The download is performed only when *fetcher* is provided; callers in
    tests inject a fetcher so no network access happens.  The production
    default fetches with the standard library.  Returns ``True`` only when the
    downloaded bytes hash to the expected digest; any mismatch or download
    failure returns ``False`` so level 2 (released artifact) can never pass on
    trust alone.
    """
    expected = normalize_digest("expected_sha256", expected_sha256)
    try:
        if fetcher is not None:
            data = fetcher(url)
        else:
            import urllib.request

            with urllib.request.urlopen(url, timeout=30) as response:
                data = response.read()
    except Exception:
        return False
    if not isinstance(data, bytes):
        data = bytes(data)
    return recompute_sha256(data) == expected


def production_acceptance_v1_json_schema() -> dict[str, Any]:
    """Load and return the packaged Production Acceptance v1 JSON Schema."""
    schema_path = Path(__file__).resolve().parent.parent / "schemas" / "production-acceptance-v1.schema.json"
    payload = json_loads(schema_path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise AcceptanceValidationError("Production Acceptance v1 schema must be a JSON object")
    return payload


def sanitize_evidence_link(url: str) -> str:
    """Return *url* only when it is a public HTTP(S) evidence link.

    Local paths, ``file://``/``ftp://`` URLs, private loopback hosts, private
    IPv4 ranges, UNC shares, absolute Windows paths, and URLs embedding
    credentials are all rejected.  The report contract must contain only
    sanitized public links, so any non-public reference fails closed.
    """
    normalized = _required_str("evidence url", url, max_chars=2048)
    lowered = normalized.lower()
    if not (lowered.startswith("http://") or lowered.startswith("https://")):
        raise AcceptanceValidationError(f"evidence url must be an http(s) URL: {normalized!r}")
    try:
        parts = urllib.parse.urlsplit(normalized)
    except ValueError as exc:
        raise AcceptanceValidationError(f"evidence url is not a valid URL: {normalized!r}") from exc
    if parts.username or parts.password:
        raise AcceptanceValidationError("evidence url must not embed credentials")
    host = (parts.hostname or "").lower()
    if not host:
        raise AcceptanceValidationError(f"evidence url has no host: {normalized!r}")
    if host in _LOOPBACK_HOSTS or host.startswith("127."):
        raise AcceptanceValidationError(f"evidence url must be public, got loopback: {normalized!r}")
    if host.startswith(_PRIVATE_V4_PREFIXES):
        raise AcceptanceValidationError(f"evidence url must be public, got private address: {normalized!r}")
    return normalized


def make_level(
    status: str,
    *,
    source: str = "none",
    checked_at: str = "",
    detail: str = "",
    evidence: Any = (),
    backend: Any = None,
) -> dict[str, Any]:
    """Build one level entry with bounded validation."""
    status = _required_choice("status", status, STATUSES)
    source = _required_choice("source", source, EVIDENCE_SOURCES)
    level: dict[str, Any] = {"status": status, "source": source, "checked_at": checked_at}
    if detail:
        level["detail"] = _required_str("detail", detail, max_chars=4096)
    if evidence:
        if not isinstance(evidence, (list, tuple)):
            raise AcceptanceValidationError("evidence must be a list")
        level["evidence"] = [
            sanitize_evidence_link(str(link)) if not isinstance(link, dict) else _validate_link(link)
            for link in evidence
        ]
    if backend is not None:
        level["backend"] = _validate_backend(backend)
    return level


def _validate_link(link: Any) -> dict[str, str]:
    if not isinstance(link, dict) or "url" not in link or "label" not in link:
        raise AcceptanceValidationError("evidence link must have url and label")
    url = sanitize_evidence_link(link["url"])
    label = _required_str("label", link["label"])
    out: dict[str, str] = {"url": url, "label": label}
    if link.get("sha256") is not None:
        out["sha256"] = normalize_digest("link.sha256", link["sha256"])
    return out


def _validate_backend(backend: Any) -> dict[str, Any]:
    if not isinstance(backend, dict):
        raise AcceptanceValidationError("backend must be an object")
    allowed = {"success", "loaded", "error"}
    unknown = set(backend) - allowed
    if unknown:
        raise AcceptanceValidationError(f"unknown backend fields: {', '.join(sorted(unknown))}")
    out: dict[str, Any] = {}
    if "success" in backend:
        out["success"] = _required_bool("backend.success", backend["success"])
    if "loaded" in backend:
        out["loaded"] = _required_bool("backend.loaded", backend["loaded"])
    if "error" in backend:
        out["error"] = backend["error"]
    return out


def _validate_product(product: Any) -> dict[str, Any]:
    if not isinstance(product, dict):
        raise AcceptanceValidationError("product must be an object")
    required = {
        "identifier",
        "entity",
        "owner_repository",
        "catalog_version",
        "package_version",
        "release_tag",
        "source_ref",
        "asset_name",
        "publisher_digest",
        "recomputed_sha256",
        "runner",
        "tests_executed",
        "tests_skipped",
        "credentials_licenses",
        "last_licensed_acceptance",
        "evidence_links",
    }
    missing = required - set(product)
    if missing:
        raise AcceptanceValidationError(f"product missing required fields: {', '.join(sorted(missing))}")
    out: dict[str, Any] = {
        "identifier": _required_str("product.identifier", product.get("identifier")),
        "entity": _required_str("product.entity", product.get("entity")),
        "owner_repository": sanitize_evidence_link(product.get("owner_repository")),
        "catalog_version": _required_str("product.catalog_version", product.get("catalog_version"), 64),
        "package_version": _required_str("product.package_version", product.get("package_version"), 64),
        "release_tag": _required_str("product.release_tag", product.get("release_tag"), 128),
        "source_ref": _optional_digestish("product.source_ref", product.get("source_ref")),
        "asset_name": _optional_str("product.asset_name", product.get("asset_name")),
        "publisher_digest": _optional_digest("product.publisher_digest", product.get("publisher_digest")),
        "recomputed_sha256": _optional_digest("product.recomputed_sha256", product.get("recomputed_sha256")),
        "runner": _validate_runner(product.get("runner")),
        "tests_executed": _required_nonneg_int("product.tests_executed", product.get("tests_executed")),
        "tests_skipped": _required_nonneg_int("product.tests_skipped", product.get("tests_skipped")),
        "credentials_licenses": _validate_credentials(product.get("credentials_licenses")),
        "last_licensed_acceptance": _optional_str(
            "product.last_licensed_acceptance", product.get("last_licensed_acceptance")
        ),
        "evidence_links": _validate_links("product.evidence_links", product.get("evidence_links")),
    }
    return out


def _optional_digestish(name: str, value: Any) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not re.match(r"^[a-fA-F0-9]{7,64}$", value.strip()):
        raise AcceptanceValidationError(f"{name} must be a 7-64 character hex source ref")
    return value.strip()


def _optional_digest(name: str, value: Any) -> str | None:
    if value is None:
        return None
    return normalize_digest(name, value)


def _validate_runner(runner: Any) -> dict[str, Any]:
    if not isinstance(runner, dict) or "available" not in runner:
        raise AcceptanceValidationError("runner must be an object with `available`")
    out: dict[str, Any] = {"available": _required_bool("runner.available", runner["available"])}
    for field in ("os", "python_version", "host_version"):
        value = runner.get(field)
        if value is not None:
            out[field] = _required_str(f"runner.{field}", value)
    return out


def _validate_credentials(credentials: Any) -> dict[str, Any]:
    if not isinstance(credentials, dict) or "licensed" not in credentials:
        raise AcceptanceValidationError("credentials_licenses must be an object with `licensed`")
    out: dict[str, Any] = {"licensed": _required_bool("credentials_licenses.licensed", credentials["licensed"])}
    if credentials.get("host_license_kind") is not None:
        out["host_license_kind"] = _required_str(
            "credentials_licenses.host_license_kind", credentials["host_license_kind"]
        )
    if credentials.get("notes") is not None:
        out["notes"] = _required_str("credentials_licenses.notes", credentials["notes"], 4096)
    return out


def _validate_links(name: str, links: Any) -> list[dict[str, str]]:
    if not isinstance(links, (list, tuple)):
        raise AcceptanceValidationError(f"{name} must be a list")
    return [_validate_link(link) for link in links]


def _validate_levels(levels: Any) -> dict[str, dict[str, Any]]:
    if not isinstance(levels, dict):
        raise AcceptanceValidationError("levels must be an object")
    missing = [key for key in LEVELS if key not in levels]
    if missing:
        raise AcceptanceValidationError(f"levels missing required level(s): {', '.join(missing)}")
    out: dict[str, dict[str, Any]] = {}
    for key in LEVELS:
        out[key] = _validate_level(key, levels[key])
    return out


def _validate_level(key: str, level: Any) -> dict[str, Any]:
    if not isinstance(level, dict):
        raise AcceptanceValidationError(f"level {key} must be an object")
    status = _required_choice(f"{key}.status", level.get("status"), STATUSES)
    source = _required_choice(f"{key}.source", level.get("source"), EVIDENCE_SOURCES)
    checked_at = _required_str(f"{key}.checked_at", level.get("checked_at"), 64)
    out: dict[str, Any] = {"status": status, "source": source, "checked_at": checked_at}
    if level.get("detail") is not None:
        out["detail"] = _required_str(f"{key}.detail", level.get("detail"), 4096)
    if level.get("evidence") is not None:
        out["evidence"] = _validate_links(f"{key}.evidence", level.get("evidence"))
    if level.get("backend") is not None:
        out["backend"] = _validate_backend(level["backend"])
    return out


def validate_acceptance_schema(record: Any) -> dict[str, Any]:
    """Validate the structural v1 contract and return a normalized copy.

    Raises :class:`AcceptanceValidationError` on any structural violation:
    a collapsed ``supported`` boolean, a missing product field, a missing or
    invalid level, or a non-public evidence link.
    """
    if not isinstance(record, dict):
        raise AcceptanceValidationError("acceptance record must be an object")
    if "supported" in record:
        raise AcceptanceValidationError("a collapsed `supported` boolean is not an acceptance record")
    if record.get("schema_version") != PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION:
        raise AcceptanceValidationError("schema_version must be 1")
    report = record.get("report")
    if not isinstance(report, dict):
        raise AcceptanceValidationError("report must be an object")
    _required_str("report.generated_at", report.get("generated_at"), 64)
    _required_choice("report.overall_status", report.get("overall_status"), STATUSES)
    _required_bool("report.sanitized", report.get("sanitized"))
    return {
        "schema_version": PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION,
        "product": _validate_product(record.get("product")),
        "levels": _validate_levels(record.get("levels")),
        "report": {
            "generated_at": report["generated_at"],
            "overall_status": report["overall_status"],
            "sanitized": report["sanitized"],
        },
    }


# ── Fail-closed evaluation ────────────────────────────────────────────────


class AcceptanceFinding:
    """A single fail-closed violation found while evaluating a record."""

    def __init__(self, code: str, level: str, message: str) -> None:
        self.code = code
        self.level = level
        self.message = message

    def to_dict(self) -> dict[str, str]:
        """Serialize the finding for the machine-readable report."""
        return {"code": self.code, "level": self.level, "message": self.message}


class AcceptanceEvaluation:
    """Result of evaluating one acceptance record.

    :attr:`ok` is ``True`` only when the derived overall status is ``PASS``.
    :attr:`effective_levels` carries each level's status after any fail-closed
    demotion, and :attr:`findings` carries the reasons for every demotion.
    """

    def __init__(
        self,
        overall_status: str,
        effective_levels: dict[str, str],
        findings: list[AcceptanceFinding],
    ) -> None:
        self.overall_status = overall_status
        self.effective_levels = effective_levels
        self.findings = findings

    @property
    def ok(self) -> bool:
        """True only when the record is fully production-accepted."""
        return self.overall_status == "PASS"


def _overall_status(levels: dict[str, str]) -> str:
    applicable = [status for key, status in levels.items() if status != "NOT_APPLICABLE"]
    if not applicable:
        return "NOT_RUN"
    for status in _STATUS_PRECEDENCE:
        if status in applicable:
            return status
    return "PASS"


def evaluate_acceptance(record: Any) -> AcceptanceEvaluation:
    """Validate *record* and apply the fail-closed acceptance rules.

    Structural violations raise :class:`AcceptanceValidationError`.  Semantic
    violations (optimistic wrapper, source-tree-only import, artifact-less
    release, mock-as-host-proof, missing transition) demote the offending
    level and are reported as findings, never silently promoted to PASS.
    """
    normalized = validate_acceptance_schema(record)
    product = normalized["product"]
    levels: dict[str, dict[str, Any]] = normalized["levels"]
    effective: dict[str, str] = {key: levels[key]["status"] for key in LEVELS}
    findings: list[AcceptanceFinding] = []

    def demote(key: str, status: str, code: str, message: str) -> None:
        effective[key] = status
        findings.append(AcceptanceFinding(code, key, message))

    for key in LEVELS:
        level = levels[key]
        if level["status"] != "PASS":
            continue
        backend = level.get("backend")
        if isinstance(backend, dict):
            loaded = backend.get("loaded")
            success = backend.get("success")
            error = backend.get("error")
            if loaded is True and (success is False or error):
                demote(key, "FAIL", "optimistic_wrapper", f"{key} reports loaded=true with a failed backend")

    package_import = levels["package_import"]
    if package_import["status"] == "PASS" and package_import["source"] == "source-tree":
        demote(
            "package_import",
            "FAIL",
            "source_tree_only_import",
            "package_import must install from a released artifact, not a source checkout",
        )

    released_artifact = levels["released_artifact"]
    if released_artifact["status"] == "PASS":
        if not product["asset_name"] or not product["publisher_digest"] or not product["recomputed_sha256"]:
            demote(
                "released_artifact",
                "FAIL",
                "artifact_less_release",
                "released_artifact requires downloadable bytes and a verifiable digest",
            )
        elif product["publisher_digest"] != product["recomputed_sha256"]:
            demote(
                "released_artifact",
                "FAIL",
                "digest_mismatch",
                "recomputed SHA-256 does not match the publisher digest",
            )

    licensed = levels["licensed_host_acceptance"]
    if licensed["status"] == "PASS" and licensed["source"] in NON_HOST_SOURCES:
        demote(
            "licensed_host_acceptance",
            "FAIL",
            "mock_as_host_proof",
            "licensed_host_acceptance requires real-host evidence, not a mock or CI signal",
        )
    elif licensed["status"] == "PASS" and not product["credentials_licenses"]["licensed"]:
        demote(
            "licensed_host_acceptance",
            "FAIL",
            "unlicensed_host",
            "licensed_host_acceptance requires licensed credentials",
        )

    chain_positions = {key: index for index, key in enumerate(TRANSITION_CHAIN)}
    for key in TRANSITION_CHAIN:
        if effective[key] != "PASS":
            continue
        for prereq in TRANSITION_CHAIN[: chain_positions[key]]:
            if levels[prereq]["status"] == "NOT_APPLICABLE":
                continue
            if effective[prereq] != "PASS":
                demote(key, "BLOCKED", "missing_transition", f"{key} cannot pass before {prereq} passes")
                break

    overall = _overall_status(effective)
    return AcceptanceEvaluation(overall, effective, findings)


def evaluate_many(records: Any) -> list[AcceptanceEvaluation]:
    """Evaluate a list of records and fail closed on duplicate ownership.

    Duplicate identifiers are rejected as a matrix-level structural violation;
    missing ``owner_repository`` is already enforced per record.
    """
    if not isinstance(records, (list, tuple)):
        raise AcceptanceValidationError("records must be a list")
    seen: dict[str, str] = {}
    evaluations: list[AcceptanceEvaluation] = []
    for record in records:
        normalized = validate_acceptance_schema(record)
        identifier = normalized["product"]["identifier"]
        if identifier in seen:
            raise AcceptanceValidationError(f"duplicate product identifier: {identifier}")
        seen[identifier] = normalized["product"]["owner_repository"]
        evaluations.append(evaluate_acceptance(normalized))
    return evaluations


def _now_utc() -> str:
    """Return a current UTC timestamp for record skeletons."""
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def record_from_catalog_entry(
    entry: Any,
    *,
    catalog_version: str = "1",
    checked_at: str = "",
) -> dict[str, Any]:
    """Derive a production-acceptance record skeleton from a catalog entry.

    The catalog carries public identity (repository, package, publisher
    digest) but not licensed real-host proof, so every level starts as
    ``NOT_RUN`` except ``repository_identity`` (``PASS`` from catalog data)
    and levels that are structurally not applicable when no install block is
    present.  The derived record is a starting point for the adapter-owned
    acceptance run, never a claim of production acceptance.
    """
    if not isinstance(entry, dict):
        raise AcceptanceValidationError("catalog entry must be an object")
    if not checked_at:
        checked_at = _now_utc()
    identifier = _required_str("name", entry.get("name"))
    entity = _required_str("maintainer", entry.get("maintainer") or entry.get("entity") or "dcc-mcp")
    owner_repository = sanitize_evidence_link(entry.get("url"))
    package_version = _required_str("version", entry.get("version") or "0.0.0", 64)
    install = entry.get("install")
    asset_name: str | None = None
    publisher_digest: str | None = None
    if isinstance(install, dict):
        asset_name = _optional_str("asset_name", install.get("url", "").rsplit("/", 1)[-1] or None)
        publisher_digest = _optional_digest("publisher_digest", install.get("sha256"))

    def level(status: str, source: str = "catalog", detail: str = "") -> dict[str, Any]:
        return make_level(status, source=source, checked_at=checked_at, detail=detail)

    has_release = bool(asset_name and publisher_digest)
    levels: dict[str, Any] = {
        "repository_identity": level("PASS", "catalog", "public repository recorded in the catalog"),
        "released_artifact": level("NOT_RUN", "catalog")
        if has_release
        else level("FAIL", "catalog", "catalog entry has no downloadable artifact or digest"),
        "package_import": level("NOT_RUN"),
        "editor_bootstrap": level("NOT_RUN"),
        "sidecar_identity": level("NOT_RUN"),
        "readyz": level("NOT_RUN"),
        "gateway_capability_index": level("NOT_RUN"),
        "instance_qualified_call": level("NOT_RUN"),
        "bounded_readback": level("NOT_RUN"),
        "licensed_host_acceptance": level("NOT_RUN", "none"),
        "uninstall_rollback": level("NOT_RUN"),
        "mixed_version_recovery": level("NOT_RUN"),
    }
    product: dict[str, Any] = {
        "identifier": identifier,
        "entity": entity,
        "owner_repository": owner_repository,
        "catalog_version": _required_str("catalog_version", catalog_version, 64),
        "package_version": package_version,
        "release_tag": "v" + package_version,
        "source_ref": None,
        "asset_name": asset_name,
        "publisher_digest": publisher_digest,
        "recomputed_sha256": None,
        "runner": {"available": False},
        "tests_executed": 0,
        "tests_skipped": 0,
        "credentials_licenses": {"licensed": False},
        "last_licensed_acceptance": None,
        "evidence_links": [{"url": owner_repository, "label": "repository"}],
    }
    return {
        "schema_version": PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION,
        "product": product,
        "levels": levels,
        "report": {"generated_at": checked_at, "overall_status": "NOT_RUN", "sanitized": True},
    }


def build_report(records: Any, *, generated_at: str = "") -> dict[str, Any]:
    """Build a deterministic, machine-readable matrix over many records.

    Products are sorted by identifier and every level key is emitted in its
    canonical order, so two identical inputs always produce byte-identical
    JSON (given the same ``generated_at``).  Duplicate identifiers fail
    closed.  The output contains only sanitized public evidence links.
    """
    evaluations = evaluate_many(records)
    products: list[dict[str, Any]] = []
    for record, evaluation in zip(records, evaluations):
        normalized = validate_acceptance_schema(record)
        product = normalized["product"]
        products.append(
            {
                "identifier": product["identifier"],
                "owner_repository": product["owner_repository"],
                "package_version": product["package_version"],
                "release_tag": product["release_tag"],
                "overall_status": evaluation.overall_status,
                "levels": {key: evaluation.effective_levels[key] for key in LEVELS},
                "findings": [finding.to_dict() for finding in evaluation.findings],
            }
        )
    products.sort(key=lambda item: item["identifier"])
    summary: dict[str, int] = {status: 0 for status in STATUSES}
    for item in products:
        status = item["overall_status"]
        if status in summary:
            summary[status] += 1
    return {
        "schema_version": PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION,
        "generated_at": generated_at,
        "sanitized": True,
        "summary": summary,
        "products": products,
    }


def dumps_report(report: dict[str, Any]) -> str:
    """Serialize a report deterministically (sorted keys, compact)."""
    return json_dumps(report)


__all__ = [
    "EVIDENCE_SOURCES",
    "LEVELS",
    "PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION",
    "STATUSES",
    "TRANSITION_CHAIN",
    "AcceptanceEvaluation",
    "AcceptanceFinding",
    "AcceptanceValidationError",
    "build_report",
    "dumps_report",
    "evaluate_acceptance",
    "evaluate_many",
    "make_level",
    "normalize_digest",
    "production_acceptance_v1_json_schema",
    "recompute_sha256",
    "record_from_catalog_entry",
    "sanitize_evidence_link",
    "validate_acceptance_schema",
    "verify_release_sha256",
]
