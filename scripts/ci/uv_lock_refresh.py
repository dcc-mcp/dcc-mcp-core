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
"""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised by the Python 3.7 CI lane
    import tomli as tomllib

ALLOWED_REFRESH_PACKAGES = ("dcc-mcp-core-semantic", "dcc-mcp-server")
CP37_WHEEL_MARKER = "cp37"


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


def main(argv: list | None = None) -> int:
    """Run the ``verify`` subcommand against two lockfiles."""
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
    args = parser.parse_args(list(sys.argv[1:] if argv is None else argv))

    allowed = tuple(args.packages) if args.packages else ALLOWED_REFRESH_PACKAGES
    errors = verify_refresh(_load_toml(args.before), _load_toml(args.after), allowed)
    if errors:
        for error in errors:
            print(f"::error::{error}", file=sys.stderr)
        print(
            "::error::uv lock refresh escaped its allowed blast radius; no pull request will be opened.",
            file=sys.stderr,
        )
        return 1
    print("uv lock refresh stayed inside its allowed blast radius.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
