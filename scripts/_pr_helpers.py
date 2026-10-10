"""Shared helpers for opening or refreshing an adapter README pointer PR.

One job regenerates the pointer block in every adapter repository. Its two
halves need the same pieces of GitHub plumbing for different reasons:

- The branch name must be stable across runs, or a second catalog change would
  open a duplicate PR next to the first instead of refreshing it.
- The push target depends on the token's actual push permission, not on whether
  its login happens to own the repository. Without confirmed push access the run
  fails loudly; forks are never used or created.
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


# The push-permission lookup has three outcomes, not two. Collapsing the
# failure into a plain False made a transient API error indistinguishable from a
# real "no write access", so a broken lookup fell through to the fork path and
# pushed somewhere the operator never authorised.
PUSH_GRANTED = "granted"
PUSH_DENIED = "denied"
PUSH_UNKNOWN = "unknown"


def check_push_access(repo: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
    """Return whether the authenticated user may push to ``repo``.

    One of ``PUSH_GRANTED``, ``PUSH_DENIED`` or ``PUSH_UNKNOWN``. The third
    state is the point: a lookup that failed is *not* the same as a lookup that
    answered "no", and callers must be able to tell them apart.

    Ownership is not a proxy for any of this: an organisation repository grants
    push to its members without them owning it, and a narrow token can be
    read-only on a repository the login *does* own. The REST ``/repos/{repo}``
    payload carries the real answer, so ``permissions.push`` is read directly.
    """
    result = _run(["gh", "api", f"repos/{repo}", "--jq", ".permissions.push"], timeout=timeout)
    if result.returncode != 0:
        return PUSH_UNKNOWN
    value = result.stdout.strip().lower()
    if value == "true":
        return PUSH_GRANTED
    if value == "false":
        return PUSH_DENIED
    # A 200 response with an empty or unexpected body is also unknown: nothing
    # here has actually confirmed write access.
    return PUSH_UNKNOWN


def _push_access_error(repo: str, state: str, detail: str = "") -> str:
    """Render the failure message for a push state that is not ``PUSH_GRANTED``."""
    suffix = f" ({detail.strip()})" if detail and detail.strip() else ""
    if state == PUSH_UNKNOWN:
        return (
            f"could not confirm push access to {repo}{suffix}; refusing to guess. "
            f"Re-run once the GitHub API is reachable, or grant the token write access to {repo}. "
            "No fork was used or created."
        )
    return (
        f"the authenticated token does not have push access to {repo}{suffix}; "
        f"grant it write access to {repo} and re-run. No fork was used or created."
    )


def ensure_push_target(upstream: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
    """Return the repository to push to: ``upstream``, or raise.

    A branch is only ever pushed into the upstream repository itself, and only
    once the token's push permission has been positively confirmed. That keeps
    the branch and the pull request in one repository, which is what the
    operator asked for.

    Every other outcome raises ``PrError``:

    - ``PUSH_DENIED`` -- the token genuinely lacks write access.
    - ``PUSH_UNKNOWN`` -- the lookup failed, so nothing was confirmed. This is
      treated as a failure, never as permission.

    Forks are not consulted, reused or created under any of them. Silently
    falling back to a personal fork is exactly the behaviour this replaced: it
    moved the branch somewhere the operator had not authorised, and an
    already-existing fork made the fallback succeed quietly.
    """
    state = check_push_access(upstream, timeout=timeout)
    if state == PUSH_GRANTED:
        return upstream
    raise PrError(_push_access_error(upstream, state))


def list_open_prs(repo: str, base: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> list[dict]:
    """Return every open pull request against ``repo``'s ``base`` branch.

    Deliberately unfiltered by head: the caller needs to see PRs opened from a
    fork, from the upstream repository itself, under an older branch name or by
    a different author, because all of those are the same generated change and
    must be recognised as such.

    Raises ``PrError`` when the lookup fails. Silently returning an empty list
    here would be indistinguishable from "no PR exists", and the caller would
    then open a duplicate -- which is exactly the failure this replaced.
    """
    result = _run(
        [
            "gh",
            "pr",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--base",
            base,
            "--json",
            "number,url,title,headRefName,headRepositoryOwner,headRefOid,author,files",
            "--limit",
            "200",
        ],
        timeout=timeout,
    )
    if result.returncode != 0:
        raise PrError(f"gh pr list failed for {repo}: {(result.stderr or result.stdout).strip()}")
    import json

    try:
        rows = json.loads(result.stdout or "[]")
    except ValueError as exc:
        raise PrError(f"gh pr list returned no JSON for {repo}: {exc}") from exc
    return [row for row in rows if isinstance(row, dict)]


def pr_file_blob(repo: str, number: int, path: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
    """Return the blob SHA an open PR leaves at ``path``, or '' when unknown.

    ``gh pr list --json files`` does not populate the blob SHA -- it reports
    path, additions and deletions only -- so content comparison based on that
    output would silently degrade to a title match, which cannot tell one
    catalog state from another. The per-PR files endpoint does carry it.
    """
    result = _run(
        ["gh", "api", f"repos/{repo}/pulls/{number}/files", "--jq", f'.[] | select(.filename=="{path}") | .sha'],
        timeout=timeout,
    )
    if result.returncode != 0:
        return ""
    return result.stdout.strip().splitlines()[0].strip() if result.stdout.strip() else ""


def resolve_pr_blobs(rows: list[dict], repo: str, path: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> list[dict]:
    """Fill in each PR's real blob SHA at ``path``, preserving order."""
    resolved = []
    for row in rows:
        number = row.get("number")
        row = dict(row)
        files = row.get("files") or []
        # Only look it up for single-file PRs whose blob is missing; anything
        # else is already known to be non-equivalent.
        if len(files) == 1 and not _pr_file_identity(row, path) and isinstance(number, int):
            sha = pr_file_blob(repo, number, path, timeout=timeout)
            if sha:
                row["files"] = [dict(files[0], sha=sha)]
        resolved.append(row)
    return resolved


def _pr_file_identity(row: dict, path: str) -> str:
    """Return the blob SHA a PR leaves at ``path``, or '' when it is unknown."""
    for entry in row.get("files") or []:
        if not isinstance(entry, dict):
            continue
        if str(entry.get("path") or "").strip() == path:
            return str(entry.get("sha") or "").strip()
    return ""


def _touches_only(row: dict, path: str) -> bool:
    """Report whether the PR changes ``path`` and nothing else.

    A PR that also carries handwritten work is not the generated change, even if
    its README matches: reusing it would fold unrelated edits into this run's
    output and hide them from the maintainers reviewing the generated PR.
    """
    files = [f for f in (row.get("files") or []) if isinstance(f, dict)]
    if len(files) != 1:
        return False
    return str(files[0].get("path") or "").strip() == path


def find_equivalent_pr(
    rows: list[dict],
    *,
    path: str,
    blob_sha: str,
    title: str,
) -> dict:
    """Return the open PR that already carries this generated change, or {}.

    Equivalence is decided on content, not on identity: the PR must touch only
    ``path`` and leave ``blob_sha`` there. Everything else -- head branch name,
    head owner, fork or same-repository, author, creation time -- is
    deliberately ignored, because a generated PR legitimately differs in all of
    those across runs:

    - the branch fingerprint was renamed, moving old PRs to a different head
    - the head moved from a personal fork to the upstream repository
    - a scheduled run, a push run and an operator dispatch each open their own

    Identity-based matching (head branch + base) treated every one of those as
    new work and opened a second PR beside an open, byte-identical first one.

    A PR whose blob is readable and different is *not* equivalent even when the
    title matches: every refresh this automation opens shares the same title, so
    the title cannot tell one catalog state from another. It is only consulted
    when the blob is missing, so a PR whose blob could not be read still counts.

    A PR carrying additional files is not equivalent either: it contains work
    this automation did not generate and must not be reused.
    """
    wanted_title = str(title or "").strip()
    fallback = None
    for row in rows:
        if not _touches_only(row, path):
            continue
        if _pr_file_identity(row, path) != blob_sha:
            continue
        if wanted_title and str(row.get("title") or "").strip() == wanted_title:
            return row
        fallback = fallback or row
    if fallback:
        return fallback

    # No content match, so the blob is either unreadable or genuinely different.
    # Only the unreadable case may fall back to a title match: a PR whose blob is
    # readable and *disagrees* is different content, and reusing it would point
    # the run at a PR that does not carry this change -- the title is shared by
    # every refresh this automation has ever opened, so it cannot distinguish
    # one catalog state from another.
    for row in rows:
        if not _touches_only(row, path):
            continue
        if _pr_file_identity(row, path):
            continue  # readable and different: not equivalent
        if wanted_title and str(row.get("title") or "").strip() == wanted_title:
            return row
    return {}


def find_open_pr(repo: str, head: str, base: str, *, timeout: int = DEFAULT_TIMEOUT_SECS) -> str:
    """Return the URL of an open pull request for ``head``, or '' if there is none.

    Kept for callers that want the narrow, head-branch-scoped lookup. The
    regeneration path uses :func:`find_equivalent_pr` instead: this one only
    matches on the head ref, so it cannot see an equivalent PR opened from a
    fork, under a previous branch name or by another author.

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
    result = _run(
        ["gh", "pr", "edit", url, "--repo", repo, "--title", title, "--body", body],
        timeout=timeout,
    )
    if result.returncode != 0:
        raise PrError(f"gh pr edit failed for {url}: {(result.stderr or result.stdout).strip()}")
