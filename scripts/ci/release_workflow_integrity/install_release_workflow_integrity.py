#!/usr/bin/env python3
"""Install the release workflow digest drift check into a repository.

The check itself lives in two files that carry no repository-specific content:
every path they use is derived from their own location. Porting the check to
another repository is therefore a copy plus one snapshot initialisation, which
is exactly what this script performs:

    python scripts/ci/release_workflow_integrity/install_release_workflow_integrity.py \
        --target ../dcc-mcp-unreal

It writes

    <target>/scripts/ci/check_release_workflow_digest.py   the checker
    <target>/tests/test_release_workflow_integrity.py      the fail-closed suite
    <target>/scripts/ci/approved_release_workflow.yml      the approved snapshot

and then verifies the snapshot it just wrote matches the target's
`.github/workflows/release.yml`, so a repository never lands with a red check.

Existing files are left alone unless ``--force`` is given, which lets a
repository re-sync with a newer kit copy without losing an approved snapshot
the operator did not intend to refresh.

Usage:
    install_release_workflow_integrity.py --target PATH [--workflow REL] [--force]

Exit codes: ``0`` on success, ``1`` when the install cannot be completed.
"""

from __future__ import annotations

import argparse
import importlib.util
import pathlib
import shutil
import sys

KIT_DIR = pathlib.Path(__file__).resolve().parent
CHECKER_NAME = "check_release_workflow_digest.py"
TEST_NAME = "test_release_workflow_integrity.py"
SNAPSHOT_NAME = "approved_release_workflow.yml"
DEFAULT_WORKFLOW = pathlib.Path(".github") / "workflows" / "release.yml"


def _install_one(source: pathlib.Path, destination: pathlib.Path, force: bool) -> bool:
    """Copy ``source`` to ``destination`` unless it exists and ``force`` is off.

    Returns True when this call wrote the file.
    """
    if destination.exists() and not force:
        print(f"kept      {destination}")
        return False
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    print(f"installed {destination}")
    return True


def _load_checker(path: pathlib.Path):
    """Load a checker module from an explicit path."""
    spec = importlib.util.spec_from_file_location("_release_workflow_checker", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def install(target: pathlib.Path, workflow: pathlib.Path, force: bool) -> int:
    """Copy the kit into ``target`` and initialise its snapshot."""
    if not target.is_dir():
        print(f"install failed: target is not a directory: {target}", file=sys.stderr)
        return 1
    release_workflow = target / workflow
    if not release_workflow.is_file():
        print(f"install failed: no release workflow at {release_workflow}", file=sys.stderr)
        return 1

    installed_checker = target / "scripts" / "ci" / CHECKER_NAME
    wrote_checker = _install_one(KIT_DIR / CHECKER_NAME, installed_checker, force)
    _install_one(KIT_DIR / TEST_NAME, target / "tests" / TEST_NAME, force)
    snapshot_written = False

    snapshot = target / "scripts" / "ci" / SNAPSHOT_NAME
    if snapshot.exists() and not force:
        print(f"kept      {snapshot}")
    else:
        snapshot.parent.mkdir(parents=True, exist_ok=True)
        snapshot.write_bytes(release_workflow.read_bytes())
        print(f"installed {snapshot}")
        snapshot_written = True

    # Verify with the checker that was actually written. A kept copy may be a
    # locally modified one, so verifying against it would report someone else's
    # edit as an install failure; the kit template is the same code whenever
    # this run installed the checker.
    verification_path = installed_checker if (wrote_checker or snapshot_written) else KIT_DIR / CHECKER_NAME
    try:
        module = _load_checker(verification_path)
        approved = module.release_workflow_digest(snapshot)
        candidate = module.release_workflow_digest(release_workflow)
    except Exception as exc:  # any failure here is a failed install, not a partial success
        print(f"install failed: cannot verify {release_workflow}: {exc}", file=sys.stderr)
        return 1

    if approved != candidate:
        print(
            "install failed: snapshot does not match the release workflow\n"
            f"  approved  : {approved}\n"
            f"  candidate : {candidate}",
            file=sys.stderr,
        )
        return 1
    print(f"verified   {snapshot} matches {release_workflow}\nrelease workflow digest: {approved}")
    return 0


def main() -> int:
    """Install the kit into the requested repository; return the exit code."""
    parser = argparse.ArgumentParser(description="Install the release workflow digest drift check into a repository.")
    parser.add_argument("--target", type=pathlib.Path, required=True, help="repository root to install into")
    parser.add_argument(
        "--workflow",
        type=pathlib.Path,
        default=DEFAULT_WORKFLOW,
        help=(
            "release workflow path relative to the target (default: "
            f"{DEFAULT_WORKFLOW.as_posix()}). A non-default path also needs the same path set in "
            "RELEASE_WORKFLOW inside the installed checker."
        ),
    )
    parser.add_argument("--force", action="store_true", help="overwrite files that already exist")
    return install(parser.parse_args().target, parser.parse_args().workflow, parser.parse_args().force)


if __name__ == "__main__":
    raise SystemExit(main())
