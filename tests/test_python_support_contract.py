"""Regression tests for the machine-readable Python support contract."""

from __future__ import annotations

from pathlib import Path

import pytest
from scripts.ci.check_python_support import collect_projection_errors
from scripts.ci.check_python_support import expected_fragments
from scripts.ci.python_support_contract import ContractError
from scripts.ci.python_support_contract import load_contract
from scripts.ci.python_support_contract import native_ci_matrix
from scripts.ci.python_support_contract import python37_test_requirements
from scripts.ci.python_support_contract import validate_contract

_REPO_ROOT = Path(__file__).resolve().parents[1]


def test_contract_is_internally_valid_and_matches_repository() -> None:
    contract = load_contract(_REPO_ROOT)
    assert collect_projection_errors(_REPO_ROOT, contract) == []


def test_contract_rejects_calendar_expiry_or_weak_deprecation() -> None:
    contract = load_contract(_REPO_ROOT)
    contract["support"]["deprecation"]["minimum_notice_days"] = 30
    with pytest.raises(ContractError, match="at least 180 days"):
        validate_contract(contract)


def test_pyo3_series_is_a_required_projection() -> None:
    contract = load_contract(_REPO_ROOT)
    fragments = expected_fragments(contract)["Cargo.toml"]
    assert 'pyo3 = { version = "0.28"' in fragments


def test_native_python37_requires_linux_and_windows() -> None:
    contract = load_contract(_REPO_ROOT)
    contract["build"]["native_py37"]["pr_matrix"] = [contract["build"]["native_py37"]["pr_matrix"][0]]
    with pytest.raises(ContractError, match="Linux and Windows"):
        validate_contract(contract)


def test_native_matrix_and_test_toolchain_are_generated_from_contract() -> None:
    contract = load_contract(_REPO_ROOT)
    matrix = native_ci_matrix(contract)
    assert [row["platform"] for row in matrix["include"]] == [
        "linux-x86_64",
        "windows-x86_64",
    ]
    assert [row["platform"] for row in matrix["include"] if row["full_suite"]] == ["linux-x86_64"]
    assert python37_test_requirements(contract) == [
        "pytest==7.4.4",
        "pytest-xdist==3.5.0",
        "typing-extensions==4.7.1",
    ]


def test_native_matrix_rejects_runner_and_boolean_drift() -> None:
    contract = load_contract(_REPO_ROOT)
    contract["build"]["native_py37"]["pr_matrix"][0]["runner"] = "windows-2022"
    with pytest.raises(ContractError, match="runner or target"):
        validate_contract(contract)

    contract = load_contract(_REPO_ROOT)
    contract["build"]["native_py37"]["pr_matrix"][0]["full_suite"] = "true"
    with pytest.raises(ContractError, match="must be booleans"):
        validate_contract(contract)


def test_schema_v1_rejects_baseline_tag_and_version_gaps() -> None:
    contract = load_contract(_REPO_ROOT)
    contract["support"]["minimum_python"] = "3.8"
    contract["support"]["tested_versions"] = contract["support"]["tested_versions"][1:]
    with pytest.raises(ContractError, match=r"requires Python 3\.7"):
        validate_contract(contract)

    for profile, bad_tag, expected in (
        ("native_py37", "cp37", "cp37-cp37m"),
        ("lite_py37", "py3-any", "py3-none-any"),
        ("abi3", "cp38", "cp38-abi3"),
    ):
        contract = load_contract(_REPO_ROOT)
        contract["build"][profile]["wheel_tag"] = bad_tag
        with pytest.raises(ContractError, match=expected):
            validate_contract(contract)

    contract = load_contract(_REPO_ROOT)
    contract["support"]["tested_versions"].remove("3.8")
    with pytest.raises(ContractError, match="must be contiguous"):
        validate_contract(contract)
