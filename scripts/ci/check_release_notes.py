#!/usr/bin/env python3
"""Fail when user-visible commits are missing from a release's generated notes.

release-please builds the notes from conventional-commit titles, so a commit can
ship in a version and still never reach a user. Two mechanisms produce that
silence, and both were observed for real in v0.20.34:

``A`` - untyped commit
    The squash title carries no ``<type>[(scope)]:`` prefix, so release-please
    drops the commit outright. ``521df1fa`` (PR #2519) shipped the v2 install
    SOP schema under a plain title and turned six downstream adapters red while
    the release notes said nothing.

``B`` - post-notes commit
    The notes are a snapshot. A commit that lands on ``main`` after
    release-please last regenerated them, but before the release PR is merged,
    is inside the tag yet absent from the notes. ``dc0e6c21`` (PR #2547) and
    ``c4369107`` (PR #2548) were swallowed that way.

The gate stays quiet about the commit types release-please hides
(``style`` / ``chore`` / ``test`` / ``ci`` / ``build``): those are absent from
the notes by design, and flagging them would get the gate switched off. The
hidden set is read from ``release-please-config.json`` so it can never drift
from the generator.

Notes are read from every source given with ``--notes-file`` (a release PR body,
a published release body, stdin via ``-``) plus, when ``--include-changelog`` is
set or no file is given, the ``CHANGELOG.md`` section release-please wrote for
the version. A commit counts as documented when it appears in any of them, so a
release-body addendum is a valid remediation for an already-published release.

The check is read-only: it never edits ``CHANGELOG.md``, never touches a tag or
a release, and never changes a version.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import re
import subprocess
import sys

RELEASE_CONFIG_NAME = "release-please-config.json"
CHANGELOG_NAME = "CHANGELOG.md"

#: Shortest sha abbreviation release-please (or a hand-written addendum) uses.
MIN_ABBREV = 7
#: Abbreviation length this repository renders in ``git log --oneline``.
SHORT_SHA_LEN = 8

# ``## [0.20.34](https://github.com/<repo>/compare/v0.20.33...v0.20.34) (date)``
_COMPARE_RE = re.compile(r"^##\s+\[([^\]]+)\]\([^)]*compare/([^)\s]+)\.\.\.([^)\s]+)\)", re.MULTILINE)
# Any version heading; the changelog is newest-first, so the first hit wins.
_VERSION_HEADING_RE = re.compile(r"^##\s+\[(\d[^\]]*)", re.MULTILINE)
# ``<type>[(<scope>)][!]: <description>``
_CONVENTIONAL_RE = re.compile(r"^([a-zA-Z]+)(\(.*\))?!?:")
# The release commit itself: ``chore(main): release 0.20.34 (#2505)``.
_RELEASE_COMMIT_RE = re.compile(r"^chore(\([^)]*\))?!?:\s*release\s+(\S+)", re.IGNORECASE)
# Squash-merged commits and release note entries both carry the PR number.
_PR_RE = re.compile(r"\(#(\d+)\)")

_HEX_DIGITS = frozenset("0123456789abcdefABCDEF")


class ReleaseNotesError(ValueError):
    """Raised when the gate cannot be evaluated (bad input, not a notes gap)."""


@dataclass(frozen=True)
class Commit:
    """One non-merge commit inside the release window."""

    sha: str
    subject: str

    @property
    def short_sha(self) -> str:
        """Return the abbreviation this repository renders in logs."""
        return self.sha[:SHORT_SHA_LEN]

    @property
    def pr_number(self) -> str | None:
        """Return the PR number the squash title references, if any."""
        match = _PR_RE.search(self.subject)
        return match.group(1) if match is not None else None

    @property
    def pr_label(self) -> str:
        """Return a human label for the pull request that carried the commit."""
        number = self.pr_number
        return f"PR #{number}" if number is not None else "no PR reference"


@dataclass(frozen=True)
class ReleaseNotesReport:
    """Outcome of one release-notes cross-check."""

    version: str
    prev_ref: str
    release_ref: str
    checked: tuple[Commit, ...] = ()
    release_commits: tuple[Commit, ...] = ()
    hidden: tuple[Commit, ...] = ()
    untyped: tuple[Commit, ...] = ()
    undocumented: tuple[Commit, ...] = ()

    @property
    def failures(self) -> tuple[Commit, ...]:
        """Return every commit the gate rejects, mechanism A before B."""
        return self.untyped + self.undocumented


def load_hidden_types(config_path: Path) -> frozenset[str]:
    """Return the conventional-commit types release-please hides from notes.

    Read from ``changelog-sections`` instead of hard-coded so the gate cannot
    disagree with the generator about what "intentionally absent" means.
    """
    try:
        payload = json.loads(config_path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise ReleaseNotesError(f"cannot read {config_path}: {exc}") from exc

    sections = payload.get("changelog-sections")
    if not isinstance(sections, list):
        raise ReleaseNotesError(f"{config_path} declares no changelog-sections list")

    hidden = set()
    for section in sections:
        if not isinstance(section, dict) or section.get("hidden") is not True:
            continue
        type_name = section.get("type")
        if isinstance(type_name, str) and type_name:
            hidden.add(type_name.lower())
    if not hidden:
        raise ReleaseNotesError(
            f"{config_path} declares no hidden changelog-sections; refusing to guess which types are absent on purpose"
        )
    return frozenset(hidden)


def commit_type(subject: str) -> str | None:
    """Return the lower-cased conventional-commit type of ``subject``."""
    match = _CONVENTIONAL_RE.match(subject)
    return match.group(1).lower() if match is not None else None


def is_release_commit(subject: str) -> str | None:
    """Return the version a ``chore(...): release <version>`` commit publishes."""
    match = _RELEASE_COMMIT_RE.match(subject)
    return match.group(2).strip() if match is not None else None


def is_documented(sha: str, notes: str) -> bool:
    """Return True when ``notes`` cites ``sha`` at any abbreviation length.

    Notes cite a commit either as a short sha inside a link label or as the full
    sha inside a ``/commit/<sha>`` URL. Neighbouring characters must not be hex,
    so a 7-character prefix cannot match itself inside a longer sha.
    """
    if not _HEX_DIGITS.issuperset(sha):
        raise ReleaseNotesError(f"{sha!r} is not a hex commit sha")
    for length in range(MIN_ABBREV, len(sha) + 1):
        abbrev = sha[:length]
        start = notes.find(abbrev)
        while start >= 0:
            end = start + length
            before = notes[start - 1] if start > 0 else ""
            after = notes[end] if end < len(notes) else ""
            if before not in _HEX_DIGITS and after not in _HEX_DIGITS:
                return True
            start = notes.find(abbrev, start + 1)
    return False


def changelog_section(changelog_text: str, version: str) -> str | None:
    """Return the ``CHANGELOG.md`` section release-please wrote for ``version``."""
    start = changelog_text.find(f"## [{version}]")
    if start < 0:
        return None
    next_heading = changelog_text.find("\n## [", start + 1)
    return changelog_text[start:] if next_heading < 0 else changelog_text[start:next_heading]


def previous_version(changelog_text: str, version: str) -> str | None:
    """Return the ref the ``CHANGELOG.md`` compare link marks as the previous release."""
    for match in _COMPARE_RE.finditer(changelog_text):
        if match.group(1) == version:
            return match.group(2)
    return None


def newest_version(changelog_text: str) -> str | None:
    """Return the newest version heading in a newest-first ``CHANGELOG.md``.

    Used when the release ref is not the release commit itself, which happens
    on a release PR branch that carries extra commits (for example a lock
    refresh) pushed after release-please wrote the release commit.
    """
    match = _VERSION_HEADING_RE.search(changelog_text)
    return match.group(1) if match is not None else None


def read_commits(root: Path, prev_ref: str, release_ref: str) -> list[Commit]:
    """Return the non-merge commits in ``prev_ref..release_ref``, newest first."""
    completed = subprocess.run(
        ["git", "log", f"{prev_ref}..{release_ref}", "--no-merges", "--format=%H%x09%s"],
        cwd=str(root),
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise ReleaseNotesError(
            f"git log {prev_ref}..{release_ref} failed in {root}: {completed.stderr.strip() or 'unknown error'}"
        )
    commits = []
    for line in completed.stdout.splitlines():
        if not line.strip():
            continue
        sha, _, subject = line.partition("\t")
        commits.append(Commit(sha=sha.strip(), subject=subject.strip()))
    return commits


def check_commits(
    commits: list[Commit],
    notes: str,
    hidden_types: frozenset[str],
    *,
    version: str = "",
    prev_ref: str = "",
    release_ref: str = "",
) -> ReleaseNotesReport:
    """Split a release window into documented, hidden, and missing commits."""
    release_commits: list[Commit] = []
    hidden: list[Commit] = []
    untyped: list[Commit] = []
    undocumented: list[Commit] = []
    checked: list[Commit] = []

    for commit in commits:
        if is_release_commit(commit.subject) is not None:
            release_commits.append(commit)
            continue
        type_name = commit_type(commit.subject)
        if type_name is None:
            # Mechanism A: no type, so release-please never saw this commit.
            untyped.append(commit)
            continue
        if type_name in hidden_types:
            hidden.append(commit)
            continue
        checked.append(commit)
        if not is_documented(commit.sha, notes):
            # Mechanism B: visible type, but the notes snapshot predates it.
            undocumented.append(commit)

    return ReleaseNotesReport(
        version=version,
        prev_ref=prev_ref,
        release_ref=release_ref,
        checked=tuple(checked),
        release_commits=tuple(release_commits),
        hidden=tuple(hidden),
        untyped=tuple(untyped),
        undocumented=tuple(undocumented),
    )


def format_errors(report: ReleaseNotesReport) -> list[str]:
    """Return one error per missing commit, mechanism A before mechanism B."""
    label = report.version or report.release_ref
    errors = []
    for commit in report.untyped:
        errors.append(
            f"[A:untyped-commit] {commit.short_sha} ({commit.pr_label}) {commit.subject!r} is user-visible but "
            f"missing from the {label} release notes: the commit title has no conventional-commit type prefix, so "
            'release-please dropped it. Fix: retitle the commit/PR as "<type>(<scope>): ..." and regenerate the '
            f"release PR."
        )
    for commit in report.undocumented:
        errors.append(
            f"[B:post-notes-commit] {commit.short_sha} ({commit.pr_label}) {commit.subject!r} is user-visible but "
            f"missing from the {label} release notes: it landed after the notes were last generated. Fix: regenerate "
            f"the release PR before merging, or publish a release-body addendum citing {commit.short_sha}."
        )
    return errors


def _git_subject(root: Path, ref: str) -> str:
    completed = subprocess.run(
        ["git", "log", "-1", "--format=%s", ref],
        cwd=str(root),
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip() if completed.returncode == 0 else ""


def _detect_version(root: Path, release_ref: str) -> str | None:
    """Return the version the release ref publishes, without needing a tag name."""
    subject = _git_subject(root, release_ref)
    version = is_release_commit(subject)
    if version:
        return version.lstrip("v")
    # A tag ref is a usable fallback even when the tip is not the release commit.
    if release_ref.startswith("v") and release_ref[1:2].isdigit():
        return release_ref[1:]
    return None


def _resolve_version(args_version: str, root: Path, release_ref: str, changelog_text: str) -> str:
    """Return the release version, and fail loudly when none can be derived."""
    if args_version:
        return args_version
    version = _detect_version(root, release_ref) or newest_version(changelog_text)
    if not version:
        raise ReleaseNotesError(
            f"cannot determine the version released by {release_ref}; pass --version (for example --version 0.20.34)"
        )
    return version


def _resolve_prev_ref(root: Path, changelog_text: str, version: str, release_ref: str) -> str:
    """Return the ref the release window starts at."""
    from_changelog = previous_version(changelog_text, version)
    if from_changelog:
        return from_changelog
    completed = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0", f"{release_ref}^"],
        cwd=str(root),
        capture_output=True,
        text=True,
    )
    if completed.returncode == 0 and completed.stdout.strip():
        return completed.stdout.strip()
    raise ReleaseNotesError(
        f"cannot resolve the previous release tag for {version}; pass --prev-tag "
        f"(git describe failed: {completed.stderr.strip() or 'no tag reachable'})"
    )


def _read_notes_file(path: Path) -> str:
    if str(path) == "-":
        return sys.stdin.read()
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ReleaseNotesError(f"cannot read notes file {path}: {exc}") from exc


def main(argv: list[str] | None = None) -> int:
    """Cross-check a release window against the notes generated for it."""
    parser = argparse.ArgumentParser(description="Fail when user-visible commits are missing from release notes.")
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="repository root (default: cwd)")
    parser.add_argument("--prev-tag", default="", help="ref the release window starts at (default: auto)")
    parser.add_argument("--release-tag", default="", help="ref the release window ends at (default: HEAD)")
    parser.add_argument("--version", default="", help="release version, for example 0.20.34 (default: auto)")
    parser.add_argument(
        "--notes-file",
        type=Path,
        action="append",
        default=[],
        help="release notes to check against (repeatable; '-' reads stdin)",
    )
    parser.add_argument(
        "--include-changelog",
        action="store_true",
        help="also accept the CHANGELOG.md section release-please wrote for the version",
    )
    parser.add_argument("--changelog", type=Path, help=f"changelog path (default: <root>/{CHANGELOG_NAME})")
    parser.add_argument("--config", type=Path, help=f"release-please config (default: <root>/{RELEASE_CONFIG_NAME})")
    args = parser.parse_args(argv)

    root = args.root
    hidden_types = load_hidden_types(args.config or root / RELEASE_CONFIG_NAME)
    release_ref = args.release_tag or "HEAD"

    changelog_path = args.changelog or root / CHANGELOG_NAME
    try:
        changelog_text = changelog_path.read_text(encoding="utf-8")
    except OSError:
        changelog_text = ""

    version = _resolve_version(args.version, root, release_ref, changelog_text)
    prev_ref = args.prev_tag or _resolve_prev_ref(root, changelog_text, version, release_ref)

    sources = [_read_notes_file(path) for path in args.notes_file]
    if args.include_changelog or not sources:
        section = changelog_section(changelog_text, version)
        if section is None:
            raise ReleaseNotesError(
                f"{changelog_path} has no '## [{version}]' section; pass --notes-file with the notes to check"
            )
        sources.append(section)

    report = check_commits(
        read_commits(root, prev_ref, release_ref),
        "\n".join(sources),
        hidden_types,
        version=version,
        prev_ref=prev_ref,
        release_ref=release_ref,
    )

    print(f"Release notes cross-check: {version} ({prev_ref}..{release_ref})")
    print(f"  user-visible commits checked: {len(report.checked)}")
    print(f"  release commits skipped: {len(report.release_commits)}")
    print(f"  hidden-type commits skipped: {len(report.hidden)}")
    print(f"  undocumented commits: {len(report.failures)}")

    errors = format_errors(report)
    if errors:
        for error in errors:
            print(f"::error::{error}", file=sys.stderr)
        print(
            f"::error::{len(errors)} user-visible commit(s) shipped in {version} without reaching its release notes. "
            f"Regenerate the release PR, or add a release-body addendum citing each commit.",
            file=sys.stderr,
        )
        return 1
    print(f"Release notes cover every user-visible commit in {version}.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ReleaseNotesError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        raise SystemExit(2) from exc
