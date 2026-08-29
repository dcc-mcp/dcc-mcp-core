#!/usr/bin/env python3
"""Fail-closed generated-lock synchronization helpers.

Generation is intentionally executed with a scrubbed environment.  The
workflow performs the read-only identity/diff checks before creating a local
commit and uses a force-with-lease push for the final, narrowly scoped write.
"""

from __future__ import annotations

import argparse
from contextlib import suppress
import ctypes
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from typing import Iterable
from typing import Mapping
from typing import NamedTuple
from typing import NoReturn
from typing import Sequence
from urllib.parse import urlparse

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
        run_bounded(command, cwd=root, env=env, timeout_seconds=timeout_seconds)


def process_exists(pid: int) -> bool:
    """Return whether a process ID is still live."""
    if os.name == "nt":
        result = subprocess.run(
            ("tasklist", "/FI", f"PID eq {pid}", "/NH"),
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        return result.returncode == 0 and str(pid) in result.stdout
    try:
        os.kill(pid, 0)
    except (OSError, ProcessLookupError):
        return False
    return True


def run_bounded(
    command: Sequence[str],
    *,
    cwd: Path | None = None,
    env: Mapping[str, str] | None = None,
    timeout_seconds: float,
) -> None:
    """Run a command in a process group/tree and kill descendants on timeout."""
    if os.name not in ("nt", "posix"):
        raise RuntimeError("process containment is unavailable on this platform")
    _enable_child_subreaper()
    creationflags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0) if os.name == "nt" else 0
    process = subprocess.Popen(
        command,
        cwd=str(cwd) if cwd is not None else None,
        env=dict(env) if env is not None else None,
        start_new_session=os.name != "nt",
        creationflags=creationflags,
    )
    try:
        process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        descendants: set[int] = set()
        if os.name == "nt":
            subprocess.run(("taskkill", "/PID", str(process.pid), "/T", "/F"), check=False)
        else:
            with suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGKILL)
            for _ in range(10):
                current = set(_descendant_pids(process.pid))
                descendants.update(current)
                for pid in current:
                    with suppress(ProcessLookupError):
                        os.kill(pid, signal.SIGKILL)
                if not current:
                    break
                time.sleep(0.02)
        process.communicate()
        remaining = [pid for pid in descendants if process_exists(pid)]
        if remaining:
            raise RuntimeError(f"process containment failed; descendants survived timeout: {remaining}") from None
        raise
    if process.returncode:
        raise subprocess.CalledProcessError(process.returncode, command)


def _enable_child_subreaper() -> None:
    """Arrange for escaped POSIX descendants to be reparented to this process."""
    if os.name != "posix" or not sys.platform.startswith("linux"):
        return
    try:
        libc = ctypes.CDLL(None)
        libc.prctl(36, 1, 0, 0, 0)  # PR_SET_CHILD_SUBREAPER
    except (AttributeError, OSError):
        return


def _descendant_pids(root_pid: int) -> list[int]:
    """Return the currently observable descendant process IDs."""
    result = subprocess.run(("ps", "-eo", "pid=,ppid="), check=False, stdout=subprocess.PIPE, text=True)
    children: dict[int, list[int]] = {}
    for line in result.stdout.splitlines():
        fields = line.split()
        if len(fields) != 2:
            continue
        pid, parent = (int(fields[0]), int(fields[1]))
        children.setdefault(parent, []).append(pid)
    pending = list(children.get(root_pid, []))
    descendants: list[int] = []
    while pending:
        pid = pending.pop()
        descendants.append(pid)
        pending.extend(children.get(pid, []))
    return descendants


def _remote_repository(url: str) -> str | None:
    """Parse a GitHub remote URL into owner/repository, rejecting ambiguity."""
    value = url.strip()
    if value.startswith("git@"):
        prefix, separator, path = value.partition(":")
        if separator and prefix == "git@github.com":
            return path[:-4].strip("/") if path.endswith(".git") else path.strip("/")
        return None
    parsed = urlparse(value)
    if parsed.scheme not in ("https", "ssh") or parsed.hostname != "github.com" or parsed.port is not None:
        return None
    if parsed.username not in (None, "git") or parsed.password is not None:
        return None
    path = parsed.path
    return (path[:-4] if path.endswith(".git") else path).strip("/")


def validate_remote_url(url: str, expected_repository: str) -> list[str]:
    """Ensure a remote URL targets github.com and the expected owner/repo."""
    repository = _remote_repository(url)
    if repository != expected_repository:
        return [f"remote URL is not bound to expected repository {expected_repository!r}"]
    return []


def validate_remote_urls(urls: Iterable[str], expected_repository: str, kind: str) -> list[str]:
    """Require one and only one origin URL for each fetch/push direction."""
    entries = [url.strip() for url in urls if url.strip()]
    if len(entries) != 1:
        return [f"origin {kind} URL must have exactly one configured entry"]
    return validate_remote_url(entries[0], expected_repository)


def validate_remote(root: Path, expected_repository: str) -> None:
    """Validate both fetch and push URLs before any generated-lock mutation."""
    for kind, args in (
        ("fetch", ("get-url", "--all", "origin")),
        ("push", ("get-url", "--push", "--all", "origin")),
    ):
        result = subprocess.run(("git", "remote", *args), cwd=str(root), check=False, stdout=subprocess.PIPE, text=True)
        if result.returncode != 0:
            _fail(f"could not read origin {kind} URL")
        errors = validate_remote_urls(result.stdout.splitlines(), expected_repository, kind)
        if errors:
            _fail(f"origin {kind} URL rejected: {errors[0]}")


def validate_force_with_lease(expected_sha: str, observed_sha: str) -> list[str]:
    """Return an error when the remote head changed since preflight."""
    if expected_sha != observed_sha:
        return ["stale head: remote branch advanced since preflight"]
    return []


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


def verify_commit(root: Path, expected_parent: str) -> None:
    """Ensure the commit has exactly the immutable PR head as its parent."""
    parents = _git(root, "rev-list", "--parents", "-n", "1", "HEAD").split()
    if len(parents) != 2 or parents[1] != expected_parent:
        _fail("generated commit parent is not the immutable original PR head")
    paths = _git(root, "diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD").splitlines()
    errors = validate_changed_files(paths)
    if errors:
        _fail(errors[0])


def main() -> None:
    """Dispatch the requested generated-lock contract operation."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command",
        choices=("generate", "preflight", "remote-preflight", "validate-remote", "verify-diff", "verify-commit"),
    )
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--expected-parent", default=os.environ.get("PR_HEAD_SHA", ""))
    args = parser.parse_args()
    if args.command == "generate":
        run_generation(args.root)
    elif args.command == "preflight":
        preflight(args.root)
    elif args.command == "remote-preflight":
        _remote_preflight(_identity_from_env())
    elif args.command == "validate-remote":
        repository = os.environ.get("GITHUB_REPOSITORY", "")
        if not repository:
            _fail("GITHUB_REPOSITORY is required")
        validate_remote(args.root, repository)
    elif args.command == "verify-commit":
        if not args.expected_parent:
            _fail("expected immutable PR head is required")
        verify_commit(args.root, args.expected_parent)
    else:
        verify_diff(args.root)


if __name__ == "__main__":
    main()
