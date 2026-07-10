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
    if contract.get("schema_version") != 2:
        raise ContractError("schema_version must be 2")

    try:
        support = contract["support"]
        build = contract["build"]
        smoke = contract["runtime_smoke"]
        toolchain = contract["test_toolchain"]
        distributions = contract["distributions"]
        optional_dependencies = contract["optional_dependencies"]
        wheel_profiles = contract["wheel_profiles"]
        projections = contract["projections"]
        versions = support["tested_versions"]
        minimum = support["minimum_python"]
        maximum = support["maximum_tested_python"]
    except (KeyError, TypeError) as exc:
        raise ContractError(f"missing required contract field: {exc}") from exc

    if support.get("policy") != "long_term_support":
        raise ContractError("support.policy must be long_term_support")
    if minimum != "3.7":
        raise ContractError("schema_version 2 requires Python 3.7 as the LTS baseline")
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
    if deprecation.get("automatic_calendar_expiry") is not False:
        raise ContractError("Python 3.7 LTS must not have an automatic calendar expiry")
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
    if native.get("wheel_profile") != "native_py37":
        raise ContractError("native_py37 must reference its wheel profile")
    if lite.get("wheel_profile") != "lite_py37":
        raise ContractError("lite_py37 must reference its wheel profile")
    if abi3.get("wheel_profile") != "abi3":
        raise ContractError("abi3 must reference its wheel profile")
    if abi3.get("minimum_python") != "3.8" or abi3.get("feature") != "abi3-py38":
        raise ContractError("the modern wheel profile must remain abi3-py38")

    expected_distributions = {
        "dcc-mcp-core": "pyproject.toml",
        "dcc-mcp-server": "pkg/dcc-mcp-server-bin/pyproject.toml",
        "dcc-mcp-core-semantic": "pkg/dcc-mcp-core-semantic/pyproject.toml",
    }
    if not isinstance(distributions, dict) or set(distributions) != set(expected_distributions):
        raise ContractError("all released Python distributions must be declared")
    for name, pyproject in expected_distributions.items():
        row = distributions.get(name, {})
        if row.get("pyproject") != pyproject or row.get("support_profile") != "lts":
            raise ContractError(f"distribution {name} must project the Python LTS profile")

    semantic = optional_dependencies.get("semantic", {})
    if semantic.get("owner_distribution") != "dcc-mcp-core":
        raise ContractError("semantic extra must belong to dcc-mcp-core")
    if semantic.get("native_dependency") != "dcc-mcp-core-semantic":
        raise ContractError("semantic extra must include dcc-mcp-core-semantic")
    if semantic.get("python_fallback_dependency") != "fastembed":
        raise ContractError("semantic extra must identify fastembed as its Python fallback")
    if semantic.get("fallback_minimum_python") != "3.8":
        raise ContractError("fastembed fallback must be inactive on Python 3.7")

    _validate_wheel_profiles(wheel_profiles)

    for key in ("requires_python_globs", "calendar_policy_globs"):
        values = projections.get(key)
        if not isinstance(values, list) or not values or not all(isinstance(value, str) for value in values):
            raise ContractError(f"projections.{key} must be a non-empty list of paths")
    exemptions = projections.get("requires_python_exemptions")
    if not isinstance(exemptions, list) or not all(isinstance(value, str) for value in exemptions):
        raise ContractError("projections.requires_python_exemptions must be a list of paths")

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


def _validate_wheel_profiles(wheel_profiles: Any) -> None:
    """Validate artifact-specific Python, ABI, and platform compatibility."""
    if not isinstance(wheel_profiles, dict):
        raise ContractError("wheel_profiles must be an object")
    required = {"native_py37", "lite_py37", "abi3", "semantic_native_py37", "semantic_abi3"}
    if set(wheel_profiles) != required:
        raise ContractError("wheel_profiles must cover every core and semantic release artifact")

    expected_tags = {
        "native_py37": "cp37-cp37m",
        "lite_py37": "py3-none",
        "abi3": "cp38-abi3",
        "semantic_native_py37": "cp37-cp37m",
        "semantic_abi3": "cp38-abi3",
    }
    for name, expected_tag in expected_tags.items():
        profile = wheel_profiles.get(name, {})
        if profile.get("wheel_tag") != expected_tag:
            raise ContractError(f"wheel profile {name} must use {expected_tag}")
        if not isinstance(profile.get("root_is_purelib"), bool):
            raise ContractError(f"wheel profile {name} must declare root_is_purelib")
        if not isinstance(profile.get("expects_extension"), bool):
            raise ContractError(f"wheel profile {name} must declare expects_extension")
        if not isinstance(profile.get("extension_module"), str):
            raise ContractError(f"wheel profile {name} must declare the extension module to inspect")
        platforms = profile.get("platforms")
        if not isinstance(platforms, dict) or not platforms:
            raise ContractError(f"wheel profile {name} must declare platforms")
        for platform, policy in platforms.items():
            if not isinstance(platform, str) or not isinstance(policy, dict):
                raise ContractError(f"wheel profile {name} has an invalid platform policy")
            allowed = policy.get("allowed_platform_tags", [])
            patterns = policy.get("allowed_platform_tag_patterns", [])
            if not allowed and not patterns:
                raise ContractError(f"wheel profile {name} platform {platform} has no allowed tags")
            if not all(isinstance(value, str) for value in [*allowed, *patterns]):
                raise ContractError(f"wheel profile {name} platform {platform} tags must be strings")

    native = wheel_profiles["native_py37"]
    if native.get("distribution") != "dcc-mcp-core" or native.get("extension_module") != "dcc_mcp_core/_core":
        raise ContractError("native_py37 must validate the dcc-mcp-core extension")
    expected_native_platforms = {
        "linux-x86_64": {"manylinux_2_17_x86_64", "manylinux2014_x86_64"},
        "windows-x86_64": {"win_amd64"},
    }
    if set(native["platforms"]) != set(expected_native_platforms):
        raise ContractError("native_py37 must cover Linux and Windows x86_64 wheel tags")
    for platform, expected in expected_native_platforms.items():
        actual = set(native["platforms"][platform].get("allowed_platform_tags", []))
        if actual != expected:
            raise ContractError(f"native_py37 {platform} must retain the legacy DCC platform baseline")

    lite = wheel_profiles["lite_py37"]
    if lite.get("distribution") != "dcc-mcp-core" or lite.get("root_is_purelib") is not True:
        raise ContractError("lite_py37 must remain a pure dcc-mcp-core wheel")
    if lite.get("extension_module") != "dcc_mcp_core/_core" or lite.get("expects_extension") is not False:
        raise ContractError("lite_py37 must reject the dcc-mcp-core extension")
    if set(lite.get("platforms", {})) != {"any"}:
        raise ContractError("lite_py37 must remain platform independent")

    abi3 = wheel_profiles["abi3"]
    if (
        abi3.get("distribution") != "dcc-mcp-core"
        or abi3.get("extension_module") != "dcc_mcp_core/_core"
        or abi3.get("root_is_purelib") is not False
        or abi3.get("expects_extension") is not True
    ):
        raise ContractError("abi3 must validate the native dcc-mcp-core extension")
    expected_abi3_platforms = {
        "linux-x86_64",
        "windows-x86_64",
        "macos-universal2",
        "macos-native",
    }
    if set(abi3.get("platforms", {})) != expected_abi3_platforms:
        raise ContractError("abi3 must cover Linux, Windows, and both macOS wheel shapes")
    linux_abi3 = abi3["platforms"]["linux-x86_64"]
    if set(linux_abi3.get("allowed_platform_tags", [])) != {
        "manylinux_2_17_x86_64",
        "manylinux2014_x86_64",
    } or linux_abi3.get("allowed_platform_tag_patterns"):
        raise ContractError("abi3 Linux must retain the manylinux2014 / glibc 2.17 baseline")
    universal_patterns = set(abi3["platforms"]["macos-universal2"].get("allowed_platform_tag_patterns", []))
    if universal_patterns != {
        "macosx_*_universal2",
        "macosx_*_x86_64",
        "macosx_*_arm64",
    }:
        raise ContractError("abi3 macOS universal2 must accept all compressed compatibility tags")
    required_universal = set(abi3["platforms"]["macos-universal2"].get("required_platform_tag_patterns", []))
    if required_universal != {"macosx_*_universal2"}:
        raise ContractError("abi3 macOS universal2 must require a universal2 compatibility tag")
    native_patterns = set(abi3["platforms"]["macos-native"].get("allowed_platform_tag_patterns", []))
    if native_patterns != {"macosx_*_x86_64", "macosx_*_arm64"}:
        raise ContractError("abi3 macOS native must accept hosted runner architectures")

    semantic = wheel_profiles["semantic_native_py37"]
    if (
        semantic.get("distribution") != "dcc-mcp-core-semantic"
        or semantic.get("extension_module") != "dcc_mcp_core_semantic/_native"
        or semantic.get("root_is_purelib") is not False
        or semantic.get("expects_extension") is not True
    ):
        raise ContractError("semantic_native_py37 must validate its native extension")
    expected_semantic_platforms = {
        "linux-x86_64": {"manylinux_2_28_x86_64"},
        "windows-x86_64": {"win_amd64"},
    }
    if set(semantic.get("platforms", {})) != set(expected_semantic_platforms):
        raise ContractError("semantic_native_py37 must declare its supported Python 3.7 platforms")
    for platform, expected in expected_semantic_platforms.items():
        actual = set(semantic["platforms"][platform].get("allowed_platform_tags", []))
        if actual != expected:
            raise ContractError(f"semantic_native_py37 {platform} platform baseline is invalid")

    semantic_abi3 = wheel_profiles["semantic_abi3"]
    if (
        semantic_abi3.get("distribution") != "dcc-mcp-core-semantic"
        or semantic_abi3.get("extension_module") != "dcc_mcp_core_semantic/_native"
        or semantic_abi3.get("root_is_purelib") is not False
        or semantic_abi3.get("expects_extension") is not True
    ):
        raise ContractError("semantic_abi3 must validate the semantic native extension")
    expected_semantic_abi3_platforms = {
        "linux-x86_64": {"manylinux_2_28_x86_64"},
        "windows-x86_64": {"win_amd64"},
    }
    if set(semantic_abi3.get("platforms", {})) != {
        *expected_semantic_abi3_platforms,
        "macos-native",
    }:
        raise ContractError("semantic_abi3 must cover Linux, Windows, and macOS arm64")
    for platform, expected in expected_semantic_abi3_platforms.items():
        actual = set(semantic_abi3["platforms"][platform].get("allowed_platform_tags", []))
        if actual != expected:
            raise ContractError(f"semantic_abi3 {platform} platform baseline is invalid")
    mac_patterns = set(semantic_abi3["platforms"]["macos-native"].get("allowed_platform_tag_patterns", []))
    if mac_patterns != {"macosx_*_arm64"}:
        raise ContractError("semantic_abi3 macOS must remain arm64-only")


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
