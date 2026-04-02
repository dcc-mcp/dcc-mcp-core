"""Analyze repository statistics using git."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def run_git(args: list[str], cwd: str | None = None) -> str:
    """Run a git command and return stdout."""
    result = subprocess.run(
        ["git", *args],
        capture_output=True,
        timeout=30,
        cwd=cwd,
        encoding="utf-8",
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def main() -> None:
    """Gather repository statistics."""
    parser = argparse.ArgumentParser(description="Analyze repository statistics.")
    parser.add_argument("--repo", default=".")
    args = parser.parse_args()

    try:
        commit_count = run_git(["rev-list", "--count", "HEAD"], cwd=args.repo)

        contributors_raw = run_git(["shortlog", "-sn", "--no-merges", "HEAD"], cwd=args.repo)
        contributors = []
        for line in contributors_raw.splitlines():
            parts = line.strip().split("\t", 1)
            if len(parts) == 2:
                contributors.append({"commits": int(parts[0].strip()), "name": parts[1].strip()})

        files_raw = run_git(["ls-files"], cwd=args.repo)
        file_count = len(files_raw.splitlines()) if files_raw else 0

        branch = run_git(["rev-parse", "--abbrev-ref", "HEAD"], cwd=args.repo)

        latest_tag = run_git(["describe", "--tags", "--abbrev=0"], cwd=args.repo) or None

        stats = {
            "branch": branch,
            "total_commits": int(commit_count) if commit_count else 0,
            "tracked_files": file_count,
            "contributors": len(contributors),
            "top_contributors": contributors[:5],
            "latest_tag": latest_tag,
        }

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Repository: {stats['total_commits']} commits, "
                    f"{stats['tracked_files']} files, "
                    f"{stats['contributors']} contributors",
                    "context": stats,
                }
            )
        )

    except FileNotFoundError:
        print(json.dumps({"success": False, "message": "git not found."}))
        sys.exit(1)


if __name__ == "__main__":
    main()
