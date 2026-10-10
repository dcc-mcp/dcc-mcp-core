#!/usr/bin/env python3
"""Regenerate the pointer block in every adapter README and open or refresh a PR.

``check_adapter_pointer_drift.py`` can only report drift. Reporting is not
enough: the catalog grows, every adapter README goes stale the moment it does,
and a detector that is permanently red is a detector people learn to ignore.
This script is the other half -- it regenerates the block each adapter should
carry and pushes it as a pull request, so the drift lane can turn green on its
own instead of waiting for someone to run another manual batch.

The regeneration itself is not new logic. It delegates to
``generate_adapter_pointer.py``, the same generator the drift check replays, so
the text a PR proposes is by construction the text the check demands.

Safety
------
- Only ever edits the generated block. It refuses to touch a repository whose
  resulting diff touches anything but ``README.md``.
- Opens a PR instead of pushing to a default branch, and refreshes the same PR
  on later runs rather than opening duplicates.
- ``--dry-run`` performs no writes, no pushes and no PR calls. It still reports
  exactly what would happen, which is the intended way to review a catalog
  change before letting a schedule act on it.

Usage
-----
    # Report what every adapter needs, changing nothing.
    python scripts/regenerate_adapter_pointers.py --dry-run --summary

    # Regenerate the real fleet and open or refresh one PR per repository.
    python scripts/regenerate_adapter_pointers.py --json-out regen-results.json

Exit codes
----------
0   every scanned repository is current, or every needed PR was opened
1   at least one repository could not be regenerated, or dry-run found work to do
2   the catalog or the arguments could not be used at all
"""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT))

from scripts import _pr_helpers as pr_helpers  # noqa: E402
from scripts import check_adapter_pointer_drift as drift  # noqa: E402
from scripts import generate_adapter_pointer as generator  # noqa: E402

# Not `docs/...`: a fork can already carry a branch literally named `docs`, and
# git refuses to create `docs/<name>` under it ("directory file conflict").
BRANCH_PREFIX = "chore/refresh-catalog-pointer-"
PR_TITLE = "docs: refresh the generated DCC-MCP host matrix pointer"

CURRENT = "current"
REGENERATED = "regenerated"
MISSING = "missing"
ERROR = "error"

DEFAULT_TIMEOUT_SECS = 600

# The regeneration only ever rewrites the pointer block inside README.md. A
# repository whose working tree picks up any other change has something running
# in it that this script does not understand (a hook, a dirty checkout), and the
# safe move is to leave it alone and report it.
ALLOWED_CHANGED_FILES = ("README.md",)


class RegenError(RuntimeError):
    """Raised when one repository cannot be regenerated."""


def _git(args: list[str], *, timeout: int) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["git", *args],
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def _clone(url: str, dest: Path, branch: str, *, timeout: int) -> None:
    result = _git(
        [
            "clone",
            "--quiet",
            "--no-tags",
            "--single-branch",
            "--branch",
            branch,
            url,
            str(dest),
        ],
        timeout=timeout,
    )
    if result.returncode != 0:
        raise RegenError(f"clone failed for {url}: {(result.stderr or result.stdout).strip()}")


def changed_files(repo_dir: Path, *, timeout: int) -> list[str]:
    """Return the paths git reports as modified or untracked in ``repo_dir``."""
    result = _git(["-C", str(repo_dir), "status", "--porcelain"], timeout=timeout)
    if result.returncode != 0:
        raise RegenError(f"git status failed: {(result.stderr or result.stdout).strip()}")
    names = []
    for line in result.stdout.splitlines():
        # A porcelain row is a two-character status followed by a path; a rename
        # carries two paths, and the second is the one now on disk.
        path = line[3:].split(" -> ")[-1].strip().strip('"')
        if path:
            names.append(path)
    return names


def render_pr_body(entry: dict, *, host_count: int, source_ref: str) -> str:
    """Render the body of the refresh PR for one adapter."""
    name = str(entry.get("name") or "").strip()
    return (
        "The DCC-MCP catalog now lists"
        f" **{host_count} host adapters**. This repository's README quoted an older"
        " count, so the generated pointer block no longer matched what the catalog"
        " produces.\n\n"
        f"This PR re-runs `scripts/generate_adapter_pointer.py` in {source_ref} for"
        f" `{name}` and commits the result. Only the generated block between the"
        " coverage-pointer markers changes; nothing else in the README is touched.\n\n"
        "The block is generated from `dcc-mcp-catalog.yml` in"
        " https://github.com/dcc-mcp/dcc-mcp-core and is not edited by hand. A"
        " scheduled job keeps it in sync, so a later catalog change refreshes this"
        " PR instead of opening another one."
    )


def regenerate_repository(
    entry: dict,
    entry_index: int,
    *,
    host_count: int,
    workdir: Path,
    timeout: int,
    branch_prefix: str,
    dry_run: bool,
    source_ref: str,
) -> dict:
    """Regenerate one adapter's pointer block and open or refresh its PR."""
    name = str(entry.get("name") or "").strip() or f"<entry {entry_index}>"
    url = str(entry.get("url") or "").strip()
    row = {
        "name": name,
        "url": url,
        "status": ERROR,
        "branch": "",
        "detail": "",
        "pr": "",
        "action": "none",
    }

    if not url:
        row["detail"] = "catalog entry has no `url`; nothing to regenerate"
        return row

    repo_dir = workdir / drift._safe_dirname(name, entry_index)

    try:
        default_branch = drift._default_branch_name(url, timeout=timeout) or "main"
        row["branch"] = default_branch
        _clone(url, repo_dir, default_branch, timeout=timeout)

        readme = repo_dir / "README.md"
        if not readme.is_file():
            row["status"] = MISSING
            row["detail"] = "no README.md on the default branch"
            return row

        path, changed = generator.apply_pointer(repo_dir, entry, host_count=host_count, write=not dry_run)
        if not changed:
            row["status"] = CURRENT
            row["detail"] = f"matches the catalog output ({host_count} adapters)"
            return row

        unexpected = [p for p in changed_files(repo_dir, timeout=timeout) if p not in ALLOWED_CHANGED_FILES]
        if unexpected:
            raise RegenError(f"refusing to commit unrelated changes: {unexpected}")

        if dry_run:
            row["status"] = REGENERATED
            row["action"] = "would-open-pr"
            row["detail"] = f"would refresh the pointer block in {path.name}"
            return row

        repo_slug = _repo_slug(url)
        head = pr_helpers.branch_slug(branch_prefix, _fingerprint(host_count))
        push_target, head_ref = _push_target(repo_slug, head, timeout=timeout)

        _commit_and_push(repo_dir, head, push_target, timeout=timeout)

        title = PR_TITLE
        body = render_pr_body(entry, host_count=host_count, source_ref=source_ref)
        existing = pr_helpers.find_open_pr(repo_slug, head_ref, default_branch, timeout=timeout)
        if existing:
            pr_helpers.update_pr(repo_slug, existing, title=title, body=body, timeout=timeout)
            row["action"] = "updated-pr"
            row["pr"] = existing
        else:
            row["pr"] = pr_helpers.create_pr(
                repo_slug, base=default_branch, head=head_ref, title=title, body=body, timeout=timeout
            )
            row["action"] = "opened-pr"

        row["status"] = REGENERATED
        row["detail"] = f"refreshed the pointer block ({host_count} adapters)"
        return row
    except Exception as exc:
        # Broad on purpose, for the same reason the drift scanner does it: this
        # loop walks repositories owned by other teams, and one unexpected
        # failure must not abort the run before the remaining repositories are
        # processed or the report is written.
        row["status"] = ERROR
        row["detail"] = f"{type(exc).__name__}: {exc}".strip()
        return row
    finally:
        drift._remove_clone(repo_dir)


def _fingerprint(host_count: int) -> str:
    """Return the branch suffix this batch regenerates under.

    Deliberately independent of ``host_count``. An earlier revision returned
    ``f"{host_count}-adapters"``, which meant a catalog change moved the branch
    and opened a second pull request beside the first: every count change would
    leave a fresh batch of orphan PRs across the fleet with nothing closing
    them. One branch per repository, force-pushed on each run, so a later
    catalog change refreshes the same PR.
    """
    return "catalog-pointer"


def _repo_slug(url: str) -> str:
    """Reduce a catalog repository URL to its ``owner/name`` slug."""
    cleaned = str(url or "").strip().rstrip("/")
    if cleaned.endswith(".git"):
        cleaned = cleaned[: -len(".git")]
    for prefix in ("https://github.com/", "http://github.com/", "git@github.com:"):
        if cleaned.startswith(prefix):
            cleaned = cleaned[len(prefix) :]
            break
    return cleaned


def _push_target(repo_slug: str, head: str, *, timeout: int) -> tuple[str, str]:
    """Return the repository to push to and the head ref the PR should name."""
    target = pr_helpers.ensure_fork(repo_slug, timeout=timeout)
    owner = pr_helpers.repo_owner(target)
    return target, f"{owner}:{head}" if owner else head


def _commit_and_push(repo_dir: Path, branch: str, push_target: str, *, timeout: int) -> None:
    """Commit the regenerated README on a new branch and push it to ``push_target``.

    The remote is pushed as a URL rather than as a named remote so the token the
    runner already holds is used and no credential is written into the
    repository's config, where a later step could leak it.
    """
    steps = (
        (["-C", str(repo_dir), "checkout", "-q", "-b", branch], "checkout -b"),
        (["-C", str(repo_dir), "add", "README.md"], "add"),
        (
            [
                "-C",
                str(repo_dir),
                "-c",
                "user.email=actions@github.com",
                "-c",
                "user.name=github-actions[bot]",
                "commit",
                "-q",
                "-m",
                PR_TITLE,
            ],
            "commit",
        ),
        (
            [
                "-C",
                str(repo_dir),
                "push",
                "--quiet",
                "--force",
                f"https://github.com/{push_target}",
                f"{branch}:{branch}",
            ],
            "push",
        ),
    )
    for args, label in steps:
        result = _git(args, timeout=timeout)
        if result.returncode != 0:
            raise RegenError(f"git {label} failed: {(result.stderr or result.stdout).strip()}")


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--catalog",
        type=Path,
        default=REPO_ROOT / "dcc-mcp-catalog.yml",
        help="Path to dcc-mcp-catalog.yml (default: the catalog in this repository).",
    )
    parser.add_argument(
        "--only",
        help="Comma-separated adapter names to regenerate; default is every catalog adapter.",
    )
    parser.add_argument(
        "--workdir",
        type=Path,
        help="Directory for the temporary clones (default: a fresh temp directory).",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=DEFAULT_TIMEOUT_SECS,
        help=f"Per-git-operation timeout in seconds (default: {DEFAULT_TIMEOUT_SECS}).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Report what would change without writing, pushing or opening anything.",
    )
    parser.add_argument(
        "--summary",
        action="store_true",
        help="Write a Markdown summary to $GITHUB_STEP_SUMMARY when it is set.",
    )
    parser.add_argument(
        "--json-out",
        type=Path,
        help="Write the per-repository result rows to this path as JSON.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    """Regenerate adapter pointer blocks and open or refresh one PR per repository."""
    args = _build_parser().parse_args(argv)

    try:
        catalog = generator.load_catalog(args.catalog)
    except generator.CoverageError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    entries = generator.adapter_entries(catalog)
    if not entries:
        print(f"error: {args.catalog} contains no adapter entries", file=sys.stderr)
        return 2

    only = None
    if args.only:
        only = {part.strip().lower() for part in args.only.split(",") if part.strip()}
        known = {str(e.get("name", "")).strip().lower() for e in entries}
        unknown = sorted(only - known)
        if unknown:
            print(f"error: catalog has no adapter named {unknown}", file=sys.stderr)
            return 2

    host_count = len(entries)
    source_ref = f"{generator.SOURCE_REPO}@{args.catalog.name}"
    managed = tempfile.TemporaryDirectory(prefix="adapter-pointer-regen-")
    results = []
    try:
        workdir = args.workdir or Path(managed.name)
        workdir.mkdir(parents=True, exist_ok=True)
        for index, entry in enumerate(entries):
            name = str(entry.get("name") or "").strip()
            if only is not None and name.lower() not in only:
                continue
            results.append(
                regenerate_repository(
                    entry,
                    index,
                    host_count=host_count,
                    workdir=workdir,
                    timeout=args.timeout,
                    branch_prefix=BRANCH_PREFIX,
                    dry_run=args.dry_run,
                    source_ref=source_ref,
                )
            )
    finally:
        managed.cleanup()

    return _report(results, args, host_count=host_count)


def _report(results: list[dict], args, *, host_count: int) -> int:
    """Print, summarise and serialise the run; return the process exit code."""
    import json
    import os

    for row in results:
        marker = "ok  " if row["status"] in (CURRENT, REGENERATED) else "FAIL"
        action = f" [{row['action']}]" if row["action"] != "none" else ""
        print(f"{marker} {row['name']:<32} {row['status']:<12} {row['detail']}{action}")

    regenerated = [r for r in results if r["status"] == REGENERATED]
    errored = [r for r in results if r["status"] == ERROR]
    missing = [r for r in results if r["status"] == MISSING]

    summary = render_summary(results, host_count=host_count, dry_run=args.dry_run)
    if args.summary:
        summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
        if summary_path:
            with Path(summary_path).open("a", encoding="utf-8") as handle:
                handle.write(summary)
        else:
            print()
            print(summary)

    if args.json_out:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(
            json.dumps(
                {"adapter_count": host_count, "dry_run": args.dry_run, "results": results},
                indent=2,
                ensure_ascii=False,
            )
            + "\n",
            encoding="utf-8",
        )

    print()
    verb = "would regenerate" if args.dry_run else "regenerated"
    print(
        f"{len(results)} repositories checked against a catalog of {host_count} adapters:"
        f" {len(regenerated)} {verb}, {len(missing)} without a README,"
        f" {len(errored)} failed."
    )

    if args.dry_run:
        # A dry run exists to answer "would this batch do anything?", so pending
        # work is the expected finding and must be visible in the exit code.
        # An error is also reported as non-zero: a repository this run could not
        # even read is not a clean result, and silently exiting 0 would let a
        # broken scan look like an empty one.
        if regenerated:
            print("Dry run: the repositories above would be refreshed. Re-run without --dry-run.")
            return 1
        if errored:
            print("Dry run: some repositories could not be read; see the errors above.")
            return 1
        return 0

    if errored:
        for row in errored:
            print(f"::error title=regeneration failed in {row['name']}::{row['detail']} ({row['url']})")
        return 1
    return 0


def render_summary(results: list[dict], *, host_count: int, dry_run: bool) -> str:
    """Render a Markdown table for a GitHub job summary."""
    verb = "would refresh" if dry_run else "refreshed"
    lines = [
        "## Adapter pointer regeneration",
        "",
        f"Catalog adapter count: **{host_count}**",
        "",
        "| Adapter | Status | Action | Pull request | Detail |",
        "|---|---|---|---|---|",
    ]
    for row in results:
        pr = f"[PR]({row['pr']})" if row["pr"] else "—"
        lines.append(f"| `{row['name']}` | {row['status']} | {row['action']} | {pr} | {row['detail']} |")

    regenerated = [r for r in results if r["status"] == REGENERATED]
    errored = [r for r in results if r["status"] == ERROR]
    missing = [r for r in results if r["status"] == MISSING]

    lines.append("")
    if regenerated:
        lines.append(f"**{len(regenerated)} of {len(results)} repositories {verb}.**")
    else:
        lines.append(f"All {len(results)} scanned repositories already match the catalog output.")
    if errored:
        lines.append("")
        lines.append(
            f"{len(errored)} repositories could not be regenerated. These need"
            " manual attention; the guard rail refused to commit anything it did"
            " not recognise."
        )
    if missing:
        lines.append("")
        lines.append(f"{len(missing)} repositories have no README.md to regenerate.")
    lines.append("")
    return "\n".join(lines)


if __name__ == "__main__":
    sys.exit(main())
