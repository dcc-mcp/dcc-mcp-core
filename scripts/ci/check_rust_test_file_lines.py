#!/usr/bin/env python3
"""Keep Rust test modules from growing into hard-to-review god files."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

MAX_TEST_LINES = 2000

# Existing oversized Rust test modules. These are allowed to stay at or below
# the recorded baseline, but any growth must be split into focused test files.
LEGACY_BASELINES = {
    "crates/dcc-mcp-gateway/src/gateway/admin/tests.rs": 4883,
    "crates/dcc-mcp-cli/tests/cli_e2e.rs": 3342,
    "crates/dcc-mcp-gateway/src/gateway/handlers/rest_impl_tests.rs": 2141,
}


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


def main() -> int:
    """Run the Rust test file length check."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".", help="Repository root")
    parser.add_argument("--max-lines", type=int, default=MAX_TEST_LINES)
    args = parser.parse_args()

    root = Path(args.root).resolve()
    failures: list[str] = []

    for path in sorted((root / "crates").rglob("*.rs")):
        if not is_rust_test_file(path):
            continue

        rel = rel_posix(path, root)
        lines = line_count(path)
        baseline = LEGACY_BASELINES.get(rel)
        if baseline is not None:
            if lines > baseline:
                failures.append(
                    f"{rel} has {lines} lines; legacy baseline is {baseline}. "
                    "Move new tests into focused files before growing this module."
                )
            elif lines > args.max_lines:
                print(f"WARN legacy oversized Rust test file: {rel} ({lines}/{baseline} lines)")
            continue

        if lines > args.max_lines:
            failures.append(f"{rel} has {lines} lines; max is {args.max_lines}.")

    if failures:
        print("Rust test file length check failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"All Rust test files are within {args.max_lines} lines or legacy baselines.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
