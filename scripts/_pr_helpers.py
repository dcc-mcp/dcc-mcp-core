"""Shared helpers for opening or refreshing an adapter README pointer PR.

One job regenerates the pointer block in every adapter repository. Its two
halves need the same pieces of GitHub plumbing for different reasons:

- The branch name must be stable across runs, or a second catalog change would
  open a duplicate PR next to the first instead of refreshing it.
- Forks cannot be pushed to directly, so the push target depends on whether the
  authenticated user owns the repository.
- The PR must be created once and updated afterwards, because re-opening it on
  every scheduled run would spam subscribers.

This module holds only that plumbing. It knows nothing about pointer blocks.
"""

from __future__ import annotations

import subprocess


class PrError(RuntimeError):
    """Raised when a git or ``gh`` command fails."""


def branch_slug(prefix: str, value: str) -> str:
    """Turn a dynamic suffix into a git-ref-safe branch name fragment.

    Catalog descriptions are free-form prose, so the regeneration fingerprint
    that goes into a branch name can contain spaces, slashes and punctuation
    that git refuses in a ref.
    """
    cleaned = "".join(char if char.isalnum() or char in ".-" else "-" for char in str(value or ""))
    while "--" in cleaned:
        cleaned = cleaned.replace("--", "-")
    return f"{prefix}{cleaned.strip('-')}"


def _run(args: list[str]) -> subprocess.CompletedProcess:
    """Run a command and return the result, converting OS errors into PrError."""
    try:
        return subprocess.run(args, capture_output=True, text=True, check=False)
    except OSError as exc:  # pragma: no cover - gh or git missing
        raise PrError(f"could not run {args[0]}: {exc}") from exc


def _gh_json(args: list[str]) -> list[dict]:
    """Run a ``gh`` command that returns JSON and raise PrError on failure."""
    import json

    result = _run(["gh", *args])
    if result.returncode != 0:
        raise PrError(f"gh {' '.join(args)} failed: {(result.stderr or result.stdout).strip()}")
    try:
        return json.loads(result.stdout or "[]")
    except ValueError as exc:
        raise PrError(f"gh {' '.join(args)} returned no JSON: {exc}") from exc


def authenticated_login() -> str:
    """Return the GitHub login of the authenticated user, or '' when unknown."""
    result = _run(["gh", "api", "user", "--jq", ".login"])
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def repo_owner(repo: str) -> str:
    """Return the owner part of an ``owner/name`` repository slug."""
    return repo.split("/", 1)[0] if "/" in repo else ""


def ensure_fork(upstream: str) -> str:
    """Return a repository the authenticated user can push to.

    ``upstream`` when the caller already owns it; otherwise a fork of it.
    Cross-repository pull requests are opened from a fork because the job's
    token is not guaranteed write access to every adapter in the catalog, and a
    fork keeps the automation working regardless of which repositories it is
    later granted.

    ``gh repo fork`` is idempotent: it reports the existing fork when one
    already exists, so this is safe to call on every run.
    """
    login = authenticated_login()
    if not login or login.lower() == repo_owner(upstream).lower():
        return upstream

    # `--clone=false` with an equals sign: `--clone false` is read as a clone
    # destination named "false". No clone is wanted -- this function only needs
    # the fork to exist so a branch can be pushed to it.
    result = _run(["gh", "repo", "fork", upstream, "--clone=false"])
    # An existing fork is reported on stderr with a non-zero exit, which is the
    # success case for this call, not a failure. The lookup below is what
    # actually proves the fork is usable.
    if result.returncode != 0 and "already exists" not in (result.stderr or ""):
        raise PrError(f"gh repo fork failed for {upstream}: {(result.stderr or result.stdout).strip()}")

    # The fork's slug is resolved through the API rather than parsed out of
    # `gh repo fork`'s prose: that command has no `--json` mode, and `gh repo
    # view <upstream>` reports the upstream even after a fork exists. The API
    # `forks` connection is authoritative.
    owner, name = upstream.split("/", 1) if "/" in upstream else ("", upstream)
    # Filtered in jq rather than through a query argument: `forks` does not take
    # an affiliation filter, and other people's forks are not pushable.
    query = (
        "query($owner:String!,$name:String!){repository(owner:$owner,name:$name)"
        "{forks(first:20){nodes{nameWithOwner owner{login}}}}}"
    )
    api = _run(
        [
            "gh",
            "api",
            "graphql",
            "-f",
            f"query={query}",
            "-F",
            f"owner={owner}",
            "-F",
            f"name={name}",
            "--jq",
            f'.data.repository.forks.nodes[] | select(.owner.login=="{login}") | .nameWithOwner',
        ]
    )
    if api.returncode == 0 and api.stdout.strip():
        return api.stdout.strip().splitlines()[0].strip()

    raise PrError(f"could not find or create a fork of {upstream} for {login}")


def find_open_pr(repo: str, head: str, base: str) -> str:
    """Return the URL of an open pull request for ``head``, or '' if there is none.

    Matched on the head ref rather than on the title so a reworded title still
    finds the existing PR and refreshes it instead of opening a second one.

    ``--head`` takes the bare branch name, not the ``owner:branch`` form that a
    cross-repository PR is *created* with; passing the qualified form matches
    nothing and every rerun would try to open a duplicate.
    """
    bare = head.split(":", 1)[1] if ":" in head else head
    result = _run(
        [
            "gh",
            "pr",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--head",
            bare,
            "--base",
            base,
            "--json",
            "url",
        ]
    )
    if result.returncode != 0:
        return ""
    import json

    try:
        rows = json.loads(result.stdout or "[]")
    except ValueError:
        return ""
    for row in rows:
        url = str(row.get("url") or "").strip()
        if url:
            return url
    return ""


def create_pr(repo: str, *, base: str, head: str, title: str, body: str) -> str:
    """Open a pull request and return its URL."""
    result = _run(
        [
            "gh",
            "pr",
            "create",
            "--repo",
            repo,
            "--base",
            base,
            "--head",
            head,
            "--title",
            title,
            "--body",
            body,
        ]
    )
    if result.returncode != 0:
        raise PrError(f"gh pr create failed for {repo}: {(result.stderr or result.stdout).strip()}")
    return result.stdout.strip().splitlines()[-1].strip()


def update_pr(repo: str, url: str, *, title: str, body: str) -> None:
    """Refresh the title and body of an existing pull request."""
    result = _run(["gh", "pr", "edit", url, "--repo", repo, "--title", title, "--body", body])
    if result.returncode != 0:
        raise PrError(f"gh pr edit failed for {url}: {(result.stderr or result.stdout).strip()}")
