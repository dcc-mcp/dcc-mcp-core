"""Red -> Green tests for the released-product production acceptance matrix.

Covers issue #2384: the shared schema rejects a collapsed ``supported``
boolean and any product missing one of the twelve levels; the fail-closed
evaluator rejects optimistic wrappers, source-tree-only imports, artifact-less
releases, mock-as-host-proof, and missing transitions; release verification
recomputes SHA-256; engine fixtures stay editor-free; and the report is
deterministic and sanitized.
"""

from __future__ import annotations

import json
from typing import Any

import pytest

from dcc_mcp_core import yaml_loads
from dcc_mcp_core.verification.acceptance import LEVELS
from dcc_mcp_core.verification.acceptance import AcceptanceValidationError
from dcc_mcp_core.verification.acceptance import build_report
from dcc_mcp_core.verification.acceptance import dumps_report
from dcc_mcp_core.verification.acceptance import evaluate_acceptance
from dcc_mcp_core.verification.acceptance import evaluate_many
from dcc_mcp_core.verification.acceptance import production_acceptance_v1_json_schema
from dcc_mcp_core.verification.acceptance import recompute_sha256
from dcc_mcp_core.verification.acceptance import record_from_catalog_entry
from dcc_mcp_core.verification.acceptance import sanitize_evidence_link
from dcc_mcp_core.verification.acceptance import validate_acceptance_schema
from dcc_mcp_core.verification.acceptance import verify_release_sha256
from dcc_mcp_core.verification.acceptance_fixtures import godot_project
from dcc_mcp_core.verification.acceptance_fixtures import godot_version_probe
from dcc_mcp_core.verification.acceptance_fixtures import unity_project_version
from dcc_mcp_core.verification.acceptance_fixtures import unreal_project

TIMESTAMP = "2026-01-01T00:00:00+00:00"
_DIGEST_A = "a" * 64
_DIGEST_B = "b" * 64
_SOURCE_REF = "0" * 40


def _level(status: str = "PASS", source: str = "real-host", backend: Any = None) -> dict[str, Any]:
    level: dict[str, Any] = {"status": status, "source": source, "checked_at": TIMESTAMP}
    if backend is not None:
        level["backend"] = backend
    return level


def _valid_record(overrides: Any = None) -> dict[str, Any]:
    record: dict[str, Any] = {
        "schema_version": 1,
        "product": {
            "identifier": "dcc-mcp-maya",
            "entity": "dcc-mcp",
            "owner_repository": "https://github.com/dcc-mcp/dcc-mcp-maya",
            "catalog_version": "1",
            "package_version": "0.9.22",
            "release_tag": "v0.9.22",
            "source_ref": _SOURCE_REF,
            "asset_name": "dcc_mcp_maya-0.9.22-py3-none-any.whl",
            "publisher_digest": _DIGEST_A,
            "recomputed_sha256": _DIGEST_A,
            "runner": {"available": True},
            "tests_executed": 10,
            "tests_skipped": 2,
            "credentials_licenses": {"licensed": True},
            "last_licensed_acceptance": "2026-01-01",
            "evidence_links": [{"url": "https://github.com/dcc-mcp/dcc-mcp-maya", "label": "repository"}],
        },
        "levels": {key: _level() for key in LEVELS},
        "report": {"generated_at": TIMESTAMP, "overall_status": "PASS", "sanitized": True},
    }
    if overrides:
        _deep_update(record, overrides)
    return record


def _deep_update(target: dict[str, Any], updates: dict[str, Any]) -> None:
    for key, value in updates.items():
        if isinstance(value, dict) and isinstance(target.get(key), dict):
            _deep_update(target[key], value)
        else:
            target[key] = value


def _jsonschema_validate(record: Any) -> list[str]:
    validator = pytest.importorskip("jsonschema").Draft202012Validator(production_acceptance_v1_json_schema())
    return [error.message for error in validator.iter_errors(record)]


# ── Schema contract ───────────────────────────────────────────────────────


def test_schema_exposes_twelve_levels_and_statuses() -> None:
    schema = production_acceptance_v1_json_schema()
    required_levels = schema["$defs"]["levels"]["required"]
    assert set(required_levels) == set(LEVELS)
    assert len(required_levels) == 12
    assert schema["$defs"]["status"]["enum"] == [
        "PASS",
        "PARTIAL",
        "FAIL",
        "NOT_RUN",
        "BLOCKED",
        "NOT_APPLICABLE",
        "UNKNOWN",
    ]
    # The top level forbids a collapsed `supported` boolean.
    assert "supported" not in schema["properties"]
    assert schema["not"] == {"required": ["supported"]}
    assert schema["additionalProperties"] is False


def test_schema_rejects_collapsed_supported_boolean() -> None:
    record = _valid_record({"supported": True})
    errors = _jsonschema_validate(record)
    assert errors, "a `supported` boolean must fail schema validation"
    with pytest.raises(AcceptanceValidationError, match="supported"):
        validate_acceptance_schema(record)


def test_schema_rejects_product_missing_a_level() -> None:
    record = _valid_record()
    del record["levels"]["readyz"]
    errors = _jsonschema_validate(record)
    assert errors
    with pytest.raises(AcceptanceValidationError, match="readyz"):
        validate_acceptance_schema(record)


def test_schema_rejects_product_non_public_owner_repository() -> None:
    record = _valid_record({"product": {"owner_repository": "file:///private/repo"}})
    with pytest.raises(AcceptanceValidationError, match="evidence url must be an http"):
        validate_acceptance_schema(record)


# ── Fail-closed evaluator ────────────────────────────────────────────────


def test_valid_record_passes() -> None:
    evaluation = evaluate_acceptance(_valid_record())
    assert evaluation.ok is True
    assert evaluation.overall_status == "PASS"
    assert evaluation.findings == []


def test_optimistic_wrapper_fails_closed() -> None:
    record = _valid_record(
        {
            "levels": {
                "instance_qualified_call": _level(backend={"loaded": True, "success": False, "error": "backend down"})
            }
        }
    )
    evaluation = evaluate_acceptance(record)
    assert evaluation.overall_status != "PASS"
    assert evaluation.effective_levels["instance_qualified_call"] == "FAIL"
    assert {finding.code for finding in evaluation.findings} >= {"optimistic_wrapper"}


@pytest.mark.parametrize(
    "backend",
    [
        pytest.param({"success": False, "error": "backend down"}, id="no-loaded-flag"),
        pytest.param({"error": "backend down"}, id="error-only"),
        pytest.param({"success": False}, id="failure-without-message"),
    ],
)
def test_optimistic_wrapper_fails_closed_without_loaded_flag(backend: Any) -> None:
    """A failed backend demotes its level even when ``loaded`` is absent.

    A wrapper that reports ``success=False`` without ever claiming ``loaded``
    must not escape the rule by omitting one key.
    """
    record = _valid_record({"levels": {"instance_qualified_call": _level(backend=backend)}})
    evaluation = evaluate_acceptance(record)
    assert evaluation.overall_status != "PASS"
    assert evaluation.effective_levels["instance_qualified_call"] == "FAIL"
    assert {finding.code for finding in evaluation.findings} >= {"optimistic_wrapper"}


def test_level_without_a_backend_block_is_not_penalized() -> None:
    """An absent ``backend`` block means "not instrumented", not "failed".

    ``optimistic_wrapper`` punishes a self-reported backend that contradicts its
    own level, so it triggers on an explicitly failed backend only.  A level
    with no backend block carries no such contradiction; whether an
    un-instrumented level should still be acceptable is a separate question the
    ``source`` field already answers, and collapsing the two would make every
    not-yet-instrumented product unshippable in one step.
    """
    record = _valid_record({"levels": {"instance_qualified_call": _level(source="real-host")}})
    evaluation = evaluate_acceptance(record)
    assert evaluation.effective_levels["instance_qualified_call"] == "PASS"
    assert [finding.code for finding in evaluation.findings] == []


def test_source_tree_only_import_fails_closed() -> None:
    record = _valid_record({"levels": {"package_import": _level(source="source-tree")}})
    evaluation = evaluate_acceptance(record)
    assert evaluation.effective_levels["package_import"] == "FAIL"
    assert {finding.code for finding in evaluation.findings} >= {"source_tree_only_import"}


def test_artifact_less_release_fails_closed() -> None:
    record = _valid_record({"product": {"recomputed_sha256": None}})
    evaluation = evaluate_acceptance(record)
    assert evaluation.effective_levels["released_artifact"] == "FAIL"
    assert {finding.code for finding in evaluation.findings} >= {"artifact_less_release"}


def test_digest_mismatch_fails_closed() -> None:
    record = _valid_record({"product": {"recomputed_sha256": _DIGEST_B}})
    evaluation = evaluate_acceptance(record)
    assert evaluation.effective_levels["released_artifact"] == "FAIL"
    assert {finding.code for finding in evaluation.findings} >= {"digest_mismatch"}


@pytest.mark.parametrize("source", ["mock", "contract-test", "public-ci", "source-tree"])
def test_mock_as_host_proof_fails_closed(source: str) -> None:
    record = _valid_record({"levels": {"licensed_host_acceptance": _level(source=source)}})
    evaluation = evaluate_acceptance(record)
    assert evaluation.effective_levels["licensed_host_acceptance"] == "FAIL"
    assert {finding.code for finding in evaluation.findings} >= {"mock_as_host_proof"}


def test_unlicensed_host_fails_closed() -> None:
    record = _valid_record({"product": {"credentials_licenses": {"licensed": False}}})
    evaluation = evaluate_acceptance(record)
    assert evaluation.effective_levels["licensed_host_acceptance"] == "FAIL"
    assert {finding.code for finding in evaluation.findings} >= {"unlicensed_host"}


def test_missing_transition_fails_closed() -> None:
    record = _valid_record({"levels": {"editor_bootstrap": _level(status="FAIL"), "readyz": _level(status="PASS")}})
    evaluation = evaluate_acceptance(record)
    assert evaluation.effective_levels["readyz"] == "BLOCKED"
    assert {finding.code for finding in evaluation.findings} >= {"missing_transition"}


def test_duplicate_identifier_fails_closed() -> None:
    first = _valid_record()
    second = _valid_record()
    with pytest.raises(AcceptanceValidationError, match="duplicate product identifier"):
        evaluate_many([first, second])


# ── Release digest verification ───────────────────────────────────────────


def test_verify_release_sha256_recomputes_and_matches() -> None:
    payload = b"released wheel bytes"
    expected = recompute_sha256(payload)

    def fetcher(url: str) -> bytes:
        return payload

    assert verify_release_sha256("https://example.com/pkg.whl", expected, fetcher=fetcher) is True
    assert verify_release_sha256("https://example.com/pkg.whl", _DIGEST_A, fetcher=fetcher) is False


def test_verify_release_sha256_fails_closed_on_download_error() -> None:
    def fetcher(url: str) -> bytes:
        raise RuntimeError("network down")

    assert verify_release_sha256("https://example.com/pkg.whl", _DIGEST_A, fetcher=fetcher) is False


@pytest.mark.parametrize(
    "url",
    [
        "file:///C:/private/pkg.whl",
        "file://localhost/C:/private/pkg.whl",
        "ftp://host/pkg.whl",
        "http://127.0.0.1/pkg.whl",
    ],
)
def test_verify_release_sha256_rejects_non_public_url(tmp_path: Any, url: str) -> None:
    """A non-public URL can never become a local-filesystem read.

    The fetcher below would return bytes whose digest matches, so a passing
    call would prove the fetcher ran instead of the URL being rejected first.
    """
    local = tmp_path / "pkg.whl"
    local.write_bytes(b"local bytes")
    digest = recompute_sha256(b"local bytes")

    def fetcher(fetched_url: str) -> bytes:
        return local.read_bytes()

    assert verify_release_sha256(url, digest, fetcher=fetcher) is False


# ── Evidence sanitization ─────────────────────────────────────────────────


@pytest.mark.parametrize(
    "url",
    [
        "file:///C:/Users/x/evidence.json",
        "http://localhost:8080/report",
        "http://127.0.0.1/report",
        "C:\\Users\\x\\report.json",
        "\\\\server\\share\\report.json",
        "ftp://host/report",
        "https://user:token@github.com/dcc-mcp/dcc-mcp-maya",
        # Alternate spellings of loopback that bypass a plain "127." prefix match.
        "http://2130706433/report",
        "http://0177.0.0.1/report",
        "http://0x7f.1/report",
        "http://127.1/report",
        "http://[fd00::1]/report",
        "https://[::ffff:127.0.0.1]/report",
        # Credentials smuggled into the query string instead of userinfo.
        "https://github.com/dcc-mcp/dcc-mcp-maya?token=SECRET",
        "https://github.com/dcc-mcp/dcc-mcp-maya?sig=abc",
    ],
)
def test_sanitize_evidence_link_rejects_non_public(url: str) -> None:
    with pytest.raises(AcceptanceValidationError):
        sanitize_evidence_link(url)


def test_sanitize_evidence_link_accepts_public_https() -> None:
    assert sanitize_evidence_link("https://github.com/dcc-mcp/dcc-mcp-maya") == (
        "https://github.com/dcc-mcp/dcc-mcp-maya"
    )
    # A public numeric host is accepted: hardening targets non-public hosts only.
    assert sanitize_evidence_link("http://93.184.216.34/report") == "http://93.184.216.34/report"


# ── Engine fixtures (editor-free) ─────────────────────────────────────────


def test_unity_project_version_writes_and_rejects_unsupported(tmp_path: Any) -> None:
    result = unity_project_version(tmp_path, "2022.3.10f1")
    assert result["launched"] is False
    assert result["kind"] == "unity"
    project_file = tmp_path / "ProjectSettings" / "ProjectVersion.txt"
    assert "m_EditorVersion: 2022.3.10f1" in project_file.read_text(encoding="utf-8")

    with pytest.raises(AcceptanceValidationError, match="unsupported"):
        unity_project_version(tmp_path / "old", "2019.4.0f1")


def test_tuanjie_project_version_decision(tmp_path: Any) -> None:
    result = unity_project_version(
        tmp_path, "2022.3.10f1", flavor="tuanjie", custom_editor_path="C:/Tuanjie/Editor.exe"
    )
    assert result["kind"] == "tuanjie"
    assert result["decision"] == "C:/Tuanjie/Editor.exe"


def test_unreal_project_writes_and_rejects_unsupported(tmp_path: Any) -> None:
    result = unreal_project(tmp_path, "5.4", released_package="dcc-mcp-unreal")
    assert result["launched"] is False
    uproject = json.loads((tmp_path / "DisposableProject.uproject").read_text(encoding="utf-8"))
    assert uproject["EngineAssociation"] == "5.4"
    assert uproject["Plugins"][0]["Name"] == "dcc-mcp-unreal"

    with pytest.raises(AcceptanceValidationError, match="unsupported"):
        unreal_project(tmp_path / "old", "4.27")


def test_godot_project_writes_and_rejects_unsupported(tmp_path: Any) -> None:
    result = godot_project(tmp_path, config_version=4)
    assert result["launched"] is False
    assert godot_version_probe(tmp_path) == 4

    with pytest.raises(AcceptanceValidationError, match="unsupported"):
        godot_project(tmp_path / "old", config_version=2)


# ── Catalog coverage ──────────────────────────────────────────────────────


def test_catalog_derives_a_record_for_every_adapter() -> None:
    from conftest import REPO_ROOT

    catalog = yaml_loads((REPO_ROOT / "dcc-mcp-catalog.yml").read_text(encoding="utf-8"))
    entries = catalog["entries"]
    assert entries

    records = [record_from_catalog_entry(entry, checked_at=TIMESTAMP) for entry in entries]
    identifiers = [record["product"]["identifier"] for record in records]

    # Unique identifiers: no duplicate adapter in the public catalog.
    assert len(identifiers) == len(set(identifiers))

    for record in records:
        # Every record carries all twelve levels and a NOT_RUN overall — the
        # catalog is identity, never production acceptance.
        assert set(record["levels"]) == set(LEVELS)
        assert record["report"]["overall_status"] != "PASS"
        assert record["report"]["sanitized"] is True


# ── Report contract ───────────────────────────────────────────────────────


def test_report_is_deterministic_and_sanitized() -> None:
    maya = _valid_record()
    blender = _valid_record(
        {
            "product": {
                "identifier": "dcc-mcp-blender",
                "owner_repository": "https://github.com/dcc-mcp/dcc-mcp-blender",
            }
        }
    )
    report = build_report([maya, blender], generated_at=TIMESTAMP)
    assert report["sanitized"] is True
    # Deterministic ordering: sorted by identifier.
    assert [item["identifier"] for item in report["products"]] == ["dcc-mcp-blender", "dcc-mcp-maya"]
    # Every product emits an independent per-level status.
    assert set(report["products"][0]["levels"]) == set(LEVELS)
    assert dumps_report(report) == dumps_report(build_report([maya, blender], generated_at=TIMESTAMP))


def test_report_promotes_nothing_from_a_failing_level() -> None:
    broken = _valid_record({"product": {"recomputed_sha256": None}})
    report = build_report([broken], generated_at=TIMESTAMP)
    item = report["products"][0]
    assert item["overall_status"] != "PASS"
    assert item["levels"]["released_artifact"] == "FAIL"
    assert {finding["code"] for finding in item["findings"]} >= {"artifact_less_release"}
