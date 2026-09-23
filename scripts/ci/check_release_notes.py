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

Breaking markers and the types release-please hides
--------------------------------------------------

The gate stays quiet about the commit types release-please hides
(``style`` / ``chore`` / ``test`` / ``ci`` / ``build``): those are absent from
the notes by design, and flagging them would get the gate switched off. The
hidden set is read from ``release-please-config.json`` so it can never drift
from the generator.

A breaking-change marker is a partial exception, because the generator does
not actually hide a breaking commit. ``conventional-changelog-conventionalcommits``
sets ``discard = false`` for any commit carrying a note, so a hidden type is
only dropped when it carries none - its own source comment says "breaking
changes attached to any type are still displayed", and it ships
``add-bang-notes.js`` to synthesise that note for exactly this case:

    // for the special case, test(system)!: hello world, where there is
    // a '!' but no 'BREAKING CHANGE' in body

Whether this gate follows the generator is decided per type by one question:
**can this type carry a breaking change a downstream consumer can feel?**

============  ==================  ===========================================
type           ``!`` overrides     why
============  ==================  ===========================================
``chore``      **yes**             Build, dependency, packaging and
                                   minimum-version policy (dropping a wheel,
                                   a Python version, an install layout) land
                                   on everyone who installs the package.
``build``      **yes**             Same family: wheel tags, matrix layout and
                                   packaging metadata are shipping surface.
``test``       no                  Test fixtures and assertions. The only
                                   breakage is contributor-side.
``style``      no                  Formatting cannot change behaviour.
``ci``         no                  Pipeline wiring. It gates contributors, not
                                   consumers of a published artefact.
============  ==================  ===========================================

``test!`` and ``style!`` are a deliberate policy choice, not a claim about the
generator: the generator *does* render them, so this gate under-reports those
two types on purpose. What buys that tolerance is the false-positive budget -
the only thing that keeps a release gate switched on. The full history holds
exactly one such commit, ``16d5c287`` ``test(skills)!: align fixtures with
nested metadata.dcc-mcp.* contract``, and it is a contributor-facing fixture
migration, so it is the case the table exists to absorb rather than the case
the table should chase.

**Adding an exception.** Add the type to ``BREAKING_CAPABLE_HIDDEN_TYPES`` and
re-run the full-history replay (every consecutive release-tag window) to prove
the change adds zero new alarms. A new alarm is only acceptable when it names
a commit a real downstream consumer can feel; if it names a fixture or
formatting change, the table is wrong, not the commit. Do not widen the set to
match the generator wholesale - ``test!`` is the counter-example that would
cost an alarm and buy nothing.

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

CR = "\r"
CRLF = CR + "\n"

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
# A breaking-change marker (``feat!:`` / ``fix(api)!:``) makes release-please
# render the commit even when its type is not declared in changelog-sections.
_BREAKING_RE = re.compile(r"^[a-zA-Z]+(\(.*\))?!:")
# ``BREAKING CHANGE: ...`` / ``BREAKING-CHANGE: ...`` in the commit body. The
# generator accepts a footer note exactly like a ``!``, so both are read.
_BREAKING_FOOTER_RE = re.compile(r"^BREAKING[ -]CHANGE\s*:", re.MULTILINE)

#: Hidden types whose breaking changes reach a downstream consumer. A ``!``
#: (or a ``BREAKING CHANGE:`` footer) on these overrides the hidden section;
#: on every other hidden type it does not. See the module docstring.
BREAKING_CAPABLE_HIDDEN_TYPES = frozenset({"chore", "build"})
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
    body: str = ""

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
    undeclared: tuple[Commit, ...] = ()
    undocumented: tuple[Commit, ...] = ()
    visible_types: frozenset[str] = frozenset()

    @property
    def failures(self) -> tuple[Commit, ...]:
        """Return every commit the gate rejects, mechanism A before B."""
        return self.untyped + self.undeclared + self.undocumented


def read_text_lf(path: Path) -> str:
    """Read a UTF-8 file with explicit LF newline handling.

    ``Path.read_text()`` translates CRLF to LF on the way in and the matching
    ``Path.write_text()`` translates LF back to CRLF on the way out, so a
    read/write round trip rewrites every LF file as CRLF on Windows. Reading
    bytes and normalizing here keeps the newline handling explicit: this gate
    only consumes text, and a future write path has to choose its own line
    endings instead of inheriting the platform default.
    """
    data = path.read_bytes()
    text = data.decode("utf-8-sig", errors="replace")
    return text.replace(CRLF, "\n").replace(CR, "\n")


def load_changelog_types(config_path: Path) -> tuple[frozenset[str], frozenset[str]]:
    """Return the ``(hidden, visible)`` types a release-please config declares.

    Read from ``changelog-sections`` instead of hard-coded so the gate cannot
    disagree with the generator about what "intentionally absent" means. A type
    the config never declares is also dropped by release-please, which makes it
    a mechanism-A problem rather than a stale-notes problem.
    """
    try:
        payload = json.loads(read_text_lf(config_path))
    except (OSError, ValueError) as exc:
        raise ReleaseNotesError(f"cannot read {config_path}: {exc}") from exc

    sections = payload.get("changelog-sections")
    if not isinstance(sections, list):
        raise ReleaseNotesError(f"{config_path} declares no changelog-sections list")

    hidden: set[str] = set()
    visible: set[str] = set()
    for section in sections:
        if not isinstance(section, dict):
            continue
        type_name = section.get("type")
        if not isinstance(type_name, str) or not type_name:
            continue
        if section.get("hidden") is True:
            hidden.add(type_name.lower())
        else:
            visible.add(type_name.lower())
    if not hidden:
        raise ReleaseNotesError(
            f"{config_path} declares no hidden changelog-sections; refusing to guess which types are absent on purpose"
        )
    return frozenset(hidden), frozenset(visible)


def load_hidden_types(config_path: Path) -> frozenset[str]:
    """Return the conventional-commit types release-please hides from notes."""
    return load_changelog_types(config_path)[0]


def commit_type(subject: str) -> str | None:
    """Return the lower-cased conventional-commit type of ``subject``."""
    match = _CONVENTIONAL_RE.match(subject)
    return match.group(1).lower() if match is not None else None


def is_breaking(subject: str, body: str = "") -> bool:
    """Return True when the commit carries a breaking-change marker.

    Two spellings, because the generator accepts both and treats them alike: the
    ``!`` marker in the title, and a ``BREAKING CHANGE:`` footer in the body. The
    footer is only visible to a caller that has the body; a caller holding just a
    subject keeps the title-only answer.
    """
    if _BREAKING_RE.match(subject) is not None:
        return True
    return bool(body) and _BREAKING_FOOTER_RE.search(body) is not None


def is_user_visible_breaking(type_name: str, subject: str, body: str = "") -> bool:
    """Return True when a hidden-type commit is breaking enough to be checked.

    Hidden means "release-please leaves this out on purpose", and this gate honours
    that. A breaking marker reverses the omission, but only for the types that can
    carry a change a downstream consumer can feel - see
    ``BREAKING_CAPABLE_HIDDEN_TYPES`` and the table in the module docstring.
    """
    return type_name in BREAKING_CAPABLE_HIDDEN_TYPES and is_breaking(subject, body)


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
    """Return the non-merge commits in ``prev_ref..release_ref``, newest first.

    Each record also carries the commit body, so a ``BREAKING CHANGE:`` footer
    is visible to ``is_breaking()``. Records are NUL-delimited because a body
    spans lines: splitting on newlines would cut every multi-line commit into
    several bogus ones.
    """
    completed = subprocess.run(
        ["git", "log", f"{prev_ref}..{release_ref}", "--no-merges", "--format=%H%x1f%s%x1f%b%x00"],
        cwd=str(root),
        capture_output=True,
        # Commit subjects carry typographic characters (em dashes, accents);
        # decoding with the host locale turns a non-UTF-8 console into a crash
        # whose exit code is indistinguishable from a real notes gap.
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if completed.returncode != 0:
        raise ReleaseNotesError(
            f"git log {prev_ref}..{release_ref} failed in {root}: {completed.stderr.strip() or 'unknown error'}"
        )
    commits = []
    for record in completed.stdout.split("\x00"):
        if not record.strip():
            continue
        sha, subject, body, *_ = [*record.split("\x1f", 2), "", ""]
        commits.append(Commit(sha=sha.strip(), subject=subject.strip(), body=body.strip()))
    return commits


def check_commits(
    commits: list[Commit],
    notes: str,
    hidden_types: frozenset[str],
    *,
    visible_types: frozenset[str] | None = None,
    version: str = "",
    prev_ref: str = "",
    release_ref: str = "",
) -> ReleaseNotesReport:
    """Split a release window into documented, hidden, and missing commits.

    ``visible_types`` are the types the release-please config declares with
    ``hidden: false``. When given, a commit whose type the config never
    declares is reported as mechanism A: release-please drops it for the same
    reason it drops an untyped title, so "regenerate the release PR" would send
    the author the wrong way. A breaking-change marker exempts the commit,
    because release-please renders breaking commits regardless of their type.

    A hidden type is exempt the other way round: the marker only pulls it back
    into the checked set when ``is_user_visible_breaking`` accepts its type.
    """
    release_commits: list[Commit] = []
    hidden: list[Commit] = []
    untyped: list[Commit] = []
    undeclared: list[Commit] = []
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
        if type_name in hidden_types and not is_user_visible_breaking(type_name, commit.subject, commit.body):
            hidden.append(commit)
            continue
        if (
            visible_types is not None
            and type_name not in visible_types
            and not is_breaking(commit.subject, commit.body)
        ):
            # Mechanism A: undeclared type, so the generator drops it too.
            undeclared.append(commit)
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
        undeclared=tuple(undeclared),
        undocumented=tuple(undocumented),
        visible_types=visible_types if visible_types is not None else frozenset(),
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
    for commit in report.undeclared:
        declared = "/".join(sorted(report.visible_types)) or "feat/fix/perf/refactor/docs"
        errors.append(
            f"[A:undeclared-type] {commit.short_sha} ({commit.pr_label}) {commit.subject!r} is user-visible but "
            f"missing from the {label} release notes: its type is not declared in release-please-config.json "
            f"changelog-sections, so release-please drops it and regenerating the release PR will not help. "
            f"Fix: retitle the commit/PR with a declared type ({declared}), or declare the type in the config."
        )
    for commit in report.undocumented:
        errors.append(
            f"[B:post-notes-commit] {commit.short_sha} ({commit.pr_label}) {commit.subject!r} is user-visible but "
            f"missing from the {label} release notes: it landed after the notes were last generated. Fix: regenerate "
            f"the release PR before merging, or publish a release-body addendum citing {commit.short_sha}. Once the "
            f"tag exists neither works; re-run the release workflow with release_tag to republish assets, which "
            f"skips this gate."
        )
    return errors


def _git_subject(root: Path, ref: str) -> str:
    completed = subprocess.run(
        ["git", "log", "-1", "--format=%s", ref],
        cwd=str(root),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
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
        encoding="utf-8",
        errors="replace",
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
        return read_text_lf(path)
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
    hidden_types, visible_types = load_changelog_types(args.config or root / RELEASE_CONFIG_NAME)
    release_ref = args.release_tag or "HEAD"

    changelog_path = args.changelog or root / CHANGELOG_NAME
    try:
        changelog_text = read_text_lf(changelog_path)
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
        visible_types=visible_types,
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
