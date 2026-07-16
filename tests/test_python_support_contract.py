"""Regression tests for the machine-readable Python support contract."""

from __future__ import annotations

from pathlib import Path
import re

import pytest
from scripts.ci.check_python_support import collect_backfill_tooling_errors
from scripts.ci.check_python_support import collect_calendar_policy_errors
from scripts.ci.check_python_support import collect_distribution_projection_errors
from scripts.ci.check_python_support import collect_document_projection_errors
from scripts.ci.check_python_support import collect_expected_fragment_errors
from scripts.ci.check_python_support import collect_full_matrix_errors
from scripts.ci.check_python_support import collect_projection_errors
from scripts.ci.check_python_support import collect_semantic_release_errors
from scripts.ci.check_python_support import collect_wheel_action_errors
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

    contract = load_contract(_REPO_ROOT)
    contract["support"]["deprecation"]["automatic_calendar_expiry"] = True
    with pytest.raises(ContractError, match="automatic calendar expiry"):
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


def test_schema_v2_rejects_baseline_tag_and_version_gaps() -> None:
    contract = load_contract(_REPO_ROOT)
    contract["support"]["minimum_python"] = "3.8"
    contract["support"]["tested_versions"] = contract["support"]["tested_versions"][1:]
    with pytest.raises(ContractError, match=r"requires Python 3\.7"):
        validate_contract(contract)

    for profile, bad_tag, expected in (
        ("native_py37", "cp37", "cp37-cp37m"),
        ("lite_py37", "py3-any", "py3-none"),
        ("abi3", "cp38", "cp38-abi3"),
    ):
        contract = load_contract(_REPO_ROOT)
        contract["wheel_profiles"][profile]["wheel_tag"] = bad_tag
        with pytest.raises(ContractError, match=expected):
            validate_contract(contract)

    contract = load_contract(_REPO_ROOT)
    contract["support"]["tested_versions"].remove("3.8")
    with pytest.raises(ContractError, match="must be contiguous"):
        validate_contract(contract)


def test_contract_rejects_relaxed_core_or_semantic_linux_baselines() -> None:
    contract = load_contract(_REPO_ROOT)
    contract["wheel_profiles"]["native_py37"]["platforms"]["linux-x86_64"] = {
        "allowed_platform_tags": ["manylinux_2_28_x86_64"]
    }
    with pytest.raises(ContractError, match="legacy DCC platform baseline"):
        validate_contract(contract)

    contract = load_contract(_REPO_ROOT)
    contract["wheel_profiles"]["semantic_native_py37"]["platforms"]["linux-x86_64"] = {
        "allowed_platform_tags": ["manylinux_2_17_x86_64"]
    }
    with pytest.raises(ContractError, match="semantic_native_py37 linux-x86_64"):
        validate_contract(contract)


def test_document_projection_catches_english_and_chinese_drift(tmp_path: Path) -> None:
    contract = load_contract(_REPO_ROOT)
    contract["projections"]["requires_python_globs"] = ["*.md", "docs/**/*.md"]
    contract["projections"]["requires_python_exemptions"] = []
    english = tmp_path / "docs" / "guide" / "architecture.md"
    chinese = tmp_path / "docs" / "zh" / "guide" / "architecture.md"
    english.parent.mkdir(parents=True)
    chinese.parent.mkdir(parents=True)
    english.write_text('requires-python = ">=3.8"\n', encoding="utf-8")
    chinese.write_text('requires-python = ">=3.9"\n', encoding="utf-8")
    (tmp_path / "README.md").write_text('requires-python = ">=3.10"\n', encoding="utf-8")

    errors = collect_document_projection_errors(tmp_path, contract)
    assert any("docs/guide/architecture.md" in error for error in errors)
    assert any("docs/zh/guide/architecture.md" in error for error in errors)
    assert any("README.md" in error for error in errors)


def test_calendar_policy_rejects_any_expiry_date_without_flagging_metadata(tmp_path: Path) -> None:
    contract = load_contract(_REPO_ROOT)
    contract["projections"]["calendar_policy_globs"] = ["docs/**/*.md"]
    policy = tmp_path / "docs" / "policy.md"
    policy.parent.mkdir(parents=True)
    policy.write_text(
        "# Policy\n\nDate: 2026-07-10\n\nPython 3.7 has no automatic calendar expiry.\n",
        encoding="utf-8",
    )
    assert collect_calendar_policy_errors(tmp_path, contract) == []

    policy.write_text(
        "# Policy\n\nDate: 2026-07-10\n\nPython 3.7 support ends on 2027-12-31.\n\nPython 3.7 支持至 2035-01-01。\n",
        encoding="utf-8",
    )
    errors = collect_calendar_policy_errors(tmp_path, contract)
    assert len(errors) == 2


def test_semantic_extra_requires_a_python37_safe_fallback_marker(tmp_path: Path) -> None:
    contract = load_contract(_REPO_ROOT)
    contract["distributions"] = {
        "dcc-mcp-core": {
            "pyproject": "pyproject.toml",
            "support_profile": "lts",
        }
    }
    text = (_REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    text = text.replace("fastembed>=0.3; python_version>='3.8'", "fastembed>=0.3")
    (tmp_path / "pyproject.toml").write_text(text, encoding="utf-8")

    errors = collect_distribution_projection_errors(tmp_path, contract)
    assert any("fastembed fallback must be inactive" in error for error in errors)

    weakened = (_REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    weakened = weakened.replace(
        "fastembed>=0.3; python_version>='3.8'",
        "fastembed>=0.3; python_version>='3.8' or python_version<'3.7'",
    )
    (tmp_path / "pyproject.toml").write_text(weakened, encoding="utf-8")
    errors = collect_distribution_projection_errors(tmp_path, contract)
    assert any("fastembed fallback must be inactive" in error for error in errors)


def test_distribution_projection_ignores_commented_metadata(tmp_path: Path) -> None:
    contract = load_contract(_REPO_ROOT)
    contract["distributions"] = {
        "dcc-mcp-core": {
            "pyproject": "pyproject.toml",
            "support_profile": "lts",
        }
    }
    text = (_REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    text = text.replace(
        'requires-python = ">=3.7"',
        '# requires-python = ">=3.7"\nrequires-python = ">=3.8"',
        1,
    )
    classifier = '    "Programming Language :: Python :: 3.14",'
    text = text.replace(classifier, f"    # {classifier.strip()}", 1)
    (tmp_path / "pyproject.toml").write_text(text, encoding="utf-8")

    errors = collect_distribution_projection_errors(tmp_path, contract)
    assert any("requires-python is '>=3.8'" in error for error in errors)
    assert any("3.14" in error and "classifiers drifted" in error for error in errors)


def test_required_workflow_fragments_cannot_be_satisfied_by_comments(tmp_path: Path) -> None:
    contract = load_contract(_REPO_ROOT)
    workflow = tmp_path / ".github" / "workflows" / "ci.yml"
    workflow.parent.mkdir(parents=True)
    fragments = expected_fragments(contract)[".github/workflows/ci.yml"]
    workflow.write_text("\n".join(f"# {fragment}" for fragment in fragments), encoding="utf-8")

    errors = collect_expected_fragment_errors(tmp_path, contract)
    ci_errors = [error for error in errors if error.startswith(".github/workflows/ci.yml:")]
    assert len(ci_errors) == len(fragments)
    assert any("--extra semantic" in error for error in ci_errors)


def test_full_matrix_must_install_prebuilt_wheels_and_never_build_in_test_job(tmp_path: Path) -> None:
    contract = load_contract(_REPO_ROOT)
    workflow = tmp_path / ".github" / "workflows" / "python-matrix-full.yml"
    workflow.parent.mkdir(parents=True)
    current = (_REPO_ROOT / ".github" / "workflows" / "python-matrix-full.yml").read_text(encoding="utf-8")
    workflow.write_text(current, encoding="utf-8")
    assert collect_full_matrix_errors(tmp_path, contract) == []

    drifted = current.replace("needs: build-wheel", "needs: []")
    drifted = drifted.replace("pip install dist/*.whl", "vx just build\n          vx just stubgen")
    workflow.write_text(drifted, encoding="utf-8")
    errors = collect_full_matrix_errors(tmp_path, contract)
    assert any("needs: build-wheel" in error for error in errors)
    assert any("pip install dist/*.whl" in error for error in errors)
    assert any("vx just build" in error for error in errors)
    assert any("stubgen" in error for error in errors)

    wrong_artifact = current.replace("python-matrix-wheel-windows", "python-matrix-wheel-linux", 1)
    workflow.write_text(wrong_artifact, encoding="utf-8")
    errors = collect_full_matrix_errors(tmp_path, contract)
    assert any("python-matrix-wheel-windows" in error for error in errors)

    unsafe_windows = current.replace('maturin_args: ""', 'maturin_args: "--find-interpreter"')
    workflow.write_text(unsafe_windows, encoding="utf-8")
    errors = collect_full_matrix_errors(tmp_path, contract)
    assert any("disable --find-interpreter" in error for error in errors)

    version_in_comment_only = current.replace('"3.9", ', "").replace(
        "        python-version: [",
        '        # Removed matrix value remains in a comment: "3.9"\n        python-version: [',
        1,
    )
    workflow.write_text(version_in_comment_only, encoding="utf-8")
    errors = collect_full_matrix_errors(tmp_path, contract)
    assert any("expected" in error and "3.9" in error for error in errors)

    indirect = current.replace(
        "python -m pytest tests/ -q --tb=short --show-capture=no -n 4 --dist loadfile",
        "vx just test-suite",
    )
    workflow.write_text(indirect, encoding="utf-8")
    errors = collect_full_matrix_errors(tmp_path, contract)
    assert any("python -m pytest tests/" in error for error in errors)
    assert any("must not run 'vx just'" in error for error in errors)


def test_wheel_action_must_pin_manylinux_and_pass_explicit_platform(tmp_path: Path) -> None:
    action = tmp_path / ".github" / "actions" / "build-wheel" / "action.yml"
    action.parent.mkdir(parents=True)
    current = (_REPO_ROOT / ".github" / "actions" / "build-wheel" / "action.yml").read_text(encoding="utf-8")
    action.write_text(current, encoding="utf-8")
    assert collect_wheel_action_errors(tmp_path) == []

    drifted = re.sub(r"(?m)^(\s*manylinux:).*$", r"\1 auto", current)
    drifted = drifted.replace("--platform", "--target-platform")
    action.write_text(drifted, encoding="utf-8")
    errors = collect_wheel_action_errors(tmp_path)
    assert any("manylinux2014" in error for error in errors)
    assert any("--platform" in error for error in errors)

    inverted = current.replace("runner.os == 'Linux'", "runner.os != 'Linux'")
    action.write_text(inverted, encoding="utf-8")
    assert any("manylinux2014" in error for error in collect_wheel_action_errors(tmp_path))

    cached_linux = current.replace(" && runner.os != 'Linux'", "")
    action.write_text(cached_linux, encoding="utf-8")
    assert any("sccache must be disabled" in error for error in collect_wheel_action_errors(tmp_path))

    unsafe_windows = current.replace('maturin_args=""', 'maturin_args="--find-interpreter"')
    action.write_text(unsafe_windows, encoding="utf-8")
    assert any('maturin_args=""' in error for error in collect_wheel_action_errors(tmp_path))


def test_backfill_jobs_must_use_workflow_owned_validation_tooling(tmp_path: Path) -> None:
    workflow = tmp_path / ".github" / "workflows" / "build-wheels.yml"
    workflow.parent.mkdir(parents=True)
    current = (_REPO_ROOT / ".github" / "workflows" / "build-wheels.yml").read_text(encoding="utf-8")
    workflow.write_text(current, encoding="utf-8")
    assert collect_backfill_tooling_errors(tmp_path) == []

    drifted = current.replace("ref: ${{ github.workflow_sha }}", "ref: ${{ inputs.checkout-ref }}")
    drifted = drifted.replace("path: .workflow-tools", "path: .source-tools", 1)
    workflow.write_text(drifted, encoding="utf-8")
    errors = collect_backfill_tooling_errors(tmp_path)
    assert any("github.workflow_sha" in error for error in errors)
    assert any("path: .workflow-tools" in error for error in errors)

    comment_only = current.replace(
        "          ref: ${{ github.workflow_sha }}",
        "          # ref: ${{ github.workflow_sha }}\n          ref: ${{ inputs.checkout-ref }}",
    )
    workflow.write_text(comment_only, encoding="utf-8")
    errors = collect_backfill_tooling_errors(tmp_path)
    assert len([error for error in errors if "github.workflow_sha" in error]) == 6


def test_semantic_release_must_validate_every_wheel_before_upload(tmp_path: Path) -> None:
    workflow = tmp_path / ".github" / "workflows" / "release.yml"
    workflow.parent.mkdir(parents=True)
    current = (_REPO_ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
    workflow.write_text(current, encoding="utf-8")
    assert collect_semantic_release_errors(tmp_path) == []

    drifted = current.replace(
        ".workflow-tools/scripts/ci/check_python_wheel.py",
        ".workflow-tools/scripts/ci/removed_validator.py",
    )
    workflow.write_text(drifted, encoding="utf-8")
    errors = collect_semantic_release_errors(tmp_path)
    assert any("check_python_wheel.py" in error for error in errors)
    assert any("build, fetch current tooling, validate, then upload" in error for error in errors)

    comment_only = current.replace(
        "          ref: ${{ github.workflow_sha }}",
        "          # ref: ${{ github.workflow_sha }}\n          ref: ${{ github.ref }}",
    )
    workflow.write_text(comment_only, encoding="utf-8")
    assert any("github.workflow_sha" in error for error in collect_semantic_release_errors(tmp_path))
