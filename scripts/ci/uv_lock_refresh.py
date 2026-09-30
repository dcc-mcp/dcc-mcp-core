#!/usr/bin/env python3
"""Guard the scheduled refresh of the two published ``pkg/`` pins in ``uv.lock``.

``pkg/dcc-mcp-core-semantic`` and ``pkg/dcc-mcp-server-bin`` publish PyPI wheels
that the root project consumes as ordinary registry dependencies, so ``uv.lock``
only moves when something asks uv to re-resolve those two names explicitly.
Nothing did for twelve releases, which is why the pins sat on ``0.20.12`` while
the repository shipped ``0.20.23``.

``.github/workflows/uv-lock-refresh.yml`` runs
``uv lock --upgrade-package dcc-mcp-core-semantic --upgrade-package dcc-mcp-server``
on a schedule. This module is the gate that runs immediately afterwards and
decides whether the resulting lockfile is allowed to become a pull request.

The gate is deliberately narrow. A refresh may only:

* move the version of the two published packages named in
  :data:`ALLOWED_REFRESH_PACKAGES`;
* keep every ``cp37`` wheel it had before (Python 3.7 support is a red line
  until 2026-12-31);
* leave ``requires-python`` untouched, so resolution cannot silently narrow the
  supported interpreter range.

Anything else fails the run instead of opening a pull request: a surprise
re-resolution of the rest of the dependency graph is reported for a human to
look at, never merged.

The gate also says *what* the refresh did. ``uv lock`` rewrites the whole
lockfile, so a run that moves no package version can still re-emit every
dependency marker: the pins then already sat on the newest release and the only
real change is marker text. :func:`classify_change` separates those two
outcomes -- :data:`CHANGE_MARKER_ONLY` versus :data:`CHANGE_VERSIONS` -- so the
workflow can describe the pull request honestly instead of announcing a version
refresh that never happened. It reads the global ``resolution-markers`` list
too, whose order encodes the resolver's Python-version layering, so a
re-layered resolution is reported even when no version moved.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised by the Python 3.7 CI lane
    import tomli as tomllib

ALLOWED_REFRESH_PACKAGES = ("dcc-mcp-core-semantic", "dcc-mcp-server")
CP37_WHEEL_MARKER = "cp37"

#: A refresh that left versions, markers and resolution markers untouched.
CHANGE_UNCHANGED = "unchanged"
#: A refresh that only re-emitted marker text; no package version moved.
CHANGE_MARKER_ONLY = "marker-only"
#: A refresh in which at least one package version moved.
CHANGE_VERSIONS = "versions"


def version_key(value: str) -> tuple:
    """Return a comparable numeric key for a PEP 440 / semver-ish version."""
    key = []
    for part in re.split(r"[.\-+]", value.strip()):
        match = re.match(r"^(\d+)", part)
        if match is None:
            break
        key.append(int(match.group(1)))
    return tuple(key) or (0,)


def _newest(counts: dict) -> str | None:
    """Return the newest version recorded in a {version: count} mapping."""
    if not counts:
        return None
    return max(counts, key=version_key)


def _load_toml(path: Path) -> dict:
    return tomllib.loads(Path(path).read_text(encoding="utf-8"))


def package_versions(lock: dict) -> dict:
    """Map package name -> {version: entry count} for a parsed lockfile."""
    versions: dict = {}
    packages = lock.get("package")
    if not isinstance(packages, list):
        return versions
    for package in packages:
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        version = package.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            continue
        counted = versions.setdefault(name, {})
        counted[version] = counted.get(version, 0) + 1
    return versions


def wheel_urls(lock: dict) -> dict:
    """Map package name -> wheel URLs recorded for that name in a lockfile."""
    urls: dict = {}
    packages = lock.get("package")
    if not isinstance(packages, list):
        return urls
    for package in packages:
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        wheels = package.get("wheels")
        if not isinstance(name, str) or not isinstance(wheels, list):
            continue
        for wheel in wheels:
            if isinstance(wheel, dict) and isinstance(wheel.get("url"), str):
                urls.setdefault(name, []).append(wheel["url"])
    return urls


def _package_entries(lock: dict) -> list:
    """Return the ``[[package]]`` blocks of a parsed lockfile."""
    packages = lock.get("package")
    if not isinstance(packages, list):
        return []
    return [package for package in packages if isinstance(package, dict)]


def _entry_markers(entry: dict) -> tuple:
    """Return every marker string a ``[[package]]`` block records, sorted.

    Three places carry markers: the resolved ``dependencies`` list, each
    ``[package.optional-dependencies]`` group, and the ``requires-dist`` list
    under ``[package.metadata]`` that mirrors the declaring project's extras.
    """
    collected = []

    dependencies = entry.get("dependencies")
    if isinstance(dependencies, list):
        for dependency in dependencies:
            if isinstance(dependency, dict) and isinstance(dependency.get("marker"), str):
                collected.append(dependency["marker"])

    optional = entry.get("optional-dependencies")
    if isinstance(optional, dict):
        for group in sorted(optional):
            members = optional[group]
            if not isinstance(members, list):
                continue
            for dependency in members:
                if isinstance(dependency, dict) and isinstance(dependency.get("marker"), str):
                    collected.append(f"{group}:{dependency['marker']}")

    metadata = entry.get("metadata")
    if isinstance(metadata, dict):
        requires_dist = metadata.get("requires-dist")
        if isinstance(requires_dist, list):
            for requirement in requires_dist:
                if isinstance(requirement, dict) and isinstance(requirement.get("marker"), str):
                    collected.append(f"requires-dist:{requirement['marker']}")

    return tuple(sorted(collected))


def marker_fingerprint(lock: dict) -> dict:
    """Map ``(name, version)`` -> per-block marker tuples for a lockfile.

    The value is a sorted tuple of one tuple per ``[[package]]`` block, so the
    fingerprint is stable under block reordering while still telling apart two
    blocks that share a name and version but carry different markers.
    """
    fingerprint: dict = {}
    for entry in _package_entries(lock):
        name = entry.get("name")
        version = entry.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            continue
        key = (name, version)
        blocks = fingerprint.setdefault(key, [])
        blocks.append(_entry_markers(entry))
    return {key: tuple(sorted(blocks)) for key, blocks in fingerprint.items()}


def resolution_markers(lock: dict) -> list:
    """Return the global ``resolution-markers`` list, order included.

    The order is the resolver's Python-version layering, so reordering the list
    is a real change even when the same markers are all still present.
    """
    markers = lock.get("resolution-markers")
    if not isinstance(markers, list):
        return []
    return [marker for marker in markers if isinstance(marker, str)]


def classify_change(before: dict, after: dict, allowed: tuple = ALLOWED_REFRESH_PACKAGES) -> dict:
    """Describe what a lock refresh actually changed.

    The returned mapping carries:

    ``kind``
        :data:`CHANGE_VERSIONS` when any package version moved,
        :data:`CHANGE_MARKER_ONLY` when only marker text or the resolution
        layering moved, :data:`CHANGE_UNCHANGED` when nothing did.
    ``version_changes``
        Package names whose resolved version multiset changed, sorted.
    ``marker_changes``
        ``name==version`` keys whose dependency markers changed, sorted.
    ``resolution_markers_changed``
        True when the global ``resolution-markers`` list differs at all.
    ``requires_python_changed``
        Reported for completeness; :func:`verify_refresh` already fails on it.
    """
    # `allowed` only scopes the error check in `verify_refresh`; the
    # classification is about what moved, not about what was permitted to move.
    before_versions = package_versions(before)
    after_versions = package_versions(after)
    version_changes = sorted(
        name
        for name in set(before_versions) | set(after_versions)
        if before_versions.get(name) != after_versions.get(name)
    )

    before_markers = marker_fingerprint(before)
    after_markers = marker_fingerprint(after)
    marker_changes = sorted(
        f"{name}=={version}"
        for name, version in set(before_markers) | set(after_markers)
        if before_markers.get((name, version)) != after_markers.get((name, version))
    )

    resolution_markers_changed = resolution_markers(before) != resolution_markers(after)
    if version_changes:
        kind = CHANGE_VERSIONS
    elif marker_changes or resolution_markers_changed:
        kind = CHANGE_MARKER_ONLY
    else:
        kind = CHANGE_UNCHANGED

    return {
        "kind": kind,
        "version_changes": version_changes,
        "marker_changes": marker_changes,
        "resolution_markers_changed": resolution_markers_changed,
        "requires_python_changed": before.get("requires-python") != after.get("requires-python"),
        "allowed_packages": list(allowed),
    }


def describe_change(report: dict) -> str:
    """Return a one-line description of a :func:`classify_change` report."""
    kind = report["kind"]
    if kind == CHANGE_UNCHANGED:
        return "uv.lock unchanged: no package version and no marker moved"
    if kind == CHANGE_VERSIONS:
        return f"version refresh: {', '.join(report['version_changes'])} moved to a different version"
    details = []
    if report["marker_changes"]:
        count = len(report["marker_changes"])
        details.append(f"{count} package block(s) re-emitted with new markers")
    if report["resolution_markers_changed"]:
        details.append("the global resolution-marker list changed (order, layering, or content)")
    detail = "; ".join(details) or "lockfile metadata changed"
    return f"marker-only relock: {detail}; no package version changed"


def verify_refresh(before: dict, after: dict, allowed: tuple = ALLOWED_REFRESH_PACKAGES) -> list:
    """Return errors when a lock refresh escaped its allowed blast radius."""
    allowed_names = set(allowed)
    errors = []

    before_python = before.get("requires-python")
    after_python = after.get("requires-python")
    if before_python != after_python:
        errors.append(
            f"uv.lock requires-python changed from {before_python!r} to {after_python!r}; "
            "the refresh must not narrow the supported Python range"
        )

    before_versions = package_versions(before)
    after_versions = package_versions(after)
    drifted = sorted(
        name
        for name in set(before_versions) | set(after_versions)
        if before_versions.get(name) != after_versions.get(name)
    )
    unexpected = [name for name in drifted if name not in allowed_names]
    if unexpected:
        errors.append(
            "uv.lock refresh changed dependencies outside the allowed refresh set "
            f"{sorted(allowed_names)!r}: {unexpected}"
        )

    before_wheels = wheel_urls(before)
    after_wheels = wheel_urls(after)
    for name in sorted(before_wheels):
        had_cp37 = any(CP37_WHEEL_MARKER in url for url in before_wheels[name])
        keeps_cp37 = any(CP37_WHEEL_MARKER in url for url in after_wheels.get(name, []))
        if had_cp37 and not keeps_cp37:
            errors.append(
                f"uv.lock refresh dropped the {CP37_WHEEL_MARKER} wheels of {name!r}; "
                "Python 3.7 wheels must stay resolved until 2026-12-31"
            )

    for name in sorted(allowed_names):
        if name in before_versions and not after_versions.get(name):
            errors.append(f"uv.lock refresh removed the {name!r} pin instead of moving it forward")
            continue
        before_newest = _newest(before_versions.get(name, {}))
        after_newest = _newest(after_versions.get(name, {}))
        if before_newest and after_newest and version_key(after_newest) < version_key(before_newest):
            errors.append(
                f"uv.lock refresh moved {name!r} backwards from {before_newest!r} to {after_newest!r}; "
                "the pins must track the newest published version, never an older one"
            )
    return errors


def _emit_github_outputs(report: dict) -> None:
    """Append the classification to the GitHub Actions step output file."""
    destination = os.environ.get("GITHUB_OUTPUT")
    if not destination:
        return
    lines = [
        f"change-kind={report['kind']}",
        f"marker-only={'true' if report['kind'] == CHANGE_MARKER_ONLY else 'false'}",
        f"version-changes={','.join(report['version_changes'])}",
        f"marker-changes={','.join(report['marker_changes'])}",
        f"resolution-markers-changed={'true' if report['resolution_markers_changed'] else 'false'}",
        f"summary={describe_change(report)}",
    ]
    with Path(destination).open("a", encoding="utf-8") as handle:
        for line in lines:
            handle.write(f"{line}\n")


def main(argv: list | None = None) -> int:
    """Run the ``verify`` or ``classify`` subcommand against two lockfiles."""
    parser = argparse.ArgumentParser(description="Guard a scheduled uv.lock refresh.")
    subcommands = parser.add_subparsers(dest="command", required=True)

    verify = subcommands.add_parser("verify", help="Compare a lockfile before and after a refresh")
    verify.add_argument("before", help="Path to the lockfile captured before the refresh")
    verify.add_argument("after", help="Path to the lockfile produced by the refresh")
    verify.add_argument(
        "--package",
        action="append",
        dest="packages",
        help="Override an allowed refresh package; repeat for each name",
    )

    classify = subcommands.add_parser("classify", help="Report what a refresh changed, without failing")
    classify.add_argument("before", help="Path to the lockfile captured before the refresh")
    classify.add_argument("after", help="Path to the lockfile produced by the refresh")
    classify.add_argument(
        "--json",
        action="store_true",
        help="Print the full classification as JSON instead of one summary line",
    )

    args = parser.parse_args(list(sys.argv[1:] if argv is None else argv))

    before = _load_toml(args.before)
    after = _load_toml(args.after)

    if args.command == "classify":
        report = classify_change(before, after)
        if args.json:
            print(json.dumps(report, indent=2, sort_keys=True))
        else:
            print(describe_change(report))
        _emit_github_outputs(report)
        return 0

    allowed = tuple(args.packages) if args.packages else ALLOWED_REFRESH_PACKAGES
    errors = verify_refresh(before, after, allowed)
    if errors:
        for error in errors:
            print(f"::error::{error}", file=sys.stderr)
        print(
            "::error::uv lock refresh escaped its allowed blast radius; no pull request will be opened.",
            file=sys.stderr,
        )
        return 1
    print("uv lock refresh stayed inside its allowed blast radius.")
    print(describe_change(classify_change(before, after, allowed)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
