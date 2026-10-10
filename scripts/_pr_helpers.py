"""Shared helpers for opening or refreshing an adapter README pointer PR.

One job regenerates the pointer block in every adapter repository. Its two
halves need the same pieces of GitHub plumbing for different reasons:

- The branch name must be stable across runs, or a second catalog change would
  open a duplicate PR next to the first instead of refreshing it.
- The push target depends on the token's actual push permission, not on whether
  its login happens to own the repository.
- The PR must be created once and updated afterwards, because re-opening it on
  every scheduled run would spam subscribers.

This module holds only that plumbing. It knows nothing about pointer blocks.
"""

from __future__ import annotations

import subprocess

# Matches the generator's own per-operation default. Every `gh` call bounded by
# this can be overridden per call site when a shorter bound is wanted.
DEFAULT_TIMEOUT_SECS = 600


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


def _run(args: list[str], *, timeout: int = DEFAULT_TIMEOUT_SECS) -> subprocess.CompletedProcess:
    """Run a command and return the result, converting failures into PrError.

    The timeout matters because this loop walks every catalog repository: a
    single hung ``gh`` call would otherwise consume the whole run and abort it
    before the summary and JSON report are written, leaving the remaining
    repositories silently unchecked.
    """
    try:
        return subprocess.run(args, capture_output=True, text=True, check=False, timeout=timeout)
    except subprocess.TimeoutExpired as exc:
        raise PrError(f"{args[0]} timed out after {timeout}s: {' '.join(args[:3])}") from exc
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


def authenticated_login(*, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
    """Return the GitHub login of the authenticated user, or '' when unknown."""
    result = _run(["gh", "api", "user", "--jq", ".login"], timeout=timeout)
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def repo_owner(repo: str) -> str:
    """Return the owner part of an ``owner/name`` repository slug."""
    return repo.split("/", 1)[0] if "/" in repo else ""


def has_push_access(repo: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> bool:
    """Return True when the authenticated user may push to ``repo``.

    Ownership is not a proxy for this: an organisation repository grants push to
    its members without them owning it, and a token with a narrow scope can be
    read-only on a repository the login *does* own. The REST ``/repos/{repo}``
    payload carries both, so ``permissions.push`` is read directly instead.

    A login is not required first -- the permission is the token's, and the
    token is what pushes.
    """
    result = _run(["gh", "api", f"repos/{repo}", "--jq", ".permissions.push"], timeout=timeout)
    if result.returncode != 0:
        return False
    return result.stdout.strip().lower() == "true"


def _fork_query_jq(login: str) -> str:
    """Build the jq filter that picks the caller's own fork out of ``forks``."""
    return f'.data.repository.forks.nodes[] | select(.owner.login=="{login}") | .nameWithOwner'


def ensure_fork(upstream: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
    """Return a repository the authenticated user can push to.

    ``upstream`` whenever the authenticated user already has push access to it.
    That is the normal case for an organisation repository the job's token is a
    member of, and pushing there keeps the branch and the pull request in one
    repository instead of splitting them across a personal fork.

    A fork is only a last resort, used when the push permission is absent:
    cross-repository pull requests keep the automation working against a
    repository it was not granted write access to. Creating one is never
    automatic -- see ``_ensure_existing_fork`` for why that distinction matters.

    Raises ``PrError`` when neither is available, so a repository the job cannot
    act on is reported as a gap rather than silently skipped.
    """
    if has_push_access(upstream, timeout=timeout):
        return upstream

    login = authenticated_login(timeout=timeout)
    if not login:
        raise PrError(
            f"no push access to {upstream} and the authenticated login is unknown; cannot fall back to a fork"
        )
    return _ensure_existing_fork(upstream, login, timeout=timeout)


def _ensure_existing_fork(upstream: str, login: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
    """Return the caller's existing fork of ``upstream``, or raise.

    The fork is looked up but never created. Auto-forking was the earlier
    behaviour and it is deliberately gone: it silently forked repositories the
    operator had *just* said to work on directly, leaving behind permanent
    personal forks nobody asked for and pull requests opened from the wrong
    place. A missing fork is now an explicit error the operator resolves once.

    Looking the fork up first is also what makes repeat runs safe: an existing
    fork is reused as-is rather than re-created.
    """
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
            _fork_query_jq(login),
        ],
        timeout=timeout,
    )
    if api.returncode == 0 and api.stdout.strip():
        return api.stdout.strip().splitlines()[0].strip()

    raise PrError(
        f"no push access to {upstream} and no fork belonging to {login} was found; "
        f"grant the job's token write access to {upstream}, or create the fork once by hand"
    )


def find_open_pr(repo: str, head: str, base: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
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
        ],
        timeout=timeout,
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


def create_pr(repo: str, *, base: str, head: str, title: str, body: str, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
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
        ],
        timeout=timeout,
    )
    if result.returncode != 0:
        raise PrError(f"gh pr create failed for {repo}: {(result.stderr or result.stdout).strip()}")
    return result.stdout.strip().splitlines()[-1].strip()


def update_pr(repo: str, url: str, *, title: str, body: str, timeout: int = DEFAULT_TIMEOUT_SECS) -> None:
    """Refresh the title and body of an existing pull request."""
    result = _run(["gh", "pr", "edit", url, "--repo", repo, "--title", title, "--body", body])
    if result.returncode != 0:
        raise PrError(f"gh pr edit failed for {url}: {(result.stderr or result.stdout).strip()}")
