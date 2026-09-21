"""Catalog provenance stays optional and typed across public install contracts."""

from copy import deepcopy
import hashlib
import json
from pathlib import Path

from jsonschema import Draft202012Validator
import pytest

ROOT = Path(__file__).resolve().parents[1]
# `-v1` is frozen at its released bytes and carries no catalog provenance; the
# live install SOP artifact is `-v2`.
SCHEMAS = [
    ROOT / "contracts/dcc-discovery-decision-v1.schema.json",
    ROOT / "python/dcc_mcp_core/schemas/adapter-install-sop-v2.schema.json",
]


def _current_wheel_resource(contract: dict) -> dict:
    resources = contract["distributions"]["dcc-mcp-core"]["wheel_resources"]
    matches = [resource for resource in resources if resource["member"].endswith("-v2.schema.json")]
    assert len(matches) == 1, "expected exactly one current install SOP wheel resource"
    return matches[0]


def test_install_schema_pins_match_python_rust_and_distribution_contract():
    schema = SCHEMAS[1].read_bytes()
    digest = hashlib.sha256(schema).hexdigest()
    for source in [
        ROOT / "crates/dcc-mcp-models/src/schema_validation.rs",
        ROOT / "python/dcc_mcp_core/deployment/install_sop.py",
    ]:
        assert '"' + digest + '"' in source.read_text(encoding="utf-8")
    contract = json.loads((ROOT / "compatibility/python.json").read_text(encoding="utf-8"))
    assert _current_wheel_resource(contract)["sha256"] == digest


@pytest.mark.parametrize("schema_path", SCHEMAS)
@pytest.mark.parametrize("source", ["remote", "cache", "bundled", "explicit", "unavailable"])
def test_catalog_provenance_accepts_each_observed_source(schema_path, source):
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema["$defs"]["catalog_provenance"])
    provenance = {"source": source, "latest_checked": source == "remote"}
    if source in {"remote", "cache"}:
        provenance.update(
            sha256="a" * 64,
            source_revision="b" * 40,
            issued_at=1000,
            expires_at=2000,
        )
    validator.validate(provenance)
    assert "catalog" not in schema["required"]


@pytest.mark.parametrize("schema_path", SCHEMAS)
@pytest.mark.parametrize(
    "field,value",
    [
        ("source", "arbitrary-remote"),
        ("latest_checked", "true"),
        ("sha256", "missing-digest"),
        ("source_revision", "main"),
        ("issued_at", -1),
        ("expires_at", "2000"),
        ("unexpected", True),
    ],
)
def test_catalog_provenance_rejects_malformed_evidence(schema_path, field, value):
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    validator = Draft202012Validator(schema["$defs"]["catalog_provenance"])
    provenance = {"source": "remote", "latest_checked": True, field: value}
    assert not validator.is_valid(provenance)


def test_existing_install_report_accepts_optional_catalog_and_rejects_invalid_evidence():
    schema = json.loads(SCHEMAS[1].read_text(encoding="utf-8"))
    validator = Draft202012Validator(schema)
    report = json.loads((ROOT / "tests/fixtures/install-execution-report-v1-success.json").read_text(encoding="utf-8"))
    validator.validate(report)
    report["catalog"] = {"source": "explicit", "latest_checked": False}
    validator.validate(report)
    invalid = deepcopy(report)
    invalid["catalog"]["sha256"] = "corrupt"
    assert not validator.is_valid(invalid)
