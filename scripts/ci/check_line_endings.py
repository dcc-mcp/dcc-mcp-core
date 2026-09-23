#!/usr/bin/env python3
"""Reject CR bytes in the committed blobs of version-controlled Python files.

Why this gate exists
--------------------
``Path.read_text()`` applies universal-newline translation on the way in and
``Path.write_text()`` reverses it on the way out. A script that round-trips a
source file through those two calls therefore rewrites every LF file as CRLF
when it runs on Windows, and the whole file shows up as a rewritten diff. That
is what happened to the release-notes gate and its tests: both files came back
as fully CRLF blobs and had to be restored by hand.

The gate inspects the committed blob instead of the working tree, so a
contributor's local ``core.autocrlf`` setting cannot hide a CRLF commit and a
CRLF file that only exists on disk is never reported.

Usage
-----
    python scripts/ci/check_line_endings.py [--root PATH] [--pattern GLOB]

Exit codes: ``0`` when no CR byte is found, ``1`` when at least one tracked
file carries CR bytes, ``2`` when git cannot answer the query.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
from typing import NamedTuple
from typing import Sequence

DEFAULT_PATTERNS: tuple[str, ...] = ("*.py",)
MAX_REPORTED = 25

REMEDIATION = """\
Fix by re-normalizing the committed blob (it honors .gitattributes, which pins *.py to LF):

    git add --renormalize <path>

or rewrite the file with LF endings and commit it. Then change the script that
produced the CRLF file to handle newlines explicitly: open(..., newline="") or
read_bytes()/write_bytes() instead of Path.read_text()/Path.write_text()."""


class CrFile(NamedTuple):
    """A tracked file whose committed blob contains at least one CR byte."""

    path: str
    cr_count: int
    byte_count: int


class LineEndingError(RuntimeError):
    """Raised when git cannot enumerate the requested blobs."""


def _run_git(root: Path, args: Sequence[str], stdin: bytes = b"") -> bytes:
    """Run a git command inside ``root`` and return its raw stdout."""
    completed = subprocess.run(
        ["git", "-C", str(root), *args],
        input=stdin,
        capture_output=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.decode("utf-8", "replace").strip()
        raise LineEndingError(f"git {' '.join(args)} failed ({completed.returncode}): {detail}")
    return completed.stdout


def _parse_index_records(listing: bytes) -> list[tuple[str, str]]:
    """Parse ``git ls-files -s -z`` output into (blob sha, path) pairs."""
    entries: list[tuple[str, str]] = []
    for record in listing.split(b"\0"):
        if not record:
            continue
        meta, tab, raw_path = record.partition(b"\t")
        if not tab:
            raise LineEndingError(f"unexpected git ls-files record: {record!r}")
        fields = meta.split(b" ")
        if len(fields) < 2:
            raise LineEndingError(f"unexpected git ls-files metadata: {meta!r}")
        entries.append((fields[1].decode("ascii", "replace"), raw_path.decode("utf-8", "surrogateescape")))
    return entries


def tracked_blobs(root: Path, patterns: Sequence[str] = DEFAULT_PATTERNS) -> list[tuple[str, bytes]]:
    """Return (path, blob) pairs for tracked files matching ``patterns``.

    Paths come from the index (NUL separated, so spaces and quotes are safe)
    and every blob is read through a single ``git cat-file --batch`` stream.
    """
    listing = _run_git(root, ["ls-files", "-s", "-z", "--", *patterns])
    entries = _parse_index_records(listing)
    if not entries:
        return []

    request = "".join(f"{sha}\n" for sha, _ in entries).encode("ascii")
    payload = _run_git(root, ["cat-file", "--batch"], stdin=request)

    pairs: list[tuple[str, bytes]] = []
    position = 0
    for sha, path in entries:
        newline = payload.find(b"\n", position)
        if newline < 0:
            raise LineEndingError(f"git cat-file --batch output ended early at {path}")
        header = payload[position:newline].decode("utf-8", "replace").split()
        if len(header) < 3 or header[1] != "blob":
            raise LineEndingError(f"git cat-file --batch could not resolve {sha} ({path})")
        try:
            size = int(header[2])
        except ValueError as error:
            raise LineEndingError(f"git cat-file --batch returned an unparsable size for {path}") from error
        start = newline + 1
        blob = payload[start : start + size]
        if len(blob) != size:
            raise LineEndingError(f"git cat-file --batch truncated the blob for {path}")
        position = start + size + 1
        pairs.append((path, blob))
    return pairs


def find_cr_files(root: Path, patterns: Sequence[str] = DEFAULT_PATTERNS) -> list[CrFile]:
    """Return every tracked file matching ``patterns`` whose blob holds a CR byte."""
    violations: list[CrFile] = []
    for path, blob in tracked_blobs(root, patterns):
        cr_count = blob.count(b"\r")
        if cr_count:
            violations.append(CrFile(path=path, cr_count=cr_count, byte_count=len(blob)))
    return violations


def format_report(violations: Sequence[CrFile], patterns: Sequence[str], max_reported: int = MAX_REPORTED) -> str:
    """Render the failure report printed to the CI job log."""
    joined = ", ".join(patterns)
    lines = [f"CRLF line endings found in {len(violations)} tracked file(s) matching {joined}:"]
    for violation in violations[:max_reported]:
        lines.append(f"  {violation.path}: {violation.cr_count} CR byte(s) in {violation.byte_count} byte(s)")
    hidden = len(violations) - min(len(violations), max_reported)
    if hidden > 0:
        lines.append(f"  ... and {hidden} more")
    lines.append("")
    lines.append(REMEDIATION)
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    """Run the line-ending gate and return the process exit code."""
    parser = argparse.ArgumentParser(description="Fail when tracked Python blobs contain CR bytes.")
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="repository root (default: cwd)")
    parser.add_argument(
        "--pattern",
        action="append",
        default=[],
        dest="patterns",
        help="pathspec to check (repeatable; default: *.py)",
    )
    parser.add_argument(
        "--max-reported",
        type=int,
        default=MAX_REPORTED,
        help="maximum number of offending files listed (default: %(default)s)",
    )
    args = parser.parse_args(argv)

    patterns = tuple(args.patterns) or DEFAULT_PATTERNS
    root = args.root

    try:
        blobs = tracked_blobs(root, patterns)
    except LineEndingError as exc:
        print(f"check_line_endings: {exc}", file=sys.stderr)
        return 2

    violations = [CrFile(path, blob.count(b"\r"), len(blob)) for path, blob in blobs if b"\r" in blob]
    if violations:
        print(format_report(violations, patterns, args.max_reported), file=sys.stderr)
        joined = ", ".join(patterns)
        print(
            f"::error::CRLF line endings in {len(violations)} tracked file(s) matching {joined}",
            file=sys.stderr,
        )
        return 1

    joined = ", ".join(patterns)
    print(f"Line endings OK: 0 CR bytes across {len(blobs)} tracked file(s) matching {joined}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
