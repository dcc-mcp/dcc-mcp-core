"""Adapter dependency declarations on `dcc-mcp-core` must carry a usable upper bound.

`dcc-mcp-core` ships `0.MINOR.PATCH` releases, so every minor bump may break an
adapter. Adapter packaging metadata pins a bounded range, while the package
environment that assembles the DCC runtime has historically requested
`dcc_mcp_core-0` — "any 0.x". Resolvers then pick a core the adapter never
claimed to support and the failure only surfaces at import time, several layers
away from the declaration that allowed it.

These tests pin the contract in `compatibility/core-bounds.json` to both
implementations: `dcc_mcp_core.deployment.core_bounds` (Python, used by adapters
and CI) and `dcc-mcp-catalog`'s `core_bounds` module (Rust, used by catalog
validation).
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys

import pytest

from dcc_mcp_core.deployment import core_bounds

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "compatibility" / "core-bounds.json"
CATALOG = ROOT / "dcc-mcp-catalog.yml"


def _contract() -> dict:
    return json.loads(CONTRACT.read_text(encoding="utf-8"))


def _codes(report: dict) -> list:
    return list(report["codes"])


# ── contract file ─────────────────────────────────────────────────────────────


def test_contract_declares_a_zero_dot_x_breaking_component():
    contract = _contract()
    assert contract["distribution"] == core_bounds.CORE_DISTRIBUTION
    assert contract["import_name"] == core_bounds.CORE_IMPORT_NAME
    assert contract["versioning"]["scheme"] == "0.x"
    assert contract["versioning"]["breaking_component"] == "minor"


def test_module_defaults_match_json_contract():
    policy = _contract()["adapter_requirement_policy"]
    assert policy["require_lower_bound"] == core_bounds.REQUIRE_LOWER_BOUND
    assert policy["require_upper_bound"] == core_bounds.REQUIRE_UPPER_BOUND
    assert policy["max_minor_lines"] == core_bounds.MAX_MINOR_LINES
    assert set(_contract()["violation_codes"]) == set(core_bounds.CODE_DESCRIPTIONS)


# ── parsing ───────────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "declaration, expected",
    [
        ("dcc-mcp-core>=0.19.3,<0.19.5", ">=0.19.3,<0.19.5"),
        (">=0.20.14,<0.21.0", ">=0.20.14,<0.21.0"),
        ("dcc_mcp_core[server] (>=0.20.0,<0.21.0); python_version >= '3.8'", ">=0.20.0,<0.21.0"),
        ("~=0.19.3", ">=0.19.3,<0.20.0"),
        ("dcc-mcp-core==0.20.28", ">=0.20.28,<=0.20.28"),
    ],
)
def test_parses_packaging_requirements(declaration, expected):
    assert core_bounds.parse_requirement(declaration).to_spec() == expected


@pytest.mark.parametrize(
    "declaration, expected",
    [
        ("dcc_mcp_core-0.20", ">=0.20.0,<0.21.0"),
        ("dcc_mcp_core-0.19.3", ">=0.19.3,<=0.19.3"),
        ("dcc_mcp_core-0.12.18..1", ">=0.12.18,<1.0.0"),
        # `dcc_mcp_core-0` means "any 0.x": every breaking minor line.
        ("dcc_mcp_core-0", ">=0.0.0,<1.0.0"),
    ],
)
def test_parses_package_environment_requests(declaration, expected):
    assert core_bounds.parse_requirement(declaration).to_spec() == expected


@pytest.mark.parametrize("declaration", ["dcc_mcp_core", "dcc-mcp-3dsmax", "dcc-mcp-core"])
def test_bare_names_are_unbounded(declaration):
    assert core_bounds.parse_requirement(declaration).is_unbounded()


# ── policy ────────────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "declaration",
    [
        ">=0.19.3,<0.19.5",
        "dcc-mcp-core>=0.20.14,<0.21.0",
        "dcc_mcp_core-0.20",
        ">=0.20.0,<=0.20.9",
        "dcc-mcp-core==0.20.28",
        "~=0.20.14",
    ],
)
def test_compliant_declarations_pass(declaration):
    report = core_bounds.evaluate(declaration)
    assert report["ok"], report["message"]


@pytest.mark.parametrize(
    "declaration, expected_codes",
    [
        ("dcc_mcp_core", [core_bounds.CODE_MISSING_LOWER_BOUND, core_bounds.CODE_MISSING_UPPER_BOUND]),
        (">=0.20.14", [core_bounds.CODE_MISSING_UPPER_BOUND]),
        ("<0.21.0", [core_bounds.CODE_MISSING_LOWER_BOUND]),
        ("dcc_mcp_core-0", [core_bounds.CODE_UPPER_BOUND_TOO_WIDE]),
        (">=0.12.18,<1.0.0", [core_bounds.CODE_UPPER_BOUND_TOO_WIDE]),
        (">=0.18.21,<1.0.0", [core_bounds.CODE_UPPER_BOUND_TOO_WIDE]),
        (">=0.20.0,<0.19.0", [core_bounds.CODE_INVERTED_RANGE]),
        ("", [core_bounds.CODE_UNPARSABLE_DECLARATION]),
    ],
)
def test_violations_are_reported(declaration, expected_codes):
    report = core_bounds.evaluate(declaration)
    assert not report["ok"]
    assert _codes(report) == expected_codes


def test_unknown_specifiers_fail_closed():
    report = core_bounds.evaluate("dcc-mcp-core>=0.19.3,===weird")
    assert core_bounds.CODE_UNSUPPORTED_SPECIFIER in _codes(report)
    # The unknown specifier also hides the upper bound.
    assert core_bounds.CODE_MISSING_UPPER_BOUND in _codes(report)


def test_suggestion_narrows_to_one_minor_line():
    assert core_bounds.evaluate(">=0.19.3")["suggestion"] == ">=0.19.3,<0.20.0"
    # `dcc_mcp_core-0` has no usable floor, so no range is invented for it.
    assert core_bounds.evaluate("dcc_mcp_core-0")["suggestion"] is None


@pytest.mark.parametrize(
    "min_core_version, expected",
    [
        ("0.19.3", ">=0.19.3,<0.20.0"),
        ("0.20", ">=0.20.0,<0.21.0"),
        ("0.20.28", ">=0.20.28,<0.21.0"),
    ],
)
def test_derive_requirement_bounds_one_minor_line(min_core_version, expected):
    assert core_bounds.derive_requirement(min_core_version) == expected


def test_derive_requirement_rejects_unreadable_versions():
    assert core_bounds.derive_requirement("not-a-version") is None


# ── the reported incident ─────────────────────────────────────────────────────


def test_reported_cases_match_the_contract():
    cases = _contract()["reported_cases"]
    assert cases, "the contract must keep the incident it was written for"
    for case in cases:
        for field in ("packaging_requirement", "package_environment_request"):
            declaration = case[field]
            report = core_bounds.evaluate(declaration)
            assert _codes(report) == case["expected_codes"][field], (
                case["adapter"],
                case["adapter_version"],
                field,
            )
        # A compliant packaging declaration never admitted the core the
        # environment resolved, which is the whole point of the contract. Wide
        # declarations such as `>=0.19.8,<1.0.0` do admit it — and are reported
        # as `upper_bound_too_wide` above instead.
        if core_bounds.evaluate(case["packaging_requirement"])["ok"]:
            runtime = core_bounds.check_runtime(case["packaging_requirement"], case["observed_core"])
            assert runtime["verdict"] == core_bounds.VERDICT_CORE_NEWER_THAN_DECLARED, case["adapter"]


def test_compare_declarations_detects_environment_drift():
    # dcc-mcp-maya 0.9.4 declares >=0.19.3,<0.19.5 on PyPI while its package
    # environment requests `dcc_mcp_core-0`.
    comparison = core_bounds.compare_declarations(">=0.19.3,<0.19.5", "dcc_mcp_core-0")
    assert comparison["drift"] == core_bounds.ENVIRONMENT_WIDER
    assert comparison["ok"] is False

    aligned = core_bounds.compare_declarations(">=0.20.0,<0.21.0", "dcc_mcp_core-0.20")
    assert aligned["drift"] == core_bounds.ALIGNED


def test_compare_declarations_reports_unreadable_ranges():
    comparison = core_bounds.compare_declarations(">=0.19.3,<0.19.5", "")
    assert comparison["drift"] == core_bounds.DECLARATION_UNUSABLE


def test_runtime_check_flags_the_unsupported_combination():
    report = core_bounds.check_runtime(">=0.19.3,<0.19.5", "0.20.28")
    assert report["verdict"] == core_bounds.VERDICT_CORE_NEWER_THAN_DECLARED
    assert report["ok"] is False
    assert report["running"] == "0.20.28"

    supported = core_bounds.check_runtime(">=0.19.3,<0.19.5", "0.19.4")
    assert supported["verdict"] == core_bounds.VERDICT_SUPPORTED

    older = core_bounds.check_runtime(">=0.19.3,<0.19.5", "0.18.0")
    assert older["verdict"] == core_bounds.VERDICT_CORE_OLDER_THAN_DECLARED

    unusable = core_bounds.check_runtime("dcc_mcp_core-0", "0.20.28")
    assert unusable["verdict"] == core_bounds.VERDICT_DECLARATION_UNUSABLE


# ── catalog consistency ───────────────────────────────────────────────────────


def test_catalog_min_core_versions_derive_compliant_requirements():
    """Every catalog floor must derive a requirement that satisfies the contract."""
    text = CATALOG.read_text(encoding="utf-8")
    floors = re.findall(r"^\s*min_core_version:\s*\"?([0-9][0-9.]*)\"?\s*$", text, flags=re.MULTILINE)
    assert len(floors) >= 10, "the catalog must still publish core floors"
    for floor in floors:
        derived = core_bounds.derive_requirement(floor)
        assert derived is not None, floor
        assert core_bounds.evaluate(derived)["ok"], derived


# ── installed-metadata reading ────────────────────────────────────────────────


def test_installed_core_requirement_reads_dist_info_metadata(tmp_path, monkeypatch):
    dist_info = tmp_path / "dcc-mcp-maya-0.9.4.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text(
        "Metadata-Version: 2.1\n"
        "Name: dcc-mcp-maya\n"
        "Version: 0.9.4\n"
        "Requires-Dist: dcc-mcp-core>=0.19.3,<0.19.5\n"
        "Requires-Dist: pyside6>=6.5\n",
        encoding="utf-8",
    )
    # Isolate the scan: the host may have real adapters installed whose
    # metadata would otherwise satisfy the lookup.
    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(core_bounds, "_read_metadata", core_bounds._scan_dist_info_metadata)
    assert core_bounds.installed_core_requirement("dcc-mcp-maya") == "dcc-mcp-core>=0.19.3,<0.19.5"
    assert core_bounds.installed_core_requirement("dcc-mcp-blender") is None
