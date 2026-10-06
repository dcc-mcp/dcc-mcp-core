"""Tests for the adapter-to-core requirement contract and its CI gate.

The contract records adapters whose package-environment requirement is wider
than the range their PyPI metadata declares. These tests keep the file honest:
every drift code in it must be one the shared parser recomputes from the same
two inputs, and the gate must turn a freshly unbounded adapter checkout into an
error rather than a note.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
from scripts.ci import adapter_core_requirement_contract as contract_module
from scripts.ci.check_adapter_core_requirement import check_adapter_root
from scripts.ci.check_adapter_core_requirement import check_contract
from scripts.ci.check_adapter_core_requirement import main

REPO_ROOT = Path(__file__).resolve().parent.parent
CONTRACT_PATH = REPO_ROOT / contract_module.CONTRACT_RELATIVE_PATH


def _write_adapter(root: Path, dependency: str, requires: str) -> Path:
    """Write a minimal adapter checkout declaring *dependency* and *requires*."""
    root.mkdir(parents=True, exist_ok=True)
    (root / "pyproject.toml").write_text(
        '[project]\nname = "dcc-mcp-example"\nversion = "1.0.0"\n'
        f"dependencies = [\n    {dependency!r},\n]\n",
        encoding="utf-8",
    )
    (root / "package.py").write_text(
        f'name = "dcc_mcp_example"\nversion = "1.0.0"\nrequires = [{requires!r}]\n',
        encoding="utf-8",
    )
    return root


def test_committed_contract_is_valid() -> None:
    """The contract in the repository satisfies every invariant."""
    contract = contract_module.load_contract(REPO_ROOT)
    assert contract["schema_version"] == 1
    assert contract["policy"]["upper_bound_required"] is True
    assert contract["known_violations"]


def test_contract_rejects_a_wrong_source_of_truth() -> None:
    """Only the Python distribution metadata may define the bound."""
    contract = contract_module.load_contract(REPO_ROOT)
    contract["policy"]["source_of_truth"] = "package_environment"
    with pytest.raises(contract_module.ContractError, match="source of truth"):
        contract_module.validate_contract(contract)


def test_contract_rejects_a_wrong_schema_version() -> None:
    """A bumped schema version without a loader update fails loudly."""
    contract = contract_module.load_contract(REPO_ROOT)
    contract["schema_version"] = 2
    with pytest.raises(contract_module.ContractError, match="schema_version must be 1"):
        contract_module.validate_contract(contract)


def test_contract_rejects_a_stale_expected_token() -> None:
    """A hand-edited expected token must match what the parser derives."""
    contract = contract_module.load_contract(REPO_ROOT)
    contract["known_violations"][0]["expected_package_environment_requirement"] = "dcc_mcp_core-0"
    with pytest.raises(contract_module.ContractError, match="expected_package_environment_requirement"):
        contract_module.validate_contract(contract)


def test_contract_rejects_stale_drift_codes() -> None:
    """Drift codes are recomputed, not curated by hand."""
    contract = contract_module.load_contract(REPO_ROOT)
    contract["known_violations"][0]["codes"] = []
    with pytest.raises(contract_module.ContractError, match="do not match the recomputed drift"):
        contract_module.validate_contract(contract)


def test_contract_rejects_duplicate_rows() -> None:
    """One row per adapter version, so the survey cannot double-count."""
    contract = contract_module.load_contract(REPO_ROOT)
    contract["known_violations"].append(dict(contract["known_violations"][0]))
    with pytest.raises(contract_module.ContractError, match="duplicate known_violations"):
        contract_module.validate_contract(contract)


def test_contract_rejects_an_unknown_status() -> None:
    """A row must be either open or fixed."""
    contract = contract_module.load_contract(REPO_ROOT)
    contract["known_violations"][0]["status"] = "maybe"
    with pytest.raises(contract_module.ContractError, match="status must be open or fixed"):
        contract_module.validate_contract(contract)


def test_check_contract_reports_open_rows_as_warnings() -> None:
    """Tracked adapter drift is reported, never silently accepted."""
    contract = contract_module.load_contract(REPO_ROOT)
    findings = check_contract(contract)
    assert findings
    assert all(finding.severity == "warning" for finding in findings)
    assert all(finding.scope for finding in findings)


def test_gate_flags_a_package_environment_requirement_that_is_too_wide(tmp_path: Path) -> None:
    """The reported failure: ``dcc_mcp_core-0`` next to a declared ``<0.19.5``."""
    root = _write_adapter(tmp_path / "drifted", "dcc-mcp-core>=0.19.3,<0.19.5", "dcc_mcp_core-0")
    contract = contract_module.load_contract(REPO_ROOT)
    findings = check_adapter_root(root, contract_module.core_requirement_module(), contract)
    codes = {finding.code for finding in findings}
    assert {"lower_bound_too_low", "upper_bound_too_high"} <= codes
    assert all(finding.severity == "error" for finding in findings)
    assert any("dcc_mcp_core-0.19.3..0.19.5" in finding.message for finding in findings)


def test_gate_accepts_a_derived_requirement(tmp_path: Path) -> None:
    """A token generated from the declared range passes with no findings."""
    root = _write_adapter(tmp_path / "clean", "dcc-mcp-core>=0.20.0,<0.21.0", "dcc_mcp_core-0.20.0..0.21.0")
    contract = contract_module.load_contract(REPO_ROOT)
    assert check_adapter_root(root, contract_module.core_requirement_module(), contract) == []


def test_gate_warns_about_the_open_ceiling_placeholder(tmp_path: Path) -> None:
    """``<1.0.0`` is a placeholder, not a version anybody verified against."""
    root = _write_adapter(tmp_path / "open", "dcc-mcp-core>=0.20.14,<1.0.0", "dcc_mcp_core-0.20.14..1.0.0")
    contract = contract_module.load_contract(REPO_ROOT)
    findings = check_adapter_root(root, contract_module.core_requirement_module(), contract)
    assert [finding.code for finding in findings] == ["open_ceiling"]
    assert findings[0].severity == "warning"


def test_gate_flags_a_missing_package_environment_requirement(tmp_path: Path) -> None:
    """Not requiring core at all leaves the resolver completely unconstrained."""
    root = _write_adapter(tmp_path / "absent", "dcc-mcp-core>=0.19.3,<0.19.5", "maya_scene_skills-1.2+")
    contract = contract_module.load_contract(REPO_ROOT)
    findings = check_adapter_root(root, contract_module.core_requirement_module(), contract)
    assert [finding.code for finding in findings] == ["missing_package_environment_requirement"]
    assert findings[0].severity == "error"


def test_gate_treats_an_unparseable_requires_list_as_an_error(tmp_path: Path) -> None:
    """A requirement the gate cannot read is a violation, never an exemption."""
    root = _write_adapter(tmp_path / "dynamic", "dcc-mcp-core>=0.19.3,<0.19.5", "dcc_mcp_core-0")
    (root / "package.py").write_text(
        'requirements = ["dcc_mcp_core-0"]\nrequires = requirements\n',
        encoding="utf-8",
    )
    contract = contract_module.load_contract(REPO_ROOT)
    findings = check_adapter_root(root, contract_module.core_requirement_module(), contract)
    assert [finding.code for finding in findings] == ["unparseable"]
    assert findings[0].severity == "error"


def test_main_exits_zero_on_the_repository() -> None:
    """Core's own tree is clean; the tracked rows are warnings only."""
    assert main(["--root", str(REPO_ROOT)]) == 0


def test_main_exits_non_zero_for_a_drifted_adapter(tmp_path: Path) -> None:
    """An adapter checkout with drift fails the run that introduced it."""
    root = _write_adapter(tmp_path / "drifted", "dcc-mcp-core>=0.19.3,<0.19.5", "dcc_mcp_core-0")
    assert main(["--root", str(REPO_ROOT), "--adapter-root", str(root)]) == 1


def test_main_reports_a_missing_adapter_root(tmp_path: Path) -> None:
    """A path that is not a checkout is reported instead of being skipped."""
    assert main(["--root", str(REPO_ROOT), "--adapter-root", str(tmp_path / "nope")]) == 1


def test_main_emits_json(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    """Machine-readable output keeps the findings usable in adapter CI."""
    root = _write_adapter(tmp_path / "drifted", "dcc-mcp-core>=0.19.3,<0.19.5", "dcc_mcp_core-0")
    assert main(["--root", str(REPO_ROOT), "--adapter-root", str(root), "--json"]) == 1
    payload = capsys.readouterr().out
    assert '"severity": "error"' in payload
    assert "lower_bound_too_low" in payload
