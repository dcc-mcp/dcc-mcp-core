#!/usr/bin/env python
"""Fail when declared dependencies drift from the VFX Platform baseline.

The baseline lives in ``config/vfx-platform-baseline.json``. Each repository
declares which VFX Platform year(s) it aligns to and which host it targets in
``pyproject.toml``::

    [tool.dcc-mcp.vfx-platform]
    year = ["CY2026", "py37"]
    host = "core"

A dependency is checked against every declared tier. A requirement whose
environment marker excludes a tier's interpreter is skipped for that tier,
which is the supported way to carry a CY2026-only dependency in a repository
that must still import on Python 3.7.

Exit status is non-zero when any tier cannot be satisfied.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
from typing import Any

import tomllib

try:
    from packaging.markers import Marker
    from packaging.requirements import Requirement
    from packaging.specifiers import SpecifierSet
    from packaging.version import InvalidVersion
    from packaging.version import Version
except ImportError as exc:  # pragma: no cover - environment guard
    raise SystemExit(
        f'packaging is required to run this check ({exc}); install it with: pip install "packaging>=23.0"'
    ) from exc


BASELINE_FILENAME = "vfx-platform-baseline.json"


class BaselineError(RuntimeError):
    """Raised when the baseline file or a declaration is unusable."""


def repository_root() -> Path:
    """Return the repository root inferred from this file's location."""
    return Path(__file__).resolve().parent.parent.parent


def load_baseline(path: Path) -> dict[str, Any]:
    """Load and structurally validate the baseline document."""
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise BaselineError(f"baseline file not found: {path}") from exc
    except json.JSONDecodeError as exc:
        raise BaselineError(f"baseline file is not valid JSON: {path}: {exc}") from exc

    if not isinstance(data, dict) or "tiers" not in data:
        raise BaselineError(f"baseline file has no 'tiers' object: {path}")
    if not isinstance(data["tiers"], dict) or not data["tiers"]:
        raise BaselineError(f"baseline 'tiers' must be a non-empty object: {path}")
    return data


def load_pyproject(path: Path) -> dict[str, Any]:
    """Load a pyproject.toml document."""
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except FileNotFoundError as exc:
        raise BaselineError(f"pyproject file not found: {path}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise BaselineError(f"pyproject file is not valid TOML: {path}: {exc}") from exc


def declaration(pyproject: dict[str, Any]) -> dict[str, Any]:
    """Return the ``[tool.dcc-mcp.vfx-platform]`` declaration."""
    tool = pyproject.get("tool")
    if not isinstance(tool, dict):
        return {}
    section = tool.get("dcc-mcp")
    if not isinstance(section, dict):
        return {}
    value = section.get("vfx-platform")
    return value if isinstance(value, dict) else {}


def normalize(name: str) -> str:
    """Normalize a distribution name for baseline lookups."""
    return name.strip().lower().replace("_", "-")


def tier_python_version(tier: dict[str, Any]) -> str:
    """Return the concrete interpreter version used to evaluate markers."""
    return str(tier.get("python_version") or tier.get("python") or "3.13.0")


def marker_applies(marker: Marker | None, python_version: str) -> bool:
    """Return True when a requirement's marker can hold on this interpreter."""
    if marker is None:
        return True
    environment = {
        "python_version": ".".join(python_version.split(".")[:2]),
        "python_full_version": python_version,
        "sys_platform": "linux",
        "platform_system": "Linux",
        "platform_machine": "x86_64",
        "os_name": "posix",
        "implementation_name": "cpython",
        "implementation_version": python_version,
        "extra": "",
    }
    try:
        return bool(marker.evaluate(environment))
    except Exception:  # pragma: no cover - defensive; unknown marker vars
        return True


def _parse_bound(value: Any) -> Version | None:
    if value in (None, ""):
        return None
    try:
        return Version(str(value))
    except InvalidVersion:
        return None


def check_requirement(req: Requirement, component: dict[str, Any], tier_name: str) -> list[str]:
    """Return error strings when a requirement cannot satisfy a tier's range."""
    floor = _parse_bound(component.get("floor"))
    ceiling = _parse_bound(component.get("ceiling"))
    if floor is None and ceiling is None:
        return []

    specifier = req.specifier
    errors: list[str] = []

    # A floor-bounded tier needs at least one admissible version at or above it.
    if floor is not None:
        if not specifier.contains(floor, prereleases=True):
            upper = f",<={ceiling}" if ceiling is not None else ""
            errors.append(
                f"'{req}' does not admit the {tier_name} floor {floor}; "
                f"expected a range that includes {component.get('specifier') or f'>={floor}{upper}'}"
            )
        elif ceiling is not None and not specifier.contains(ceiling, prereleases=True):
            errors.append(
                f"'{req}' excludes the {tier_name} target {ceiling}; "
                f"expected a range that includes {component.get('specifier') or f'>={floor},<={ceiling}'}"
            )
    elif ceiling is not None and not specifier.contains(ceiling, prereleases=True):
        # py37-style tier: only an upper bound is meaningful. The declared range
        # must still admit a version that installs on that interpreter.
        errors.append(
            f"'{req}' cannot resolve to a {tier_name}-installable version; "
            f"the highest release supporting that interpreter is {ceiling} "
            f"(expected a range admitting {component.get('specifier') or f'<={ceiling}'})"
        )

    return errors


def open_upper_bound(req: Requirement, component: dict[str, Any], tier_name: str) -> bool:
    """Return True when the range admits versions past the tier's ceiling."""
    ceiling = _parse_bound(component.get("ceiling"))
    if ceiling is None or not component.get("floor"):
        return False
    # Only floor-bearing tiers have a meaningful upper bound to leak past;
    # for a py37-style tier the ceiling alone already fails the error check.
    probe = Version(f"{ceiling.major + 1}.0.0")
    return req.specifier.contains(probe, prereleases=True)


def component_index(tier: dict[str, Any]) -> dict[str, tuple[str, dict[str, Any]]]:
    """Map normalized distribution name -> (component name, component body)."""
    index: dict[str, tuple[str, dict[str, Any]]] = {}
    for name, body in (tier.get("components") or {}).items():
        if not isinstance(body, dict):
            continue
        pypi = body.get("pypi")
        if not pypi:
            continue
        index[normalize(str(pypi))] = (name, body)
    return index


def tier_for_python(tier: dict[str, Any]) -> SpecifierSet | None:
    """Return the interpreter range a tier demands, when it declares one."""
    value = tier.get("requires_python_floor")
    return SpecifierSet(str(value)) if value else None


def collect_requirements(pyproject: dict[str, Any]) -> list[Requirement]:
    """Parse the runtime dependencies declared by a project."""
    project = pyproject.get("project")
    if not isinstance(project, dict):
        return []
    raw = project.get("dependencies")
    if not isinstance(raw, list):
        return []
    requirements: list[Requirement] = []
    for item in raw:
        if not isinstance(item, str):
            continue
        try:
            requirements.append(Requirement(item))
        except Exception as exc:
            raise BaselineError(f"cannot parse dependency {item!r}: {exc}") from exc
    return requirements


def evaluate(pyproject: dict[str, Any], baseline: dict[str, Any]) -> tuple[list[str], list[str]]:
    """Return (errors, warnings) for a repository against the baseline."""
    errors: list[str] = []
    warnings: list[str] = []

    decl = declaration(pyproject)
    if not decl:
        return ["missing declaration: add [tool.dcc-mcp.vfx-platform] with 'year' and 'host' to pyproject.toml"], []

    raw_years = decl.get("year", decl.get("years"))
    if isinstance(raw_years, str):
        years = [raw_years]
    elif isinstance(raw_years, list) and raw_years:
        years = [str(year) for year in raw_years]
    else:
        return ["[tool.dcc-mcp.vfx-platform].year must name at least one baseline tier"], []

    tiers = baseline["tiers"]
    unknown = [year for year in years if year not in tiers]
    if unknown:
        known = ", ".join(sorted(tiers))
        return [f"unknown baseline tier(s): {', '.join(unknown)} (known: {known})"], []

    host = decl.get("host")
    if not host:
        errors.append("[tool.dcc-mcp.vfx-platform].host is required")

    # -- Python floor policy ------------------------------------------------
    floor_policy = baseline.get("python37_floor") or {}
    constrained = {str(h).lower() for h in floor_policy.get("constrained_hosts", [])}
    exempt = {str(h).lower() for h in floor_policy.get("exempt_hosts", [])}
    requires_python = (pyproject.get("project") or {}).get("requires-python")
    host_key = str(host).lower() if host else ""

    if requires_python and host_key in constrained:
        floor = SpecifierSet(str(requires_python))
        if Version("3.7.17") not in floor:
            errors.append(
                f"host '{host}' is subject to the Python 3.7 floor until "
                f"{floor_policy.get('expires', 'the policy expiry')}; "
                f"requires-python '{requires_python}' excludes 3.7"
            )

    # -- Dependency alignment ----------------------------------------------
    requirements = collect_requirements(pyproject)
    strict_upper = bool(decl.get("strict_upper_bound"))

    for year in years:
        tier = tiers[year]
        python_version = tier_python_version(tier)
        index = component_index(tier)

        if requires_python and not tier_for_python(tier):
            pass  # Tier declares no interpreter requirement; nothing to assert.

        for req in requirements:
            if not marker_applies(req.marker, python_version):
                continue
            match = index.get(normalize(req.name))
            if match is None:
                continue
            component_name, component = match
            found = check_requirement(req, component, year)
            errors.extend(f"{year}/{component_name}: {message}" for message in found)
            if open_upper_bound(req, component, year):
                message = (
                    f"{year}/{component_name}: '{req}' is open above the {year} ceiling "
                    f"{component.get('ceiling')}; it may resolve to a later year's release"
                )
                if strict_upper:
                    errors.append(message)
                else:
                    warnings.append(message)

    if host_key and host_key not in constrained and host_key not in exempt:
        warnings.append(
            f"host '{host}' is not in the baseline host lists; add it to "
            f"'python37_floor.constrained_hosts' or 'python37_floor.exempt_hosts'"
        )

    return errors, warnings


def main(argv: list[str] | None = None) -> int:
    """Run the baseline check and return a shell exit status."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, default=None, help="path to the baseline JSON")
    parser.add_argument("--pyproject", type=Path, default=None, help="path to pyproject.toml")
    parser.add_argument("--json", action="store_true", help="emit findings as JSON instead of text")
    args = parser.parse_args(argv)

    root = repository_root()
    baseline_path = args.baseline or root / "config" / BASELINE_FILENAME
    pyproject_path = args.pyproject or root / "pyproject.toml"

    try:
        baseline = load_baseline(baseline_path)
        pyproject = load_pyproject(pyproject_path)
    except BaselineError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    try:
        errors, warnings = evaluate(pyproject, baseline)
    except BaselineError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps({"errors": errors, "warnings": warnings}, indent=2))
        return 1 if errors else 0

    for message in warnings:
        print(f"warning: {message}")
    for message in errors:
        print(f"error: {message}")

    if errors:
        print(f"\nFAIL: {len(errors)} VFX Platform baseline violation(s).")
        return 1

    print("OK: dependencies match the declared VFX Platform baseline.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
