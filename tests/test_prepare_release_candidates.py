"""Release-index candidates reuse the publisher without granting install authority."""

from __future__ import annotations

import copy
import hashlib
import io
import json
from pathlib import Path
import zipfile

import pytest
from scripts.ci import prepare_install_catalog as publisher
from scripts.ci import prepare_release_candidates as candidates

NAME = "dcc-mcp-openusd"
VERSION = "0.11.0"
COMMIT = "b" * 40
WHEEL_URL = f"https://files.pythonhosted.org/packages/example/dcc_mcp_openusd-{VERSION}-py3-none-any.whl"
PYPI_URL = f"https://pypi.org/pypi/{NAME}/{VERSION}/json"
LATEST_URL = f"https://pypi.org/pypi/{NAME}/json"
REF_URL = f"https://api.github.com/repos/dcc-mcp/{NAME}/git/ref/tags/v{VERSION}"
RUNBOOK_URL = f"https://raw.githubusercontent.com/dcc-mcp/{NAME}/{COMMIT}/install.md"


class Transport:
    def __init__(self, responses):
        self.responses = responses
        self.calls = []

    def fetch(self, url, limit, optional=False):
        self.calls.append(url)
        result = self.responses[url]
        if isinstance(result, Exception):
            raise result
        return result


def fixture(requirement="dcc-mcp-core>=0.20.36,<0.21.0", compatibility=False, extra=None, entry_points=None):
    """Build real wheel bytes with code which must never be executed."""
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w") as archive:
        archive.writestr("dcc_mcp_openusd/__init__.py", "raise RuntimeError('must never import adapter')")
        requirements = [] if requirement is None else [requirement]
        if isinstance(requirement, list):
            requirements = requirement
        metadata = f"Metadata-Version: 2.1\nName: {NAME}\nVersion: {VERSION}\nRequires-Python: >=3.9\n"
        metadata += "".join("Requires-Dist: " + value + "\n" for value in requirements)
        metadata += 'Requires-Dist: usd-core<27,>=24.11; extra == "openusd"\n\n'
        archive.writestr(f"dcc_mcp_openusd-{VERSION}.dist-info/METADATA", metadata)
        archive.writestr(
            f"dcc_mcp_openusd-{VERSION}.dist-info/entry_points.txt",
            entry_points or "[console_scripts]\ndcc-mcp-openusd = dcc_mcp_openusd.cli:main\n",
        )
        if compatibility:
            archive.writestr("dcc_mcp_openusd/compat_matrix.json", '{"schema_version":1,"hosts":["1.0.x"]}')
        for path, content in (extra or {}).items():
            archive.writestr(path, content)
    data = stream.getvalue()
    digest = hashlib.sha256(data).hexdigest()
    index = {
        "schema_version": 1,
        "sources": [{"name": NAME, "repository": f"dcc-mcp/{NAME}", "pypi_project": NAME}],
    }
    catalog = {
        "version": "1",
        "entries": [
            {
                "name": NAME,
                "description": "OpenUSD adapter",
                "dcc": ["openusd"],
                "url": f"https://github.com/dcc-mcp/{NAME}",
                "version": "0.8.1",
                "min_core_version": "0.19.45",
                "install": {
                    "type": "pip",
                    "pip_package": NAME,
                    "url": "https://files.pythonhosted.org/packages/old/dcc_mcp_openusd-0.8.1-py3-none-any.whl",
                    "sha256": "a" * 64,
                    "instructions_url": f"https://raw.githubusercontent.com/dcc-mcp/{NAME}/main/README.md",
                },
            }
        ],
    }
    artifact = {
        "filename": WHEEL_URL.rsplit("/", 1)[-1],
        "url": WHEEL_URL,
        "digests": {"sha256": digest},
        "size": len(data),
        "packagetype": "bdist_wheel",
        "requires_python": ">=3.9",
        "yanked": False,
    }
    pypi = {
        "info": {"name": NAME, "version": VERSION, "requires_python": ">=3.9", "yanked": False},
        "urls": [artifact],
    }
    transport = Transport(
        {
            LATEST_URL: json.dumps(pypi).encode(),
            PYPI_URL: json.dumps(pypi).encode(),
            WHEEL_URL: data,
            REF_URL: json.dumps({"object": {"type": "commit", "sha": COMMIT}}).encode(),
            RUNBOOK_URL: b"# Install\n",
        }
    )
    return index, catalog, transport, pypi


def prepare(index, catalog, transport, selected=None):
    """Use a deterministic observation time and optional exact selection."""
    return candidates.prepare_candidates(index, catalog, selected or {}, 1234567, transport)


@pytest.mark.parametrize("number", ["NaN", "Infinity", "-Infinity", "1e309", "-1e309"])
def test_packaged_compatibility_rejects_nonfinite_json(number):
    path = "dcc_mcp_openusd/compat_matrix.json"
    raw = '{"schema_version":1,"nested":{"value":' + number + "}}"
    index, catalog, transport, _ = fixture(extra={path: raw})
    index["sources"][0]["compatibility_path"] = path
    with pytest.raises(publisher.CatalogPreparationError, match="invalid packaged compatibility JSON"):
        prepare(index, catalog, transport, {NAME: VERSION})


@pytest.mark.parametrize("number", [0.0, 1e308, -1e308])
def test_packaged_compatibility_preserves_finite_json_numbers(number):
    path = "dcc_mcp_openusd/compat_matrix.json"
    document = {"schema_version": 1, "nested": {"value": number}}
    index, catalog, transport, _ = fixture(extra={path: json.dumps(document, allow_nan=False)})
    index["sources"][0]["compatibility_path"] = path
    report = prepare(index, catalog, transport, {NAME: VERSION})
    assert report["candidates"][0]["compatibility"]["packaged_compatibility"]["document"] == document
    assert json.loads(json.dumps(report, allow_nan=False)) == report


def test_latest_is_review_evidence_never_an_install_selection():
    index, catalog, transport, _ = fixture()
    original = copy.deepcopy(catalog)
    report = prepare(index, catalog, transport)
    item = report["candidates"][0]
    assert catalog == original
    assert report["authority"] == "candidate_only"
    assert report["observed_at"] == 1234567
    assert item["selection"] == "observed_latest"
    assert item["status"] == "review_required"
    assert item["source_commit"] == COMMIT
    assert item["artifact"]["sha256"] == hashlib.sha256(transport.responses[WHEEL_URL]).hexdigest()
    assert item["catalog_entry"]["install"]["instructions_url"] == RUNBOOK_URL
    assert item["compatibility"]["host_status"] == "not_checked"
    assert item["compatibility"]["native_extras_status"] == "not_checked"
    assert item["provenance"]["verification"] == "not_checked"
    assert "sop_version" not in item["catalog_entry"]["install"]
    with pytest.raises(publisher.CatalogPreparationError, match="explicit versions"):
        candidates.export_catalog(report)


def test_explicit_selection_exports_legacy_catalog_and_reuses_existing_publisher():
    index, catalog, transport, _ = fixture()
    report = prepare(index, catalog, transport, {NAME: VERSION})
    exported = candidates.export_catalog(report)
    assert LATEST_URL not in transport.calls
    assert exported["version"] == "1"
    assert exported["entries"][0]["version"] == VERSION
    assert exported["entries"][0]["core_requirement"] == ">=0.20.36,<0.21.0"
    payload = publisher.prepare_catalog(exported, "a" * 40, 1234567, transport)
    assert payload["entries"] == exported["entries"]
    assert payload["schema_version"] == 1
    assert "authority" not in payload
    assert payload["expires_at"] == 1234567 + publisher.VALIDITY_SECONDS


@pytest.mark.parametrize(
    ("requirement", "code"),
    [
        ("dcc-mcp-core>=0.19.45,<1.0.0", "core_upper_bound_too_wide"),
        ("dcc-mcp-core>=0.20.36", "core_missing_upper_bound"),
        (None, "core_requirement_missing_ambiguous_or_conditional"),
        (
            ["dcc-mcp-core>=0.20.0,<0.21.0", "dcc_mcp_core>=0.20.2,<0.21.0"],
            "core_requirement_missing_ambiguous_or_conditional",
        ),
        ('dcc-mcp-core>=0.20.0,<0.21.0; python_version > "3.8"', "core_requirement_missing_ambiguous_or_conditional"),
    ],
)
def test_invalid_core_claim_blocks_export_without_inventing_bounds(requirement, code):
    index, catalog, transport, _ = fixture(requirement)
    report = prepare(index, catalog, transport, {NAME: VERSION})
    item = report["candidates"][0]
    assert item["status"] == "blocked"
    assert code in item["blockers"]
    assert item["catalog_entry"] is None
    with pytest.raises(publisher.CatalogPreparationError, match="promotion blockers"):
        candidates.export_catalog(report)


@pytest.mark.parametrize("field", ["repository", "pypi_project", "name", "url"])
def test_index_cannot_rebind_an_existing_entry_or_supply_an_arbitrary_url(field):
    index, catalog, transport, _ = fixture()
    index["sources"][0][field] = "attacker/other"
    with pytest.raises(publisher.CatalogPreparationError):
        prepare(index, catalog, transport)
    assert not transport.calls


def test_duplicate_sources_and_unknown_selection_fail_before_network():
    index, catalog, transport, _ = fixture()
    index["sources"].append(copy.deepcopy(index["sources"][0]))
    with pytest.raises(publisher.CatalogPreparationError, match="uniquely bind"):
        prepare(index, catalog, transport)
    index["sources"].pop()
    with pytest.raises(publisher.CatalogPreparationError, match="belong to"):
        prepare(index, catalog, transport, {"dcc-mcp-unknown": VERSION})
    assert not transport.calls


@pytest.mark.parametrize("version", ["latest", "main", "0.11", "0.11.0rc1", "v0.11.0", "0.11.0/../json"])
def test_floating_or_unsupported_versions_are_rejected(version):
    index, catalog, transport, _ = fixture()
    with pytest.raises(publisher.CatalogPreparationError, match="exact stable"):
        prepare(index, catalog, transport, {NAME: version})
    assert not transport.calls


@pytest.mark.parametrize(
    "mutation", ["yanked", "digest", "identity", "requires_python", "size", "missing", "duplicate"]
)
def test_publisher_artifact_validation_is_not_bypassed(mutation):
    index, catalog, transport, pypi = fixture()
    artifact = pypi["urls"][0]
    if mutation == "yanked":
        artifact["yanked"] = True
    elif mutation == "digest":
        artifact["digests"]["sha256"] = "0" * 64
    elif mutation == "identity":
        pypi["info"]["name"] = "unrelated"
    elif mutation == "requires_python":
        artifact["requires_python"] = ">=3.7"
    elif mutation == "size":
        artifact["size"] += 1
    elif mutation == "missing":
        pypi["urls"] = []
    else:
        pypi["urls"].append(copy.deepcopy(artifact))
    transport.responses[PYPI_URL] = json.dumps(pypi).encode()
    with pytest.raises(publisher.CatalogPreparationError):
        prepare(index, catalog, transport, {NAME: VERSION})


def test_packaged_host_matrix_is_digest_bound_but_not_live_compatibility():
    index, catalog, transport, _ = fixture(compatibility=True)
    index["sources"][0]["compatibility_path"] = "dcc_mcp_openusd/compat_matrix.json"
    evidence = prepare(index, catalog, transport)["candidates"][0]["compatibility"]
    assert evidence["packaged_compatibility"]["document"]["hosts"] == ["1.0.x"]
    assert len(evidence["packaged_compatibility"]["sha256"]) == 64
    assert evidence["host_status"] == "not_checked"


@pytest.mark.parametrize("path", ["../compat.json", "/tmp/compat.json", "dcc_mcp_openusd/../compat.json"])
def test_compatibility_path_cannot_escape_verified_archive(path):
    index, catalog, transport, _ = fixture()
    index["sources"][0]["compatibility_path"] = path
    with pytest.raises(publisher.CatalogPreparationError, match="relative JSON"):
        prepare(index, catalog, transport)
    assert not transport.calls


def test_missing_indexed_compatibility_fails_closed():
    index, catalog, transport, _ = fixture()
    index["sources"][0]["compatibility_path"] = "dcc_mcp_openusd/compat_matrix.json"
    with pytest.raises(publisher.CatalogPreparationError, match="missing"):
        prepare(index, catalog, transport)


def test_policy_and_downgrade_are_not_overridden_by_new_release_metadata():
    index, catalog, transport, _ = fixture()
    catalog["entries"][0]["version"] = "0.12.0"
    catalog["entries"][0]["policy"] = {"installation": "not_available", "reason": "Known host regression"}
    item = prepare(index, catalog, transport)["candidates"][0]
    assert set(item["blockers"]) == {
        "candidate_older_than_approved",
        "installation_policy_not_available",
        "curated_policy_requires_review",
    }
    assert item["catalog_entry"] is None


def test_network_failure_is_not_reported_as_latest_or_a_successful_observation():
    index, catalog, transport, _ = fixture()
    transport.responses[LATEST_URL] = publisher.CatalogPreparationError("HTTP verification failed")
    with pytest.raises(publisher.CatalogPreparationError, match="invalid remote JSON metadata"):
        prepare(index, catalog, transport)


def test_cli_failure_preserves_previous_output_and_inputs(tmp_path, monkeypatch):
    index, catalog, transport, _ = fixture("dcc-mcp-core>=0.20.36,<1.0.0")
    monkeypatch.setattr(publisher, "HttpTransport", lambda token: transport)
    index_path, catalog_path = tmp_path / "index.json", tmp_path / "catalog.json"
    report_path, export_path = tmp_path / "report.json", tmp_path / "export.json"
    index_path.write_text(json.dumps(index), encoding="utf-8")
    catalog_path.write_text(json.dumps(catalog), encoding="utf-8")
    report_path.write_text("old report", encoding="utf-8")
    export_path.write_text("old export", encoding="utf-8")
    args = ["--index", str(index_path), "--catalog", str(catalog_path), "--output", str(report_path)]
    assert candidates.main([*args, "--select", f"{NAME}=={VERSION}", "--catalog-output", str(export_path)]) == 1
    assert report_path.read_text(encoding="utf-8") == "old report"
    assert export_path.read_text(encoding="utf-8") == "old export"
    assert candidates.main([*args, "--output", str(catalog_path)]) == 1
    assert json.loads(catalog_path.read_text(encoding="utf-8")) == catalog


def test_existing_legacy_curated_catalog_still_prepares_without_index():
    _, catalog, transport, _ = fixture()
    entry = catalog["entries"][0]
    entry["version"] = VERSION
    entry["install"]["url"] = WHEEL_URL
    entry["install"]["sha256"] = hashlib.sha256(transport.responses[WHEEL_URL]).hexdigest()
    payload = publisher.prepare_catalog(catalog, "a" * 40, 1234567, transport)
    assert "core_requirement" not in payload["entries"][0]
    assert payload["entries"][0]["min_core_version"] == "0.19.45"
    assert LATEST_URL not in transport.calls


def test_repository_index_is_only_bindings_without_release_pins():
    root = Path(__file__).resolve().parents[1]
    index = json.loads((root / "compatibility/adapter-release-index.json").read_text(encoding="utf-8"))
    assert {source["name"] for source in index["sources"]} == {NAME, "dcc-mcp-freecad"}
    assert all("version" not in source and "sha256" not in source for source in index["sources"])


def test_same_approved_version_cannot_replace_digest_or_move_pinned_commit():
    index, catalog, transport, _ = fixture()
    entry = catalog["entries"][0]
    entry["version"] = VERSION
    entry["install"]["instructions_url"] = RUNBOOK_URL.replace(COMMIT, "c" * 40)
    item = prepare(index, catalog, transport, {NAME: VERSION})["candidates"][0]
    assert set(item["blockers"]) == {
        "artifact_changed_for_approved_version",
        "release_commit_changed_for_approved_version",
    }
    assert item["catalog_entry"] is None


@pytest.mark.parametrize("version", [True, "1", 1.0, 2])
def test_index_version_is_strict_integer_one(version):
    index, catalog, transport, _ = fixture()
    index["schema_version"] = version
    with pytest.raises(publisher.CatalogPreparationError, match="schema"):
        prepare(index, catalog, transport)
    assert not transport.calls


@pytest.mark.parametrize("field,value", [("install", []), ("policy", []), ("description", None)])
def test_malformed_approved_entry_fails_before_network(field, value):
    index, catalog, transport, _ = fixture()
    catalog["entries"][0][field] = value
    with pytest.raises(publisher.CatalogPreparationError, match="metadata or policy"):
        prepare(index, catalog, transport)
    assert not transport.calls


def test_reports_conform_to_closed_schema_and_never_claim_install_authority():
    jsonschema = pytest.importorskip("jsonschema")
    schema_path = Path(__file__).resolve().parents[1] / "contracts/adapter-release-candidate-v1.schema.json"
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    validator = jsonschema.Draft202012Validator(schema)
    validator.check_schema(schema)
    for requirement in ("dcc-mcp-core>=0.20.36,<0.21.0", "dcc-mcp-core>=0.20.36,<1.0.0"):
        index, catalog, transport, _ = fixture(requirement, compatibility=True)
        index["sources"][0]["compatibility_path"] = "dcc_mcp_openusd/compat_matrix.json"
        report = prepare(index, catalog, transport)
        validator.validate(report)
        invalid = copy.deepcopy(report)
        invalid["authority"] = "official_verified"
        assert list(validator.iter_errors(invalid))
        invalid = copy.deepcopy(report)
        invalid["latest_checked"] = True
        assert list(validator.iter_errors(invalid))
        invalid = copy.deepcopy(report)
        invalid["candidates"][0]["compatibility"]["host_status"] = "available"
        assert list(validator.iter_errors(invalid))
        invalid = copy.deepcopy(report)
        invalid["candidates"][0]["compatibility"]["core_bounds"]["install_authorized"] = True
        assert list(validator.iter_errors(invalid))


@pytest.mark.parametrize(
    "requirement",
    [
        "dcc-mcp-core @ https://example.com/a>=0.20.0,<0.21.0",
        "dcc-mcp-core@https://example.com/a>=0.20.0,<0.21.0",
        "DCC.__MCP_-CORE@https://example.com/core.whl",
        "dcc-mcp-core nonsense>=0.20.0,<0.21.0",
        "dcc-mcp-core (>=0.20.0,<0.21.0",
        "dcc-mcp-core >=0.20.0,,<0.21.0",
        "dcc-mcp-core >=0.20.0,<0.21.0,",
        "dcc-mcp-core~=0.20",
        "dcc-mcp-core==0.20.*",
        "dcc-mcp-core>=0.20.0rc1,<0.21.0",
    ],
)
def test_policy_parser_does_not_replace_requirement_syntax_validation(requirement):
    index, catalog, transport, _ = fixture(requirement)
    item = prepare(index, catalog, transport, {NAME: VERSION})["candidates"][0]
    assert item["blockers"] == ["core_requirement_invalid_syntax"]
    assert item["catalog_entry"] is None


def test_empty_core_range_is_not_exportable():
    index, catalog, transport, _ = fixture("dcc-mcp-core>0.20.0,<=0.20.0")
    item = prepare(index, catalog, transport, {NAME: VERSION})["candidates"][0]
    assert item["blockers"] == ["core_empty_range"]
    assert item["catalog_entry"] is None


def test_valid_parenthesized_requirement_preserves_existing_bound_policy():
    index, catalog, transport, _ = fixture("dcc-mcp-core (>=0.20.36, <0.21.0)")
    item = prepare(index, catalog, transport, {NAME: VERSION})["candidates"][0]
    assert not item["blockers"]
    assert item["catalog_entry"]["core_requirement"] == ">=0.20.36,<0.21.0"


def test_entrypoint_metadata_preserves_case_and_rejects_default_inheritance():
    index, catalog, transport, _ = fixture(entry_points="[console_scripts]\nMixedCase = dcc_mcp_openusd.cli:main\n")
    item = prepare(index, catalog, transport)["candidates"][0]
    assert "MixedCase" in item["compatibility"]["entry_points"]["console_scripts"]
    index, catalog, transport, _ = fixture(entry_points="[DEFAULT]\nphantom = bad\n[console_scripts]\nReal = good\n")
    with pytest.raises(publisher.CatalogPreparationError, match="DEFAULT"):
        prepare(index, catalog, transport)


@pytest.mark.parametrize("field", ["url", "pip_package"])
def test_nonstring_source_binding_fails_cleanly(field):
    index, catalog, transport, _ = fixture()
    target = catalog["entries"][0] if field == "url" else catalog["entries"][0]["install"]
    target[field] = None
    with pytest.raises(publisher.CatalogPreparationError):
        prepare(index, catalog, transport)
    assert not transport.calls


def test_recorded_public_pilot_preserves_blockers_without_becoming_offline_authority():
    jsonschema = pytest.importorskip("jsonschema")
    root = Path(__file__).resolve().parents[1]
    report = json.loads(
        (root / "tests/fixtures/release-index/public-observation-2026-10-13.json").read_text(encoding="utf-8")
    )
    schema = json.loads((root / "contracts/adapter-release-candidate-v1.schema.json").read_text(encoding="utf-8"))
    jsonschema.Draft202012Validator(schema).validate(report)
    assert {item["name"] for item in report["candidates"]} == {"dcc-mcp-freecad", "dcc-mcp-openusd"}
    for item in report["candidates"]:
        assert item["blockers"] == ["core_upper_bound_too_wide"]
        assert item["catalog_entry"] is None
        assert item["provenance"]["verification"] == "not_checked"
    with pytest.raises(publisher.CatalogPreparationError):
        candidates.export_catalog(report)


def test_unchanged_core_requirement_is_not_reported_as_a_change():
    index, catalog, transport, _ = fixture()
    catalog["entries"][0]["core_requirement"] = ">=0.20.36,<0.21.0"
    item = prepare(index, catalog, transport)["candidates"][0]
    assert item["catalog_entry"]["core_requirement"] == catalog["entries"][0]["core_requirement"]
    assert "core_requirement" not in item["changes"]
    assert item["compatibility"]["core_bounds"]["ok"] is True


@pytest.mark.parametrize("name", ["dcc--mcp--core", "DCC.__MCP_-CORE", "dcc_mcp_core", "DCC.MCP.CORE"])
def test_equivalent_core_distribution_names_preserve_declared_bounds(name):
    declaration = name + ">=0.20.36,<0.21.0"
    index, catalog, transport, _ = fixture(declaration)
    report = prepare(index, catalog, transport, {NAME: VERSION})
    item = report["candidates"][0]
    assert item["blockers"] == []
    assert item["compatibility"]["core_bounds"]["declaration"] == declaration
    assert candidates.export_catalog(report)["entries"][0]["core_requirement"] == ">=0.20.36,<0.21.0"


@pytest.mark.parametrize(
    "other",
    [
        "dcc--mcp--core>=0.21.0,<0.22.0",
        "DCC.__MCP_-CORE>=0.21.0,<0.22.0",
        "dcc-mcp-core@https://example.com/core.whl",
        "dcc--mcp--core[extra]>=0.20.36,<0.21.0",
        'dcc--mcp--core>=0.21.0,<0.22.0; python_version >= "3.9"',
    ],
)
def test_equivalent_or_direct_core_requirements_cannot_hide_duplicate_declarations(other):
    index, catalog, transport, _ = fixture(["dcc-mcp-core>=0.20.36,<0.21.0", other])
    report = prepare(index, catalog, transport, {NAME: VERSION})
    item = report["candidates"][0]
    assert item["blockers"] == ["core_requirement_missing_ambiguous_or_conditional"]
    assert item["catalog_entry"] is None
    with pytest.raises(publisher.CatalogPreparationError, match="promotion blockers"):
        candidates.export_catalog(report)


@pytest.mark.parametrize("name", ["dcc-mcp-core-extra", "dcc-mcp-core2", "dcc-mcp-core.tools"])
def test_distinct_core_prefixed_distributions_do_not_create_false_duplicates(name):
    index, catalog, transport, _ = fixture(["dcc-mcp-core>=0.20.36,<0.21.0", name + ">=1.0.0"])
    report = prepare(index, catalog, transport, {NAME: VERSION})
    assert report["candidates"][0]["blockers"] == []
    assert candidates.export_catalog(report)["entries"][0]["core_requirement"] == ">=0.20.36,<0.21.0"
