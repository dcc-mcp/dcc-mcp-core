"""Shared loader and invariants for the Python support contract."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

CONTRACT_RELATIVE_PATH = Path("compatibility") / "python.json"


class ContractError(ValueError):
    """Raised when the Python support contract is malformed."""


def repository_root() -> Path:
    """Return the repository root for scripts executed from any directory."""
    return Path(__file__).resolve().parents[2]


def load_contract(root: Path | None = None) -> dict[str, Any]:
    """Load and validate the machine-readable Python support contract."""
    repo_root = Path(root) if root is not None else repository_root()
    path = repo_root / CONTRACT_RELATIVE_PATH
    try:
        contract = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise ContractError(f"cannot load {path}: {exc}") from exc
    validate_contract(contract)
    return contract


def validate_contract(contract: dict[str, Any]) -> None:
    """Validate internal relationships before consumers use the contract."""
    if contract.get("schema_version") != 1:
        raise ContractError("schema_version must be 1")

    try:
        support = contract["support"]
        build = contract["build"]
        smoke = contract["runtime_smoke"]
        toolchain = contract["test_toolchain"]
        versions = support["tested_versions"]
        minimum = support["minimum_python"]
        maximum = support["maximum_tested_python"]
    except (KeyError, TypeError) as exc:
        raise ContractError(f"missing required contract field: {exc}") from exc

    if support.get("policy") != "long_term_support":
        raise ContractError("support.policy must be long_term_support")
    if minimum != "3.7":
        raise ContractError("schema_version 1 requires Python 3.7 as the LTS baseline")
    if not versions or versions[0] != minimum or versions[-1] != maximum:
        raise ContractError("tested_versions must span minimum_python through maximum_tested_python")
    if len(versions) != len(set(versions)):
        raise ContractError("tested_versions must not contain duplicates")
    try:
        minimum_minor = int(minimum.split(".")[1])
        maximum_minor = int(maximum.split(".")[1])
    except (IndexError, ValueError) as exc:
        raise ContractError("Python support versions must use major.minor form") from exc
    expected_versions = [f"3.{minor}" for minor in range(minimum_minor, maximum_minor + 1)]
    if versions != expected_versions:
        raise ContractError("tested_versions must be contiguous from minimum to maximum")

    deprecation = support.get("deprecation", {})
    if not deprecation.get("requires_major_release"):
        raise ContractError("Python 3.7 deprecation must require a major release")
    if not deprecation.get("requires_accepted_adr"):
        raise ContractError("Python 3.7 deprecation must require an accepted ADR")
    if int(deprecation.get("minimum_notice_days", 0)) < 180:
        raise ContractError("Python 3.7 deprecation notice must be at least 180 days")

    native = build.get("native_py37", {})
    lite = build.get("lite_py37", {})
    abi3 = build.get("abi3", {})
    if native.get("python") != minimum or lite.get("python") != minimum:
        raise ContractError("native_py37 and lite_py37 must target minimum_python")
    if native.get("wheel_tag") != "cp37-cp37m":
        raise ContractError("native_py37 wheel_tag must be cp37-cp37m")
    if lite.get("wheel_tag") != "py3-none-any":
        raise ContractError("lite_py37 wheel_tag must be py3-none-any")
    if abi3.get("wheel_tag") != "cp38-abi3":
        raise ContractError("abi3 wheel_tag must be cp38-abi3")
    if abi3.get("minimum_python") != "3.8" or abi3.get("feature") != "abi3-py38":
        raise ContractError("the modern wheel profile must remain abi3-py38")

    pr_matrix = native.get("pr_matrix", [])
    if not isinstance(pr_matrix, list) or not all(isinstance(row, dict) for row in pr_matrix):
        raise ContractError("native_py37.pr_matrix must be a list of objects")
    expected_platforms = {
        "linux-x86_64": {"runner": "ubuntu-22.04", "target": "x86_64"},
        "windows-x86_64": {"runner": "windows-2022", "target": "x64"},
    }
    platforms = {row.get("platform") for row in pr_matrix}
    if len(pr_matrix) != len(expected_platforms) or platforms != set(expected_platforms):
        raise ContractError("native_py37 PR coverage must include Linux and Windows x86_64")
    for row in pr_matrix:
        platform = row["platform"]
        expected = expected_platforms[platform]
        if row.get("runner") != expected["runner"] or row.get("target") != expected["target"]:
            raise ContractError(f"native_py37 {platform} runner or target is invalid")
        if not isinstance(row.get("full_suite"), bool):
            raise ContractError("native_py37 full_suite values must be booleans")
    full_suite_rows = [row for row in pr_matrix if row.get("full_suite")]
    if len(full_suite_rows) != 1 or full_suite_rows[0].get("platform") != "linux-x86_64":
        raise ContractError("the full Python 3.7 suite must run once on Linux x86_64")

    for profile in ("native_py37", "lite_py37"):
        imports = smoke.get(profile)
        if not isinstance(imports, list) or not imports:
            raise ContractError(f"runtime_smoke.{profile} must be a non-empty list")
    if "dcc_mcp_core._core" not in smoke["native_py37"]:
        raise ContractError("native_py37 smoke must import dcc_mcp_core._core")
    if "dcc_mcp_core._core" in smoke["lite_py37"]:
        raise ContractError("lite_py37 smoke must not require dcc_mcp_core._core")

    for name in ("pytest", "pytest_xdist", "typing_extensions"):
        if not toolchain.get(name):
            raise ContractError(f"test_toolchain.{name} is required")


def minimum_python_spec(contract: dict[str, Any]) -> str:
    """Return the canonical Requires-Python lower bound."""
    return f">={contract['support']['minimum_python']}"


def native_ci_matrix(contract: dict[str, Any]) -> dict[str, list[dict[str, Any]]]:
    """Return the GitHub Actions matrix for native Python 3.7 validation."""
    rows = []
    for platform in contract["build"]["native_py37"]["pr_matrix"]:
        rows.append(
            {
                "os": platform["runner"],
                "platform": platform["platform"],
                "target": platform["target"],
                "full_suite": bool(platform["full_suite"]),
            }
        )
    return {"include": rows}


def python37_test_requirements(contract: dict[str, Any]) -> list[str]:
    """Return the pinned test tools used by every Python 3.7 CI profile."""
    toolchain = contract["test_toolchain"]
    return [
        f"pytest=={toolchain['pytest']}",
        f"pytest-xdist=={toolchain['pytest_xdist']}",
        f"typing-extensions=={toolchain['typing_extensions']}",
    ]
