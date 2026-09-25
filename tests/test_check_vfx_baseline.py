"""Tests for the VFX Platform dependency baseline gate.

Every expectation is derived from config/vfx-platform-baseline.json at run
time. Tests must never hardcode an exact component version: the baseline moves
when the VFX Reference Platform publishes a new year, and a pinned literal
would then fail for no reason.
"""

from __future__ import annotations

import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts" / "ci"))

from check_vfx_baseline import BASELINE_FILENAME
from check_vfx_baseline import component_index
from check_vfx_baseline import evaluate
from check_vfx_baseline import load_baseline
from check_vfx_baseline import load_pyproject
from check_vfx_baseline import repository_root

REPO_ROOT = repository_root()
BASELINE_PATH = REPO_ROOT / "config" / BASELINE_FILENAME


@pytest.fixture(scope="module")
def baseline():
    return load_baseline(BASELINE_PATH)


def declare(year, host="core", requires_python=">=3.7", dependencies=()):
    """Build a minimal pyproject document for one scenario."""
    return {
        "project": {
            "name": "probe",
            "requires-python": requires_python,
            "dependencies": list(dependencies),
        },
        "tool": {"dcc-mcp": {"vfx-platform": {"year": year, "host": host}}},
    }


def first_pypi_component(baseline, tier, with_floor):
    """Return (component_name, component) for a component carrying a pypi name."""
    for name, body in baseline["tiers"][tier]["components"].items():
        if not body.get("pypi"):
            continue
        if with_floor and not body.get("floor"):
            continue
        if not with_floor and body.get("floor"):
            continue
        return name, body
    raise AssertionError(f"no matching component in tier {tier!r}")


# -- baseline document ------------------------------------------------------


def test_baseline_file_is_valid_json():
    assert json.loads(BASELINE_PATH.read_text(encoding="utf-8"))


def test_baseline_declares_both_required_tiers(baseline):
    assert "CY2026" in baseline["tiers"]
    assert "py37" in baseline["tiers"]


def test_usd_and_openimageio_are_recorded_as_absent_not_invented(baseline):
    """The official table has no USD/OpenImageIO row; record that, never guess."""
    na = baseline.get("na_components") or {}
    assert "usd" in na and str(na["usd"]).strip()
    assert "openimageio" in na and str(na["openimageio"]).strip()
    for tier in baseline["tiers"].values():
        for name in tier.get("components", {}):
            assert name not in {"usd", "openimageio"}


def test_every_tracked_component_names_the_distribution_or_is_explained(baseline):
    for tier_name, tier in baseline["tiers"].items():
        for name, body in tier.get("components", {}).items():
            assert body.get("pypi") or body.get("note"), f"{tier_name}/{name}"


# -- declaration handling ---------------------------------------------------


def test_missing_declaration_is_an_error(baseline):
    errors, _ = evaluate({"project": {"dependencies": []}}, baseline)
    assert any("missing declaration" in e for e in errors)


def test_unknown_tier_is_an_error(baseline):
    errors, _ = evaluate(declare("CY1999"), baseline)
    assert any("unknown baseline tier" in e for e in errors)


def test_missing_host_is_an_error(baseline):
    doc = declare("CY2026")
    doc["tool"]["dcc-mcp"]["vfx-platform"].pop("host")
    errors, _ = evaluate(doc, baseline)
    assert any("host is required" in e for e in errors)


def test_core_repository_declares_both_tiers():
    """The real pyproject in this repo must declare its alignment."""
    pyproject = load_pyproject(REPO_ROOT / "pyproject.toml")
    decl = pyproject["tool"]["dcc-mcp"]["vfx-platform"]
    assert set(decl["year"]) == {"CY2026", "py37"}
    assert decl["host"] == "core"


# -- python 3.7 floor policy ------------------------------------------------


def test_raising_the_floor_on_a_constrained_host_fails(baseline):
    constrained = baseline["python37_floor"]["constrained_hosts"][0]
    errors, _ = evaluate(declare("CY2026", host=constrained, requires_python=">=3.9"), baseline)
    assert any("Python 3.7 floor" in e for e in errors)


def test_exempt_hosts_may_raise_the_floor(baseline):
    exempt = baseline["python37_floor"]["exempt_hosts"][0]
    errors, _ = evaluate(declare("CY2026", host=exempt, requires_python=">=3.9"), baseline)
    assert not any("Python 3.7 floor" in e for e in errors)


def test_constrained_host_keeping_the_floor_passes(baseline):
    constrained = baseline["python37_floor"]["constrained_hosts"][0]
    errors, _ = evaluate(declare("CY2026", host=constrained, requires_python=">=3.7"), baseline)
    assert not errors


# -- dependency alignment ---------------------------------------------------


def test_untracked_dependencies_are_ignored(baseline):
    errors, _ = evaluate(
        declare(["CY2026", "py37"], dependencies=["totally-untracked-pkg>=1.0"]),
        baseline,
    )
    assert not errors


def test_range_below_the_tier_floor_fails(baseline):
    name, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    floor = body["floor"]
    major = floor.split(".")[0]
    # A range capped strictly below the tier floor shares no version with it.
    requirement = f"{body['pypi']}>={int(major) - 1},<{floor}"
    errors, _ = evaluate(declare("CY2026", dependencies=[requirement]), baseline)
    assert any(f"CY2026/{name}" in e and "admits no version inside" in e for e in errors)


def test_range_above_the_py37_ceiling_fails(baseline):
    name, body = first_pypi_component(baseline, "py37", with_floor=False)
    ceiling = body["ceiling"]
    major = int(ceiling.split(".")[0])
    requirement = f"{body['pypi']}>={major + 1}.0"
    errors, _ = evaluate(declare("py37", dependencies=[requirement]), baseline)
    assert any(f"py37/{name}" in e and "installable" in e for e in errors)


def test_range_admitting_the_tier_target_passes(baseline):
    _, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    requirement = f"{body['pypi']}{body['specifier']}"
    errors, _ = evaluate(declare("CY2026", dependencies=[requirement]), baseline)
    assert not errors, errors


def test_marker_can_exempt_a_dependency_from_a_tier(baseline):
    """A CY2026-only pin guarded by a marker must not fail the py37 tier."""
    name, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    requirement = f"{body['pypi']}{body['specifier']}; python_version>='3.9'"
    errors, _ = evaluate(declare(["CY2026", "py37"], dependencies=[requirement]), baseline)
    assert not any(f"py37/{name}" in e for e in errors)


def test_exact_pin_inside_the_tier_range_passes(baseline):
    """An exact pin is the most common declaration and must not be rejected.

    The predicate tests range overlap, not containment of the tier endpoints:
    an exact pin lands inside the range without ever containing its floor.
    """
    _, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    pin = f"{body['pypi']}=={body['floor']}"
    errors, _ = evaluate(declare("CY2026", dependencies=[pin]), baseline)
    assert not errors, errors


def test_compatible_release_operator_inside_the_tier_passes(baseline):
    _, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    errors, _ = evaluate(declare("CY2026", dependencies=[f"{body['pypi']}~={body['floor']}"]), baseline)
    assert not errors, errors


def test_exact_pin_below_a_ceiling_only_tier_passes(baseline):
    """py37 tiers are ceiling-only; a pin under the ceiling is valid.

    pyside2==5.15.2 is the standard pin for Maya 2022 hosts, which is exactly
    the audience this tier exists to serve.
    """
    components = baseline["tiers"]["py37"]["components"]
    body = components.get("pyside2")
    if body is None:  # pragma: no cover - guard for baseline reshuffles
        pytest.skip("pyside2 entry not present to exercise the Maya 2022 pin")
    errors, _ = evaluate(declare("py37", dependencies=[f"{body['pypi']}==5.15.2"]), baseline)
    assert not errors, errors


def test_sub_range_inside_the_tier_passes(baseline):
    """A range strictly inside the tier overlaps it without touching an endpoint."""
    _, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    floor = body["floor"]
    parts = floor.split(".")
    narrowed = f"{parts[0]}.{parts[1]}.{int(parts[2]) + 1}"
    requirement = f"{body['pypi']}>={floor},<{narrowed}"
    errors, _ = evaluate(declare("CY2026", dependencies=[requirement]), baseline)
    assert not errors, errors


def test_range_entirely_above_an_exact_pin_tier_fails(baseline):
    _, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    major = body["floor"].split(".")[0]
    requirement = f"{body['pypi']}>=999{major}.0"
    errors, _ = evaluate(declare("CY2026", dependencies=[requirement]), baseline)
    assert errors


def test_open_upper_bound_warns_without_failing(baseline):
    name, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    requirement = f"{body['pypi']}>={body['floor']}"
    errors, warnings = evaluate(declare("CY2026", dependencies=[requirement]), baseline)
    assert not errors
    assert any(f"CY2026/{name}" in w and "open above" in w for w in warnings)


def test_open_upper_bound_can_be_made_strict(baseline):
    name, body = first_pypi_component(baseline, "CY2026", with_floor=True)
    doc = declare("CY2026", dependencies=[f"{body['pypi']}>={body['floor']}"])
    doc["tool"]["dcc-mcp"]["vfx-platform"]["strict_upper_bound"] = True
    errors, _ = evaluate(doc, baseline)
    assert any(f"CY2026/{name}" in e and "open above" in e for e in errors)


def test_component_without_bounds_is_never_silently_checked(baseline):
    """A component with neither floor nor ceiling validates nothing.

    Guards against reintroducing a no-op entry: the py37 pyqt row once shipped
    null bounds, which accepted pyqt6>=6.9 even though no such release has a
    cp37 wheel.
    """
    for tier_name, tier in baseline["tiers"].items():
        for name, body in tier.get("components", {}).items():
            unbounded = body.get("floor") is None and body.get("ceiling") is None
            assert not unbounded, f"{tier_name}/{name} has no bounds and checks nothing"


def test_component_index_maps_only_tracked_distributions(baseline):
    index = component_index(baseline["tiers"]["CY2026"])
    assert index
    assert all(isinstance(key, str) for key in index)
