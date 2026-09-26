#!/usr/bin/env python3
"""Fail when committed lockfiles drift from the versions declared in the repo.

Two independent invariants are enforced, one per lockfile.

``Cargo.lock``
    Every workspace member crate must be pinned to the version its
    ``Cargo.toml`` resolves to. Crates inherit the workspace version through
    ``version.workspace = true``, so release-please bumps the declaration and
    the lock together; any difference means one of the two was regenerated or
    hand-edited alone.

``uv.lock``
    ``pkg/*/pyproject.toml`` builds PyPI distributions that the root project
    consumes as ordinary registry dependencies (``dcc-mcp-server`` and
    ``dcc-mcp-core-semantic``). Those wheels only exist once a release is
    published, so ``uv.lock`` can never pin the version an open release-please
    PR is declaring: resolution reads the index, not this checkout. The
    enforceable invariant is therefore that the lock must not fall behind the
    newest *already released* version. That floor is read from ``CHANGELOG.md``
    with the in-flight version excluded, so a lock left several releases stale
    fails.

    A release-please PR gets one extra release of tolerance. The pins only
    move when ``uv`` re-resolves them against the index, which
    ``uv-lock-refresh.yml`` does on a daily schedule through a pull request
    that stays open for a human to merge. Release-please opens the next release
    PR hours before that refresh lands, so at PR birth the lock is legitimately
    one published release behind -- the same distance ``main`` always sits at,
    because ``main`` excludes its own declared version from the floor. Without
    the extra step the gate fires on every release PR by construction, not
    because anything drifted.

Both checks are read-only and never rewrite a lockfile, so they cannot narrow
``requires-python`` or drop Python 3.7 markers.
"""

from __future__ import annotations

import os
from pathlib import Path
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised by the Python 3.7 CI lane
    import tomli as tomllib

# Release-please names its PR branches after the target branch and component,
# e.g. ``release-please--branches--main--components--dcc-mcp-core``.
RELEASE_PR_REF_PREFIX = "release-please--"
# ``GITHUB_HEAD_REF`` is the head branch of a pull request run; ``GITHUB_REF``
# is the fallback for branch pushes, where it is ``refs/heads/<branch>``.
RELEASE_PR_REF_ENV_VARS = ("GITHUB_HEAD_REF", "GITHUB_REF")

_RELEASE_HEADING_RE = re.compile(r"^## \[([^\]]+)\]", re.MULTILINE)


def _load_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def _version_key(value: str) -> tuple:
    """Return a comparable numeric key for a PEP 440 / semver-ish version."""
    key = []
    for part in re.split(r"[.\-+]", value.strip()):
        match = re.match(r"^(\d+)", part)
        if match is None:
            break
        key.append(int(match.group(1)))
    return tuple(key) or (0,)


def _newest_released_except(released: list[str], excluded: str, *also_excluded: str) -> str | None:
    """Return the newest released version that is not one of the excluded ones."""
    skip = {excluded, *also_excluded}
    candidates = [version for version in released if version not in skip and _version_key(version)]
    if not candidates:
        return None
    return max(candidates, key=_version_key)


def _is_release_pr_ref(ref: str | None) -> bool:
    """Report whether a git ref belongs to a release-please PR branch."""
    if not ref:
        return False
    # ``GITHUB_REF`` carries a fully qualified ``refs/heads/<branch>`` ref on
    # pushes, and ``refs/pull/<n>/merge`` on pull request runs; only the branch
    # form is unqualified here.
    if ref.startswith("refs/heads/"):
        ref = ref[len("refs/heads/") :]
    return ref.startswith(RELEASE_PR_REF_PREFIX)


def _release_pr_ref(env: dict[str, str] | None = None) -> str | None:
    """Return the branch ref the gate is running against, or ``None`` in CI."""
    environment = os.environ if env is None else env
    for name in RELEASE_PR_REF_ENV_VARS:
        value = environment.get(name)
        if value:
            return value
    return None


def _locked_versions(lock: dict) -> dict:
    """Map package name -> list of versions pinned in a Cargo/uv lockfile."""
    versions: dict = {}
    packages = lock.get("package")
    if not isinstance(packages, list):
        return versions
    for package in packages:
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        version = package.get("version")
        if isinstance(name, str) and isinstance(version, str):
            versions.setdefault(name, []).append(version)
    return versions


def check_cargo_lock_versions(root: Path) -> list[str]:
    """Return errors for workspace crates whose ``Cargo.lock`` pin drifted."""
    cargo_toml_path = root / "Cargo.toml"
    cargo_lock_path = root / "Cargo.lock"
    if not cargo_toml_path.exists() or not cargo_lock_path.exists():
        return []

    workspace = _load_toml(cargo_toml_path).get("workspace")
    if not isinstance(workspace, dict):
        return ["Cargo.toml must declare a [workspace] table"]
    workspace_version = workspace.get("package", {}).get("version")
    members = workspace.get("members")
    if not isinstance(workspace_version, str) or not isinstance(members, list):
        return ["Cargo.toml must declare workspace.package.version and workspace.members"]

    locked = _locked_versions(_load_toml(cargo_lock_path))

    errors = []
    for member in members:
        if not isinstance(member, str):
            continue
        member_toml_path = root / member / "Cargo.toml"
        if not member_toml_path.exists():
            errors.append(f"Cargo.toml member {member!r} has no Cargo.toml")
            continue
        package = _load_toml(member_toml_path).get("package")
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        declared = package.get("version")
        if isinstance(declared, dict):
            # ``version.workspace = true`` inherits the workspace version; any
            # other table form is an inheritance we do not model, so skip it
            # rather than reporting a false drift.
            if declared.get("workspace") is True:
                declared = workspace_version
            else:
                continue
        if not isinstance(name, str) or not isinstance(declared, str):
            continue

        versions = locked.get(name)
        if not versions:
            errors.append(f"Cargo.lock is missing workspace member {name!r}")
        elif declared not in versions:
            errors.append(f"Cargo.lock {name} version {versions[0]!r} != declared {declared!r} ({member}/Cargo.toml)")
    return errors


def check_uv_lock_published_versions(root: Path, *, release_pr: bool = False) -> list[str]:
    """Return errors for ``pkg/`` distributions pinned behind the last release.

    ``release_pr`` grants one extra release of tolerance, matching the
    distance a release-please PR legitimately sits behind the newest published
    wheels while the scheduled refresh is still in flight.
    """
    lock_path = root / "uv.lock"
    pkg_dir = root / "pkg"
    if not lock_path.exists() or not pkg_dir.is_dir():
        return []

    locked = _locked_versions(_load_toml(lock_path))

    changelog_path = root / "CHANGELOG.md"
    released = (
        _RELEASE_HEADING_RE.findall(changelog_path.read_text(encoding="utf-8")) if changelog_path.exists() else []
    )

    errors = []
    for pyproject_path in sorted(pkg_dir.glob("*/pyproject.toml")):
        project = _load_toml(pyproject_path).get("project")
        if not isinstance(project, dict):
            continue
        name = project.get("name")
        declared = project.get("version")
        if not isinstance(name, str) or not isinstance(declared, str):
            continue

        versions = locked.get(name)
        if not versions:
            # Not resolved through uv; there is no pin to compare against.
            continue

        floor = _newest_released_except(released, declared)
        if floor is None:
            # No earlier release to measure against, so every pin is valid.
            continue
        if release_pr:
            # Drop one more release so a release PR is measured the same way
            # ``main`` is. ``or floor`` keeps the stricter floor when no older
            # release is left to fall back to.
            floor = _newest_released_except(released, declared, floor) or floor

        newest = max(versions, key=_version_key)
        if _version_key(newest) < _version_key(floor):
            errors.append(
                f"uv.lock {name} version {newest!r} is behind the last released version "
                f"{floor!r} ({pyproject_path} declares {declared!r}); refresh it with "
                f"`uv lock --upgrade-package {name}`"
            )
    return errors


def main(argv: list[str] | None = None) -> int:
    """Run every lockfile version check against the repository root."""
    args = list(sys.argv[1:] if argv is None else argv)
    root = Path(args[0]) if args else Path.cwd()

    # Only the branch name can say that the pins are still waiting on the
    # scheduled refresh. Outside CI both variables are unset, so a local run
    # and ``main`` keep the strict gate.
    release_pr = _is_release_pr_ref(_release_pr_ref())

    errors = check_cargo_lock_versions(root) + check_uv_lock_published_versions(root, release_pr=release_pr)
    if errors:
        for error in errors:
            print(f"::error::{error}", file=sys.stderr)
        print(
            "::error::Lockfile versions drifted from the declared versions. "
            "See the diff lines above for each offending package.",
            file=sys.stderr,
        )
        return 1
    print("Lockfile versions match the declared versions.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
