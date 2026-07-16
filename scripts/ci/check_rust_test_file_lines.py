#!/usr/bin/env python3
"""Keep Rust test modules from growing into hard-to-review god files."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

MAX_TEST_LINES = 2000


def is_rust_test_file(path: Path) -> bool:
    """Return whether a Rust source path is a test module or integration test."""
    parts = set(path.parts)
    name = path.name
    return "tests" in parts or name == "tests.rs" or name.endswith("_tests.rs")


def rel_posix(path: Path, root: Path) -> str:
    """Return a repository-relative POSIX path for stable CI diagnostics."""
    return path.relative_to(root).as_posix()


def line_count(path: Path) -> int:
    """Count physical lines using the same broad semantics as wc -l."""
    with path.open("rb") as handle:
        return sum(1 for _ in handle)


def load_exemptions(root: Path) -> set[str]:
    """Load file paths exempted from the size check."""
    exemptions_file = root / ".github" / "file-size-exemptions.txt"
    if not exemptions_file.is_file():
        return set()
    exempt: set[str] = set()
    with exemptions_file.open("r", encoding="utf-8") as handle:
        for raw in handle:
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            exempt.add(line)
    return exempt


def main() -> int:
    """Run the Rust test file length check."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".", help="Repository root")
    parser.add_argument("--max-lines", type=int, default=MAX_TEST_LINES)
    args = parser.parse_args()

    root = Path(args.root).resolve()
    exemptions = load_exemptions(root)
    failures: list[str] = []

    for path in sorted((root / "crates").rglob("*.rs")):
        if not is_rust_test_file(path):
            continue

        rel = rel_posix(path, root)
        if rel in exemptions:
            lines = line_count(path)
            print(f"⚠️  EXEMPT  {rel} ({lines} lines, max {args.max_lines})")
            continue

        lines = line_count(path)
        if lines > args.max_lines:
            failures.append(f"{rel} has {lines} lines; max is {args.max_lines}.")

    if failures:
        print("Rust test file length check failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"All Rust test files are within {args.max_lines} lines.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
