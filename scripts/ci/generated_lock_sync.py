#!/usr/bin/env python3
"""Fail-closed generated-lock synchronization helpers.

Generation is intentionally executed with a scrubbed environment.  The
workflow performs the read-only identity/diff checks before creating a local
commit and uses a force-with-lease push for the final, narrowly scoped write.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Iterable
from typing import Mapping
from typing import NamedTuple
from typing import NoReturn

LOCK_OUTPUTS = frozenset(("Cargo.lock", "uv.lock", "crates/workspace-hack/Cargo.toml"))
BRANCH_PREFIXES = ("release-please--branches--main", "renovate/")
CREDENTIAL_ENV_KEYS = frozenset(
    (
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "PERSONAL_ACCESS_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "GIT_ASKPASS",
        "SSH_AUTH_SOCK",
    )
)


class PullRequestIdentity(NamedTuple):
    """Immutable identity tuple for the eligible pull request head."""

    repository: str
    number: int
    head_repository: str
    head_branch: str
    head_sha: str
    title: str


def sanitized_environment(source: Mapping[str, str]) -> dict[str, str]:
    """Return an environment safe for project-controlled build backends."""
    env = {key: value for key, value in source.items() if key not in CREDENTIAL_ENV_KEYS}
    # Do not allow a repository checkout to re-enable a credential-bearing
    # global/system Git config or prompt for credentials.
    env.pop("GIT_CONFIG_GLOBAL", None)
    env.pop("GIT_CONFIG_SYSTEM", None)
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    env["GIT_CONFIG_GLOBAL"] = os.devnull
    env["GIT_TERMINAL_PROMPT"] = "0"
    return env


def validate_identity(expected: PullRequestIdentity, observed: PullRequestIdentity) -> list[str]:
    """Compare every PR identity field and reject forks/unsupported branches."""
    errors: list[str] = []
    if observed.head_repository != observed.repository:
        errors.append("forked pull requests are not eligible for generated-lock writes")
    if observed.repository != expected.repository:
        errors.append("repository identity drift")
    if observed.number != expected.number:
        errors.append("pull request number drift")
    if observed.head_repository != expected.head_repository:
        errors.append("head repository identity drift")
    if observed.head_branch != expected.head_branch:
        errors.append("head branch identity drift")
    if observed.head_sha != expected.head_sha:
        errors.append("head SHA identity drift")
    if observed.title != expected.title:
        errors.append("pull request title identity drift")
    if not observed.head_branch.startswith(BRANCH_PREFIXES):
        errors.append("head branch is outside the approved release/automation branch class")
    if observed.head_branch.startswith("release-please--branches--main") and not observed.title.startswith(
        "chore(main): release "
    ):
        errors.append("release-please branch has an unexpected title")
    return errors


def validate_changed_files(paths: Iterable[str]) -> list[str]:
    """Return a stable error when generation touched anything beyond lock outputs."""
    unexpected = sorted(set(paths) - LOCK_OUTPUTS)
    if unexpected:
        return [f"unexpected generated-lock diff paths: {', '.join(unexpected)}"]
    return []


def run_generation(root: Path, *, timeout_seconds: int = 900) -> None:
    """Run lock generators with bounded timeouts and a scrubbed environment."""
    env = sanitized_environment(os.environ)
    commands = (("cargo", "update", "-w"), ("cargo", "hakari", "generate"), ("vx", "uv", "lock"))
    for command in commands:
        subprocess.run(command, cwd=str(root), env=env, check=True, timeout=timeout_seconds)


def _fail(message: str) -> NoReturn:
    print(f"::error::{message}", file=sys.stderr)
    raise SystemExit(1)


def _identity_from_env() -> PullRequestIdentity:
    values = {
        "repository": os.environ.get("GITHUB_REPOSITORY", ""),
        "head_repository": os.environ.get("PR_HEAD_REPOSITORY", ""),
        "head_branch": os.environ.get("PR_HEAD_REF", ""),
        "head_sha": os.environ.get("PR_HEAD_SHA", ""),
        "title": os.environ.get("PR_TITLE", ""),
    }
    try:
        number = int(os.environ.get("PR_NUMBER", "0"))
    except ValueError:
        number = 0
    if not values["repository"] or not values["head_repository"] or not values["head_branch"] or not values["head_sha"]:
        _fail("all pull request identity fields are required")
    return PullRequestIdentity(number=number, **values)


def _git(root: Path, *args: str) -> str:
    result = subprocess.run(("git", *args), cwd=str(root), check=True, stdout=subprocess.PIPE, text=True)
    return result.stdout.strip()


def _remote_preflight(expected: PullRequestIdentity) -> None:
    """Re-capture the remote PR identity using a read-only token."""
    if os.environ.get("DCC_LOCK_SYNC_SKIP_REMOTE") == "1":
        return
    result = subprocess.run(
        ("gh", "api", f"repos/{expected.repository}/pulls/{expected.number}"),
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=30,
    )
    if result.returncode != 0:
        _fail(f"could not recapture pull request identity: {result.stdout.strip()}")
    payload = json.loads(result.stdout)
    if payload.get("state") != "open" or payload.get("base", {}).get("ref") != "main":
        _fail("pull request is no longer an open PR targeting main")
    if payload.get("base", {}).get("repo", {}).get("full_name") != expected.repository:
        _fail("pull request base repository identity drift")
    remote = PullRequestIdentity(
        repository=expected.repository,
        number=expected.number,
        head_repository=str(payload.get("head", {}).get("repo", {}).get("full_name", "")),
        head_branch=str(payload.get("head", {}).get("ref", "")),
        head_sha=str(payload.get("head", {}).get("sha", "")),
        title=str(payload.get("title", "")),
    )
    errors = validate_identity(expected, remote)
    if errors:
        _fail("; ".join(errors))


def preflight(root: Path) -> None:
    """Revalidate the exact local head and remote PR head."""
    expected = _identity_from_env()
    observed = PullRequestIdentity(
        repository=expected.repository,
        number=expected.number,
        head_repository=expected.head_repository,
        # The checkout is intentionally detached at the immutable PR SHA;
        # branch identity comes from the event and remote re-capture below.
        head_branch=expected.head_branch,
        head_sha=_git(root, "rev-parse", "HEAD"),
        title=expected.title,
    )
    errors = validate_identity(expected, observed)
    if errors:
        _fail("; ".join(errors))
    _remote_preflight(expected)


def verify_diff(root: Path) -> None:
    """Reject tracked or untracked changes outside the lock output allowlist."""
    paths = _git(root, "diff", "--name-only").splitlines()
    for status in _git(root, "status", "--porcelain=v1", "--untracked-files=all").splitlines():
        if not status:
            continue
        path = status[3:]
        if " -> " in path:
            paths.extend(path.split(" -> "))
        else:
            paths.append(path)
    errors = validate_changed_files(paths)
    if errors:
        _fail(errors[0])


def verify_commit(root: Path) -> None:
    """Ensure the commit about to be pushed contains only lock outputs."""
    paths = _git(root, "diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD").splitlines()
    errors = validate_changed_files(paths)
    if errors:
        _fail(errors[0])


def main() -> None:
    """Dispatch the requested generated-lock contract operation."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command", choices=("generate", "preflight", "remote-preflight", "verify-diff", "verify-commit")
    )
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    if args.command == "generate":
        run_generation(args.root)
    elif args.command == "preflight":
        preflight(args.root)
    elif args.command == "remote-preflight":
        _remote_preflight(_identity_from_env())
    elif args.command == "verify-commit":
        verify_commit(args.root)
    else:
        verify_diff(args.root)


if __name__ == "__main__":
    main()
